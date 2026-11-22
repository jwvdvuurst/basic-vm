use std::cmp::Ordering;
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::time::Instant;

use crate::files::{FileTable, MAX_FILES};
use crate::heap::{Heap, HeapStats, ObjectRef, VALUE_BYTES};
use crate::stdlib::SplitMix64;
use crate::value::{Value, cmp_numeric, float_to_int};
use crate::{
    BinaryOp, CallBinding, CollectionKind, Error, FieldDefault, Op, Place, Program, VarType,
};

mod assertions;
mod builtins;
mod media;
mod testing;

use testing::TestState;
/// Largest number of elements a `DIM`/`REDIM` may allocate (counting every
/// nesting level of a multi-dimensional array).
const MAX_ARRAY_ELEMENTS: usize = 16 * 1024 * 1024;
/// Upper bound on the array objects (the array plus all its rows) one DIM or
/// REDIM may allocate.
const MAX_ARRAY_OBJECTS: usize = 1024 * 1024;
/// Deepest nesting of active function, method, constructor and event-handler
/// calls. Reaching it raises an ordinary, catchable runtime error, which is
/// how a function that calls itself without an end is reported instead of
/// exhausting memory.
pub const MAX_CALL_DEPTH: usize = 25_000;

#[derive(Clone)]
enum LoopState {
    Numeric {
        variable: String,
        end: usize,
        limit: f64,
        step: f64,
    },
    /// Exact integer loop (start and step are integers); the limit may be
    /// an integer or a double and is compared exactly.
    Integer {
        variable: String,
        end: usize,
        limit: Value,
        step: i64,
    },
    Each {
        variable: String,
        /// Value variable of the two-variable map form; the snapshot then
        /// holds interleaved key/value pairs.
        second: Option<String>,
        end: usize,
        snapshot: ObjectRef,
        index: usize,
    },
}

impl LoopState {
    fn end(&self) -> usize {
        match self {
            Self::Numeric { end, .. } | Self::Integer { end, .. } | Self::Each { end, .. } => *end,
        }
    }

    fn reference(&self) -> Option<ObjectRef> {
        match self {
            Self::Each { snapshot, .. } => Some(*snapshot),
            Self::Numeric { .. } | Self::Integer { .. } => None,
        }
    }
}

struct CallFrame {
    function: usize,
    return_pc: usize,
    stack_base: usize,
    discard_result: bool,
    locals: HashMap<String, Option<Value>>,
    caller_loops: HashMap<usize, LoopState>,
    /// BYREF parameters: (parameter index, caller place to copy out to).
    writebacks: Vec<(usize, PlaceValue)>,
}

/// A BYREF argument's caller-side place, with its operands evaluated.
#[derive(Clone)]
enum PlaceValue {
    Variable(String),
    Element(Value, Value),
    Field(Value, String),
}

/// A runtime error in flight or being handled.
#[derive(Clone)]
struct Fault {
    line: usize,
    file: usize,
    function: Option<String>,
    message: String,
    /// Raised by a failed `ASSERT*` statement.
    assertion: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Body,
    Catch,
    Finally,
}

/// What a FINALLY block must do when it ends.
enum Pending {
    Resume,
    Leave { target: usize, tries: usize },
    Raise(Fault),
}

/// One active TRY block.
struct Handler {
    catch: Option<usize>,
    finally: Option<usize>,
    frames: usize,
    stack: usize,
    phase: Phase,
    /// The error being handled while in the CATCH phase.
    error: Option<Fault>,
    pending: Pending,
}

pub struct Vm {
    program: Program,
    heap: Heap,
    variables: HashMap<String, Value>,
    /// Declared types of main-program variables (`DIM`).
    var_types: HashMap<String, VarType>,
    stack: Vec<Value>,
    pc: usize,
    loops: HashMap<usize, LoopState>,
    frames: Vec<CallFrame>,
    handlers: Vec<Handler>,
    /// Set by a rethrow so the original line and function are preserved.
    error_origin: Option<(usize, usize, Option<String>)>,
    /// Set while an assertion failure (or a rethrown one) propagates out of
    /// `execute`; consumed when the fault is created.
    assertion_failure: bool,
    files: FileTable,
    rng: SplitMix64,
    started: Instant,
    /// Test mode (see `testing.rs`): `SLEEP` does not wait.
    test: TestState,
    arguments: Vec<String>,
    media: media::Media,
}

impl Vm {
    pub fn new(program: Program) -> Result<Self, Error> {
        Ok(Self {
            program,
            heap: Heap::new().map_err(|message| Error::new(1, message))?,
            variables: HashMap::new(),
            var_types: HashMap::new(),
            stack: Vec::new(),
            pc: 0,
            loops: HashMap::new(),
            frames: Vec::new(),
            handlers: Vec::new(),
            error_origin: None,
            assertion_failure: false,
            files: FileTable::default(),
            rng: SplitMix64::new(SplitMix64::DEFAULT_SEED),
            started: Instant::now(),
            test: TestState::default(),
            arguments: Vec::new(),
            media: media::Media::default(),
        })
    }

    /// Sets the program arguments visible through `ARGC()` and `ARG$(n)`.
    pub fn set_arguments(&mut self, arguments: Vec<String>) {
        self.arguments = arguments;
    }

    pub fn heap_stats(&self) -> HeapStats {
        self.heap.stats()
    }

    /// Runs the program. Every open file is flushed and closed before this
    /// returns, whether the program finished, halted or failed.
    pub fn run(&mut self, input: &mut impl BufRead, output: &mut impl Write) -> Result<(), Error> {
        let result = self.run_instructions(input, output, usize::MAX);
        let media = self.finish_media(output);
        let closed = self.files.close_all();
        result?;
        media.map_err(|message| Error::new(self.current_line(), message))?;
        closed.map_err(|message| Error::new(self.current_line(), message))
    }

    /// Executes instructions from the current position until the program
    /// halts, runs off its end or reaches `limit`.
    fn run_instructions(
        &mut self,
        input: &mut impl BufRead,
        output: &mut impl Write,
        limit: usize,
    ) -> Result<(), Error> {
        while self.pc < limit
            && let Some(instruction) = self.program.instructions.get(self.pc)
        {
            let line = instruction.line;
            let file = instruction.file;
            let op = instruction.op.clone();
            self.pc += 1;
            if let Err(message) = self.execute(op, input, output) {
                self.media.cancel_dispatch();
                let assertion = std::mem::take(&mut self.assertion_failure);
                let (line, file, function) = self
                    .error_origin
                    .take()
                    .unwrap_or_else(|| (line, file, self.function_name()));
                self.raise(Fault {
                    line,
                    file,
                    function,
                    message,
                    assertion,
                })
                .map_err(|fault| Error {
                    line: fault.line,
                    message: fault.message,
                    function: fault.function,
                    file: Some(self.program.file_name(fault.file))
                        .filter(|name| !name.is_empty())
                        .map(str::to_string),
                    assertion: fault.assertion,
                })?;
            }
        }
        output
            .flush()
            .map_err(|e| Error::new(self.current_line(), format!("output failed: {e}")))?;
        Ok(())
    }

    fn function_name(&self) -> Option<String> {
        self.frames
            .last()
            .map(|frame| self.program.functions[frame.function].qualified.clone())
    }

    /// Unwinds to the innermost handler that can take `fault`: frames, loop
    /// state and operand stack above it are discarded, then control moves to
    /// its CATCH or FINALLY block. Returns the fault when nothing handles it.
    fn raise(&mut self, fault: Fault) -> Result<(), Fault> {
        loop {
            let Some(handler) = self.handlers.last() else {
                return Err(fault);
            };
            let (frames, stack) = (handler.frames, handler.stack);
            while self.frames.len() > frames {
                if let Some(frame) = self.frames.pop() {
                    self.loops = frame.caller_loops;
                }
            }
            self.stack.truncate(stack);
            let Some(handler) = self.handlers.last_mut() else {
                return Err(fault);
            };
            let target = match (handler.phase, handler.catch, handler.finally) {
                (Phase::Body, Some(catch), _) => {
                    handler.phase = Phase::Catch;
                    handler.error = Some(fault);
                    catch
                }
                (Phase::Body | Phase::Catch, _, Some(finally)) => {
                    handler.phase = Phase::Finally;
                    handler.error = None;
                    handler.pending = Pending::Raise(fault);
                    finally
                }
                // No FINALLY to run, or the error came from the FINALLY block
                // itself: this handler is finished and the error moves outward.
                _ => {
                    self.handlers.pop();
                    continue;
                }
            };
            self.jump(target);
            return Ok(());
        }
    }

    /// Leaves `tries` TRY blocks on the way to `target`, running FINALLY blocks.
    fn leave(&mut self, target: usize, tries: usize) -> Result<(), String> {
        let mut tries = tries;
        while tries > 0 {
            let handler = self
                .handlers
                .last_mut()
                .ok_or("TRY block exit without an active handler")?;
            tries -= 1;
            match (handler.phase, handler.finally) {
                (Phase::Body | Phase::Catch, Some(finally)) => {
                    handler.phase = Phase::Finally;
                    handler.error = None;
                    handler.pending = Pending::Leave { target, tries };
                    self.jump(finally);
                    return Ok(());
                }
                // Leaving from inside FINALLY abandons whatever it had pending.
                _ => {
                    self.handlers.pop();
                }
            }
        }
        self.jump(target);
        Ok(())
    }

    /// The error being handled by the innermost active CATCH (or by a FINALLY
    /// that is running because of an error).
    fn current_error(&self) -> Option<&Fault> {
        self.handlers
            .iter()
            .rev()
            .find_map(|handler| match (handler.phase, &handler.pending) {
                (Phase::Catch, _) => handler.error.as_ref(),
                (Phase::Finally, Pending::Raise(fault)) => Some(fault),
                _ => None,
            })
    }

    fn current_line(&self) -> usize {
        self.program
            .instructions
            .get(self.pc.saturating_sub(1))
            .map_or(1, |i| i.line)
    }

    fn roots(&self) -> Vec<ObjectRef> {
        self.variables
            .values()
            .chain(self.stack.iter())
            .filter_map(|value| value.reference())
            .chain(self.loops.values().filter_map(LoopState::reference))
            .chain(self.frames.iter().flat_map(|frame| {
                frame
                    .locals
                    .values()
                    .filter_map(|value| value.as_ref().and_then(|value| value.reference()))
                    .chain(frame.caller_loops.values().filter_map(LoopState::reference))
                    .chain(frame.writebacks.iter().flat_map(|(_, place)| {
                        let pair = match place {
                            PlaceValue::Variable(_) => [None, None],
                            PlaceValue::Element(a, b) => [a.reference(), b.reference()],
                            PlaceValue::Field(a, _) => [a.reference(), None],
                        };
                        pair.into_iter().flatten()
                    }))
            }))
            .collect()
    }

    /// The roots to hand to a single heap allocation of `bytes` bytes: all of
    /// them when the allocation will collect garbage, none otherwise (the heap
    /// only reads them when it collects). Gathering roots walks every call
    /// frame, so skipping it keeps deep recursion that allocates linear.
    fn roots_for(&self, bytes: usize) -> Vec<ObjectRef> {
        if self.heap.collection_due(bytes) {
            self.roots()
        } else {
            Vec::new()
        }
    }

    /// [`Vm::roots_for`] for a payload of `values` values.
    fn roots_for_values(&self, values: usize) -> Vec<ObjectRef> {
        self.roots_for(values.saturating_mul(VALUE_BYTES))
    }

    fn allocate(&mut self, text: &str) -> Result<Value, String> {
        let roots = self.roots_for(text.len());
        self.heap.allocate(text, &roots).map(Value::String)
    }

    fn pop(&mut self) -> Result<Value, String> {
        if self.stack.len() <= self.stack_base() {
            return Err("bytecode stack underflow".into());
        }
        self.stack
            .pop()
            .ok_or_else(|| "bytecode stack underflow".into())
    }

    /// The declared type of `name` in the current scope, if any.
    fn declared_type(&self, name: &str) -> Option<VarType> {
        match self.frames.last() {
            Some(frame) if frame.locals.contains_key(name) => self.program.functions
                [frame.function]
                .types
                .get(name)
                .copied(),
            _ => self.var_types.get(name).copied(),
        }
    }

    fn is_local(&self, name: &str) -> bool {
        self.frames
            .last()
            .is_some_and(|frame| frame.locals.contains_key(name))
    }

    fn store(&mut self, name: String, value: Value) -> Result<(), String> {
        let declared = self.declared_type(&name);
        let value = match declared {
            // VARIANT variables hold anything, whatever their name ends in.
            Some(VarType::Variant) => value,
            Some(ty) => {
                check_type(&name, value)?;
                self.coerce(ty, &name, value)?
            }
            None => {
                check_type(&name, value)?;
                value
            }
        };
        if let Some(local) = self
            .frames
            .last_mut()
            .and_then(|frame| frame.locals.get_mut(&name))
        {
            *local = Some(value);
        } else {
            self.variables.insert(name, value);
        }
        Ok(())
    }

    /// Describes a value for type-mismatch messages.
    fn describe(&self, value: Value) -> String {
        match value {
            Value::Number(n) if n.fract() != 0.0 => format!("the number {n} (it has a fraction)"),
            Value::Number(n) => format!("the number {n} (outside the 64-bit integer range)"),
            Value::Int(n) => format!("the integer {n}"),
            Value::String(_) => "a string".into(),
            Value::Collection(reference) => match self.heap.collection_kind(reference) {
                Ok(CollectionKind::Array) => "an array".into(),
                Ok(CollectionKind::List) => "a list".into(),
                Ok(CollectionKind::Map) => "a map".into(),
                Err(_) => "a collection".into(),
            },
            Value::Record(reference) => match self.heap.record_type(reference) {
                Ok(id) => format!("a record of type {}", self.program.types[id].name),
                Err(_) => "a record".into(),
            },
            Value::Nothing => "NOTHING".into(),
        }
    }

    /// The name of a declared type as shown in messages.
    fn type_label(&self, ty: VarType) -> String {
        match ty {
            VarType::Record(id) => self.program.types[id].name.clone(),
            other => other.name().to_string(),
        }
    }

    /// Converts `value` for assignment to a variable declared as `ty`, or
    /// fails with a type-mismatch error.
    fn coerce(&self, ty: VarType, name: &str, value: Value) -> Result<Value, String> {
        let mismatch = || {
            format!(
                "type mismatch: {name} is declared AS {}; cannot assign {}",
                self.type_label(ty),
                self.describe(value)
            )
        };
        match (ty, value) {
            (VarType::Variant, _) => Ok(value),
            (VarType::Record(_), Value::Nothing) => Ok(value),
            (VarType::Record(_), Value::Int(n)) => Err(format!(
                "type mismatch: {name} is declared AS {}; cannot assign the number {n}",
                self.type_label(ty)
            )),
            (VarType::Record(_), Value::Number(n)) => Err(format!(
                "type mismatch: {name} is declared AS {}; cannot assign the number {n}",
                self.type_label(ty)
            )),
            (VarType::Record(id), Value::Record(reference)) => {
                if self.heap.record_type(reference)? == id {
                    Ok(value)
                } else {
                    Err(mismatch())
                }
            }
            (VarType::Integer, Value::Int(_)) => Ok(value),
            (VarType::Integer, Value::Number(n)) => {
                float_to_int(n).map(Value::Int).ok_or_else(mismatch)
            }
            (VarType::Double, Value::Number(_)) => Ok(value),
            (VarType::Double, Value::Int(n)) => Ok(Value::Number(n as f64)),
            (VarType::Boolean, Value::Number(n)) => Ok(boolean(n != 0.0)),
            (VarType::Boolean, Value::Int(n)) => Ok(boolean(n != 0)),
            (VarType::String, Value::String(_)) => Ok(value),
            (VarType::List | VarType::Map | VarType::Array, Value::Collection(reference)) => {
                let kind = self.heap.collection_kind(reference)?;
                let wanted = match ty {
                    VarType::List => CollectionKind::List,
                    VarType::Map => CollectionKind::Map,
                    _ => CollectionKind::Array,
                };
                if kind == wanted {
                    Ok(value)
                } else {
                    Err(mismatch())
                }
            }
            _ => Err(mismatch()),
        }
    }

    /// The default value of a type (allocating for strings and collections).
    /// A variable declared AS a record type starts as a fresh instance.
    fn default_value(&mut self, ty: VarType) -> Result<Value, String> {
        Ok(match ty {
            VarType::Integer => Value::Int(0),
            VarType::Double | VarType::Boolean | VarType::Variant => Value::Number(0.0),
            VarType::String => self.allocate("")?,
            VarType::Record(id) => self.new_record(id, &[])?,
            VarType::List | VarType::Map | VarType::Array => {
                let kind = match ty {
                    VarType::List => CollectionKind::List,
                    VarType::Map => CollectionKind::Map,
                    _ => CollectionKind::Array,
                };
                let reference = self.heap.new_collection(kind, &[], &self.roots_for(0))?;
                Value::Collection(reference)
            }
        })
    }

    /// Allocates an instance of record type `ty`. `initial` holds
    /// `(field index, value)` pairs already popped from the operand stack;
    /// every other field gets its default (record-typed fields: NOTHING).
    /// Values built along the way are kept rooted in `extra` until the
    /// instance itself exists.
    fn new_record(&mut self, ty: usize, initial: &[(usize, Value)]) -> Result<Value, String> {
        let definition = self.program.types.get(ty).ok_or("invalid record type")?;
        let name = definition.name.clone();
        let fields: Vec<_> = definition
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.ty, field.default.clone()))
            .collect();
        let mut extra: Vec<ObjectRef> = initial
            .iter()
            .filter_map(|(_, value)| value.reference())
            .collect();
        let mut values = Vec::with_capacity(fields.len());
        for (index, (field_name, field_type, default)) in fields.into_iter().enumerate() {
            let label = format!("{name}.{field_name}");
            let value = match initial.iter().find(|(field, _)| *field == index) {
                Some((_, value)) => self.coerce(field_type, &label, *value)?,
                None => self.field_default(field_type, default, &label, &extra)?,
            };
            extra.extend(value.reference());
            values.push(value);
        }
        let roots = if self.heap.collection_due(values.len() * VALUE_BYTES) {
            self.roots().into_iter().chain(extra).collect()
        } else {
            Vec::new()
        };
        let reference = self.heap.new_record(ty, &values, &roots)?;
        Ok(Value::Record(reference))
    }

    fn field_default(
        &mut self,
        ty: VarType,
        default: Option<FieldDefault>,
        label: &str,
        extra: &[ObjectRef],
    ) -> Result<Value, String> {
        let roots = |bytes: usize| -> Vec<ObjectRef> {
            if self.heap.collection_due(bytes) {
                self.roots()
                    .into_iter()
                    .chain(extra.iter().copied())
                    .collect()
            } else {
                Vec::new()
            }
        };
        let value = match default {
            Some(FieldDefault::Number(n)) => Value::Number(n),
            Some(FieldDefault::Int(n)) => Value::Int(n),
            Some(FieldDefault::Nothing) => Value::Nothing,
            Some(FieldDefault::Text(text)) => {
                let roots = roots(text.len());
                Value::String(self.heap.allocate(&text, &roots)?)
            }
            Some(FieldDefault::Constant(constant)) => {
                *self.variables.get(&constant).ok_or_else(|| {
                    format!(
                        "default {constant} of field {label} is not defined yet; \
                     declare the CONST before creating instances"
                    )
                })?
            }
            None => match ty {
                VarType::Integer => Value::Int(0),
                VarType::Double | VarType::Boolean | VarType::Variant => Value::Number(0.0),
                VarType::Record(_) => Value::Nothing,
                VarType::String => {
                    let roots = roots(0);
                    Value::String(self.heap.allocate("", &roots)?)
                }
                VarType::List | VarType::Map | VarType::Array => {
                    let kind = match ty {
                        VarType::List => CollectionKind::List,
                        VarType::Map => CollectionKind::Map,
                        _ => CollectionKind::Array,
                    };
                    let roots = roots(0);
                    Value::Collection(self.heap.new_collection(kind, &[], &roots)?)
                }
            },
        };
        self.coerce(ty, label, value)
    }

    /// Locates `name` in a record: the record and the field's index.
    fn field_slot(&self, target: Value, name: &str) -> Result<(ObjectRef, usize), String> {
        match target {
            Value::Record(reference) => {
                let definition = &self.program.types[self.heap.record_type(reference)?];
                let index = definition
                    .fields
                    .iter()
                    .position(|field| field.name == name)
                    .ok_or_else(|| {
                        format!("record type {} has no field {name}", definition.name)
                    })?;
                Ok((reference, index))
            }
            Value::Nothing => Err(format!("cannot access field {name} of NOTHING")),
            other => Err(format!(
                "cannot access field {name}: expected a record, found {}",
                self.describe(other)
            )),
        }
    }

    /// Reads a field of a record value.
    fn get_field(&self, target: Value, name: &str) -> Result<Value, String> {
        let (reference, index) = self.field_slot(target, name)?;
        self.heap.record_field(reference, index)
    }

    /// Assigns a field of a record value, enforcing the field's type.
    fn set_field(&mut self, target: Value, name: &str, value: Value) -> Result<(), String> {
        let (reference, index) = self.field_slot(target, name)?;
        let definition = &self.program.types[self.heap.record_type(reference)?];
        let field = &definition.fields[index];
        let label = format!("{}.{}", definition.name, field.name);
        let value = self.coerce(field.ty, &label, value)?;
        self.heap.set_record_field(reference, index, value)
    }

    /// Reads dimension sizes (non-negative whole numbers) and checks the
    /// total allocation size.
    fn dimension_sizes(&mut self, count: usize) -> Result<Vec<usize>, String> {
        let mut sizes = Vec::with_capacity(count);
        for value in self.pop_arguments(count)? {
            let size = match value {
                Value::Int(n) => usize::try_from(n).ok(),
                Value::Number(n) => float_to_int(n).and_then(|n| usize::try_from(n).ok()),
                _ => return Err("array dimensions must be numbers".into()),
            };
            sizes.push(size.ok_or("array dimensions must be non-negative whole numbers")?);
        }
        // Every dimension is bounded on its own, so a zero dimension cannot
        // hide an enormous one.
        if sizes.iter().any(|size| *size > MAX_ARRAY_ELEMENTS) {
            return Err("array is too large: a dimension exceeds 16777216".into());
        }
        let mut total = 1usize;
        for size in &sizes {
            total = total.saturating_mul(*size).min(MAX_ARRAY_ELEMENTS + 1);
        }
        if total > MAX_ARRAY_ELEMENTS {
            return Err("array is too large: more than 16777216 elements".into());
        }
        // Count every array object that will be allocated (all levels).
        let mut objects = 1usize;
        let mut prefix = 1usize;
        for size in &sizes[..sizes.len().saturating_sub(1)] {
            prefix = prefix.saturating_mul(*size);
            objects = objects.saturating_add(prefix);
            if objects > MAX_ARRAY_OBJECTS {
                return Err("array is too large: more than 1048576 array rows".into());
            }
            if prefix == 0 {
                break;
            }
        }
        Ok(sizes)
    }

    /// Builds the (unallocated) top-level elements of a nested array. Every
    /// row allocated on the way stays rooted through `extra`.
    fn build_array(
        &mut self,
        dims: &[usize],
        old: Option<&[Value]>,
        fill: Value,
        roots: &mut Vec<ObjectRef>,
    ) -> Result<Vec<Value>, String> {
        let mut elements = Vec::with_capacity(dims[0]);
        for index in 0..dims[0] {
            let previous = old.and_then(|old| old.get(index)).copied();
            if dims.len() == 1 {
                elements.push(previous.unwrap_or(fill));
                continue;
            }
            let old_row = match previous {
                Some(Value::Collection(reference))
                    if self.heap.collection_kind(reference)? == CollectionKind::Array =>
                {
                    Some(self.heap.collection(reference)?.1)
                }
                _ => None,
            };
            let row = self.build_array(&dims[1..], old_row.as_deref(), fill, roots)?;
            let reference = self
                .heap
                .new_collection(CollectionKind::Array, &row, roots)?;
            roots.push(reference);
            elements.push(Value::Collection(reference));
        }
        Ok(elements)
    }

    fn fill_value(&mut self, fill: VarType) -> Result<Value, String> {
        match fill {
            VarType::Integer => Ok(Value::Int(0)),
            VarType::String => self.allocate(""),
            VarType::Record(_) => Ok(Value::Nothing),
            _ => Ok(Value::Number(0.0)),
        }
    }

    fn dim_array(&mut self, dims: usize, fill: VarType) -> Result<(), String> {
        let sizes = self.dimension_sizes(dims)?;
        let fill = self.fill_value(fill)?;
        let mut roots = self.roots();
        roots.extend(fill.reference());
        let elements = self.build_array(&sizes, None, fill, &mut roots)?;
        let reference = self
            .heap
            .new_collection(CollectionKind::Array, &elements, &roots)?;
        self.stack.push(Value::Collection(reference));
        Ok(())
    }

    fn redim(&mut self, dims: usize, preserve: bool, fill: VarType) -> Result<(), String> {
        let sizes = self.dimension_sizes(dims)?;
        // The array stays on the operand stack (rooted) until the end.
        let Some(Value::Collection(reference)) = self.stack.last().copied() else {
            return Err("REDIM requires an array variable".into());
        };
        let (kind, old) = self.heap.collection(reference)?;
        if kind != CollectionKind::Array {
            return Err("REDIM requires an array".into());
        }
        let fill = self.fill_value(fill)?;
        let mut roots = self.roots();
        roots.extend(fill.reference());
        let elements =
            self.build_array(&sizes, preserve.then_some(old.as_slice()), fill, &mut roots)?;
        self.heap.replace_collection(reference, &elements, &roots)?;
        self.stack.pop();
        Ok(())
    }

    fn load(&self, name: &str) -> Result<Value, String> {
        if let Some(value) = self.frames.last().and_then(|frame| frame.locals.get(name)) {
            return value.ok_or_else(|| format!("uninitialized local variable {name}"));
        }
        self.variables.get(name).copied().ok_or_else(|| {
            if name == "SELF" {
                "undefined variable SELF (SELF is only available inside a CLASS method)".to_string()
            } else {
                format!("undefined variable {name}")
            }
        })
    }

    fn stack_base(&self) -> usize {
        self.frames.last().map_or(0, |frame| frame.stack_base)
    }

    fn pop_arguments(&mut self, count: usize) -> Result<Vec<Value>, String> {
        let start = self
            .stack
            .len()
            .checked_sub(count)
            .ok_or("bytecode stack underflow")?;
        if start < self.stack_base() {
            return Err("bytecode stack underflow".into());
        }
        Ok(self.stack.split_off(start))
    }

    fn call(
        &mut self,
        function: usize,
        count: usize,
        discard_result: bool,
        binding: Option<&CallBinding>,
    ) -> Result<(), String> {
        if self.frames.len() >= MAX_CALL_DEPTH {
            let name = self
                .program
                .functions
                .get(function)
                .map_or("?", |definition| definition.qualified.as_str());
            return Err(format!(
                "call depth limit exceeded ({MAX_CALL_DEPTH} frames) calling {name}; \
                 is a function calling itself without end?"
            ));
        }
        let extra_operands: usize =
            binding.map_or(0, |b| b.refs.iter().map(|r| r.place.operands()).sum());
        let start = self
            .stack
            .len()
            .checked_sub(count + extra_operands)
            .filter(|start| *start >= self.stack_base())
            .ok_or("bytecode stack underflow")?;
        let definition = self
            .program
            .functions
            .get(function)
            .ok_or("invalid function address")?;
        let total = definition.parameters.len();
        let required = definition.params.iter().filter(|p| !p.optional).count();
        if count < required || count > total {
            return Err(format!(
                "incorrect argument count for function {}",
                definition.qualified
            ));
        }
        // Split the stack region into argument values (by parameter) and the
        // operands of the BYREF places.
        let mut values: Vec<Option<Value>> = vec![None; total];
        let mut writebacks: Vec<(usize, PlaceValue)> = Vec::new();
        let mut cursor = start;
        for argument in 0..count {
            let slot = match binding {
                Some(b) if !b.slots.is_empty() => *b.slots.get(argument).ok_or("bad binding")?,
                _ => argument,
            };
            let reference = binding.and_then(|b| b.refs.iter().find(|r| r.arg == argument));
            let place = match reference.map(|r| &r.place) {
                Some(Place::Variable(name)) => Some(PlaceValue::Variable(name.clone())),
                Some(Place::Element) => {
                    let place = PlaceValue::Element(self.stack[cursor], self.stack[cursor + 1]);
                    cursor += 2;
                    Some(place)
                }
                Some(Place::Field(name)) => {
                    let place = PlaceValue::Field(self.stack[cursor], name.clone());
                    cursor += 1;
                    Some(place)
                }
                None => None,
            };
            values[slot] = Some(self.stack[cursor]);
            cursor += 1;
            if let Some(place) = place {
                writebacks.push((slot, place));
            }
        }
        let mut locals = HashMap::new();
        if definition.class.is_some() && matches!(values[0], Some(Value::Nothing)) {
            return Err(format!(
                "cannot call method {} on NOTHING",
                definition.qualified
            ));
        }
        locals.insert(definition.name.clone(), None);
        for name in &definition.locals {
            locals.insert(name.clone(), None);
        }
        let mut filled: Vec<ObjectRef> = Vec::new();
        for slot in 0..total {
            let definition = &self.program.functions[function];
            let name = definition.parameters[slot].clone();
            let declared = definition.types.get(&name).copied();
            let value = match values[slot] {
                Some(value) => value,
                None => {
                    let ty = declared.unwrap_or(if name.ends_with('$') {
                        VarType::String
                    } else {
                        VarType::Variant
                    });
                    let default = definition.params[slot].default.clone();
                    let label = format!("PARAMETER {name} of {}", definition.qualified);
                    let value = self.field_default(ty, default, &label, &filled)?;
                    if let Some(reference) = value.reference() {
                        filled.push(reference);
                    }
                    value
                }
            };
            let qualified = &self.program.functions[function].qualified;
            let value = match declared {
                Some(VarType::Variant) => value,
                Some(ty) => {
                    check_type(&name, value)?;
                    self.coerce(ty, &name, value).map_err(|message| {
                        format!("argument for PARAMETER {name} of {qualified}: {message}")
                    })?
                }
                None => {
                    check_type(&name, value)?;
                    value
                }
            };
            locals.insert(name, Some(value));
        }
        let entry = self.program.functions[function].entry;
        self.stack.truncate(start);
        self.frames.push(CallFrame {
            function,
            return_pc: self.pc,
            stack_base: self.stack.len(),
            discard_result,
            locals,
            caller_loops: std::mem::take(&mut self.loops),
            writebacks,
        });
        self.pc = entry;
        Ok(())
    }

    /// Calls method `name` on the receiver beneath the `count` arguments,
    /// choosing the method by the receiver's runtime class.
    fn call_method(
        &mut self,
        name: &str,
        count: usize,
        discard_result: bool,
    ) -> Result<(), String> {
        let index = self
            .stack
            .len()
            .checked_sub(count + 1)
            .filter(|index| *index >= self.stack_base())
            .ok_or("bytecode stack underflow")?;
        let id = match self.stack[index] {
            Value::Record(reference) => self.heap.record_type(reference)?,
            Value::Nothing => return Err(format!("cannot call method {name} on NOTHING")),
            other => {
                return Err(format!(
                    "cannot call method {name}: expected an object, found {}",
                    self.describe(other)
                ));
            }
        };
        let class = &self.program.types[id];
        let function = class
            .methods
            .iter()
            .find(|(method, _)| method == name)
            .map(|(_, function)| *function)
            .ok_or_else(|| {
                format!(
                    "{} {} has no method {name}",
                    if class.is_class {
                        "class"
                    } else {
                        "record type"
                    },
                    class.name
                )
            })?;
        let definition = &self.program.functions[function];
        if definition.params.iter().any(|param| param.by_ref) {
            return Err(format!(
                "method {} has BYREF parameters; it can only be called when the \
                 receiver's class is known at compile time (declare the variable AS {})",
                definition.qualified, class.name
            ));
        }
        self.call(function, count + 1, discard_result, None)
    }

    fn index_set(&mut self, container: Value, index: Value, value: Value) -> Result<(), String> {
        let reference = collection(container)?;
        let (kind, mut values) = self.heap.collection(reference)?;
        if let Some(position) = self.position(kind, &values, index)? {
            values[position] = value;
        } else {
            values.push(index);
            values.push(value);
        }
        self.heap
            .replace_collection(reference, &values, &self.roots_for_values(values.len()))
    }

    fn return_from_function(&mut self) -> Result<(), String> {
        let frame = self
            .frames
            .last()
            .ok_or("FEND without an active function call")?;
        let definition = &self.program.functions[frame.function];
        let name = &definition.name;
        let result = frame.locals.get(name).copied().flatten();
        if !frame.discard_result && result.is_none() {
            return Err(format!(
                "function {} did not assign its result",
                definition.qualified
            ));
        }
        if self.stack.len() != frame.stack_base {
            return Err("unbalanced function operand stack".into());
        }
        // Copy-out: the final values of the BYREF parameters.
        let mut outputs: Vec<(PlaceValue, Value)> = Vec::new();
        for (slot, place) in &frame.writebacks {
            let parameter = &definition.parameters[*slot];
            let value = frame
                .locals
                .get(parameter)
                .copied()
                .flatten()
                .ok_or_else(|| format!("BYREF parameter {parameter} has no value"))?;
            outputs.push((place.clone(), value));
        }
        let frame = self.frames.pop().ok_or("missing function call frame")?;
        self.pc = frame.return_pc;
        self.loops = frame.caller_loops;
        if !frame.discard_result {
            self.stack.push(result.ok_or("missing function result")?);
        }
        // Keep the values on the operand stack (a GC root) while storing.
        let base = self.stack.len();
        for (place, value) in &outputs {
            match place {
                PlaceValue::Variable(_) => {}
                PlaceValue::Element(container, index) => {
                    self.stack.push(*container);
                    self.stack.push(*index);
                }
                PlaceValue::Field(record, _) => self.stack.push(*record),
            }
            self.stack.push(*value);
        }
        for (place, value) in outputs {
            match place {
                PlaceValue::Variable(name) => self.store(name, value)?,
                PlaceValue::Element(container, index) => self.index_set(container, index, value)?,
                PlaceValue::Field(record, name) => self.set_field(record, &name, value)?,
            }
        }
        self.stack.truncate(base);
        Ok(())
    }

    fn execute(
        &mut self,
        op: Op,
        input: &mut impl BufRead,
        output: &mut impl Write,
    ) -> Result<(), String> {
        match op {
            Op::Number(n) => self.stack.push(Value::Number(n)),
            Op::Int(n) => self.stack.push(Value::Int(n)),
            Op::Declare { name, ty, has_init } => {
                let value = if has_init {
                    self.pop()?
                } else {
                    self.default_value(ty)?
                };
                if self.is_local(&name) {
                    self.store(name, value)?;
                } else {
                    let previous = self.var_types.insert(name.clone(), ty);
                    if let Err(message) = self.store(name.clone(), value) {
                        match previous {
                            Some(previous) => self.var_types.insert(name, previous),
                            None => self.var_types.remove(&name),
                        };
                        return Err(message);
                    }
                }
            }
            Op::DimArray { dims, fill } => self.dim_array(dims, fill)?,
            Op::Redim {
                dims,
                preserve,
                fill,
            } => self.redim(dims, preserve, fill)?,
            Op::String(text) => {
                let value = self.allocate(&text)?;
                self.stack.push(value);
            }
            Op::Load(name) => {
                let value = self.load(&name)?;
                self.stack.push(value);
            }
            Op::Store(name) => {
                let value = self.pop()?;
                self.store(name, value)?;
            }
            Op::Negate => {
                let value = match self.pop()? {
                    Value::Int(n) => {
                        Value::Int(n.checked_neg().ok_or("integer overflow in negation")?)
                    }
                    other => Value::Number(-number(other)? + 0.0),
                };
                self.stack.push(value);
            }
            Op::Not => {
                let value = number(self.pop()?)?;
                self.stack.push(boolean(value == 0.0));
            }
            Op::Truth => {
                let value = number(self.pop()?)?;
                self.stack.push(boolean(value != 0.0));
            }
            Op::AndJump(target) => {
                if number(self.pop()?)? == 0.0 {
                    self.stack.push(Value::Number(0.0));
                    self.pc = target;
                }
            }
            Op::OrJump(target) => {
                if number(self.pop()?)? != 0.0 {
                    self.stack.push(Value::Number(1.0));
                    self.pc = target;
                }
            }
            Op::Dup => {
                let value = self.pop()?;
                self.stack.push(value);
                self.stack.push(value);
            }
            Op::Dup2 => {
                let top = self.pop()?;
                let below = self.pop()?;
                self.stack.push(below);
                self.stack.push(top);
                self.stack.push(below);
                self.stack.push(top);
            }
            Op::Over => {
                let top = self.pop()?;
                let below = self.pop()?;
                self.stack.push(below);
                self.stack.push(top);
                self.stack.push(below);
            }
            Op::Pop => {
                self.pop()?;
            }
            Op::Binary(op) => {
                let right = self.pop()?;
                let left = self.pop()?;
                let value = self.binary(op, left, right)?;
                self.stack.push(value);
            }
            Op::Collection(kind, count) => {
                let mut values = self.pop_arguments(count)?;
                if kind == CollectionKind::Map {
                    values = self.map_pairs(&values)?;
                }
                let reference = self.heap.new_collection(
                    kind,
                    &values,
                    &self.roots_for_values(values.len()),
                )?;
                self.stack.push(Value::Collection(reference));
            }
            Op::IndexGet => {
                let index = self.pop()?;
                let reference = collection(self.pop()?)?;
                let (kind, values) = self.heap.collection(reference)?;
                let position = self
                    .position(kind, &values, index)?
                    .ok_or("missing map key")?;
                self.stack.push(values[position]);
            }
            Op::IndexSet => {
                let value = self.pop()?;
                let index = self.pop()?;
                let container = self.pop()?;
                self.index_set(container, index, value)?;
            }
            Op::Nothing => self.stack.push(Value::Nothing),
            Op::NewRecord { ty, fields } => {
                let values = self.pop_arguments(fields.len())?;
                let initial: Vec<(usize, Value)> = fields.into_iter().zip(values).collect();
                let value = self.new_record(ty, &initial)?;
                self.stack.push(value);
            }
            Op::GetField(name) => {
                let target = self.pop()?;
                let value = self.get_field(target, &name)?;
                self.stack.push(value);
            }
            Op::SetField(name) => {
                let value = self.pop()?;
                let target = self.pop()?;
                self.set_field(target, &name, value)?;
            }
            Op::Length => {
                let length = match self.pop()? {
                    Value::String(reference) => self.heap.get(reference)?.chars().count(),
                    Value::Collection(reference) => {
                        let (kind, values) = self.heap.collection(reference)?;
                        if kind == CollectionKind::Map {
                            values.len() / 2
                        } else {
                            values.len()
                        }
                    }
                    Value::Number(_) | Value::Int(_) | Value::Record(_) | Value::Nothing => {
                        return Err("LEN requires a string or collection".into());
                    }
                };
                self.stack.push(Value::Number(length as f64));
            }
            Op::Append => {
                let value = self.pop()?;
                let reference = collection(self.pop()?)?;
                let (kind, mut values) = self.heap.collection(reference)?;
                if kind != CollectionKind::List {
                    return Err("APPEND requires a list".into());
                }
                values.push(value);
                self.heap.replace_collection(
                    reference,
                    &values,
                    &self.roots_for_values(values.len()),
                )?;
            }
            Op::Remove => {
                let index = self.pop()?;
                let reference = collection(self.pop()?)?;
                let (kind, mut values) = self.heap.collection(reference)?;
                if kind == CollectionKind::Array {
                    return Err("cannot REMOVE from a fixed-length array".into());
                }
                let position = self
                    .position(kind, &values, index)?
                    .ok_or("missing map key")?;
                if kind == CollectionKind::Map {
                    values.drain(position - 1..=position);
                } else {
                    values.remove(position);
                }
                self.heap.replace_collection(
                    reference,
                    &values,
                    &self.roots_for_values(values.len()),
                )?;
            }
            Op::ForStart { variable, end } => {
                let step_value = self.pop()?;
                let limit_value = self.pop()?;
                let first_value = self.pop()?;
                let start = self.pc - 1;
                let int_step = match step_value {
                    Value::Int(n) => Some(n),
                    Value::Number(x) => float_to_int(x),
                    _ => None,
                };
                if let (Value::Int(first), Some(step)) = (first_value, int_step) {
                    number(limit_value)?;
                    if step == 0 {
                        return Err("FOR STEP must not be zero".into());
                    }
                    self.loops.remove(&start);
                    self.store(variable.clone(), Value::Int(first))?;
                    if int_in_range(first, limit_value, step) {
                        self.loops.insert(
                            start,
                            LoopState::Integer {
                                variable,
                                end,
                                limit: limit_value,
                                step,
                            },
                        );
                    } else {
                        self.jump(end);
                    }
                    return Ok(());
                }
                let step = number(step_value)?;
                let limit = number(limit_value)?;
                let first = number(first_value)?;
                if step == 0.0 {
                    return Err("FOR STEP must not be zero".into());
                }
                self.loops.remove(&start);
                self.store(variable.clone(), Value::Number(first))?;
                if in_range(first, limit, step) {
                    self.loops.insert(
                        start,
                        LoopState::Numeric {
                            variable,
                            end,
                            limit,
                            step,
                        },
                    );
                } else {
                    self.jump(end);
                }
            }
            Op::EachStart {
                variable,
                value_variable,
                end,
            } => {
                let reference = collection(self.pop()?)?;
                let (kind, values) = self.heap.collection(reference)?;
                let paired = value_variable.is_some();
                if paired && kind != CollectionKind::Map {
                    return Err("FOR EACH with two variables requires a map".into());
                }
                let values = if kind == CollectionKind::Map && !paired {
                    values.into_iter().skip(1).step_by(2).collect()
                } else {
                    values
                };
                let start = self.pc - 1;
                self.loops.remove(&start);
                if let Some(first) = values.first().copied() {
                    let snapshot = self.heap.new_collection(
                        CollectionKind::List,
                        &values,
                        &self.roots_for_values(values.len()),
                    )?;
                    self.store(variable.clone(), first)?;
                    if let Some(second) = &value_variable {
                        self.store(second.clone(), values[1])?;
                    }
                    self.loops.insert(
                        start,
                        LoopState::Each {
                            variable,
                            index: if paired { 2 } else { 1 },
                            second: value_variable,
                            end,
                            snapshot,
                        },
                    );
                } else {
                    self.jump(end);
                }
            }
            Op::ForNext { start } => self.next_iteration(start)?,
            Op::Call {
                function,
                arguments,
                discard_result,
                binding,
            } => self.call(function, arguments, discard_result, binding.as_deref())?,
            Op::Return => self.return_from_function()?,
            Op::CallMethod {
                name,
                arguments,
                discard_result,
            } => self.call_method(&name, arguments, discard_result)?,
            Op::Builtin {
                builtin,
                arguments,
                discard_result,
            } => {
                let result = if builtin.is_media() {
                    self.media_builtin(builtin, arguments, output)?
                } else {
                    self.builtin(builtin, arguments)?
                };
                if let (Some(value), false) = (result, discard_result) {
                    self.stack.push(value);
                }
            }
            Op::Jump(target) => self.jump(target),
            Op::JumpIfFalse(target) => {
                if number(self.pop()?)? == 0.0 {
                    self.jump(target);
                }
            }
            Op::JumpIfTrue(target) => {
                if number(self.pop()?)? != 0.0 {
                    self.jump(target);
                }
            }
            Op::Print => {
                let text = self.print_text()?;
                write!(output, "{text}").map_err(|e| format!("output failed: {e}"))?;
            }
            Op::PrintSeparator => {
                write!(output, "\t").map_err(|e| format!("output failed: {e}"))?
            }
            Op::PrintNewline => writeln!(output).map_err(|e| format!("output failed: {e}"))?,
            Op::FilePrint => {
                let text = self.print_text()?;
                let file = self.peek_file_number("PRINT #")?;
                self.files
                    .write_text(file, &text)
                    .map_err(|e| format!("PRINT #: {e}"))?;
            }
            Op::FilePrintSeparator | Op::FilePrintNewline => {
                let file = self.peek_file_number("PRINT #")?;
                let text = if op == Op::FilePrintSeparator {
                    "\t"
                } else {
                    "\n"
                };
                self.files
                    .write_text(file, text)
                    .map_err(|e| format!("PRINT #: {e}"))?;
            }
            Op::FileInput(name) => {
                let file = file_number(self.pop()?, "INPUT #")?;
                let text = self
                    .files
                    .read_line(file)
                    .map_err(|e| format!("INPUT #: {e}"))?;
                self.store_input(name, text)?;
            }
            Op::Input(name) => {
                output.flush().map_err(|e| format!("output failed: {e}"))?;
                let mut text = String::new();
                if input
                    .read_line(&mut text)
                    .map_err(|e| format!("input failed: {e}"))?
                    == 0
                {
                    return Err("unexpected end of input".into());
                }
                if text.ends_with('\n') {
                    text.pop();
                    if text.ends_with('\r') {
                        text.pop();
                    }
                }
                self.store_input(name, text)?;
            }
            Op::TryBegin { catch, finally } => self.handlers.push(Handler {
                catch,
                finally,
                frames: self.frames.len(),
                stack: self.stack.len(),
                phase: Phase::Body,
                error: None,
                pending: Pending::Resume,
            }),
            Op::Leave { target, tries } => self.leave(target, tries)?,
            Op::CatchStore(name) => {
                if let Some(name) = name {
                    let message = self
                        .handlers
                        .last()
                        .and_then(|handler| handler.error.as_ref())
                        .map(|fault| fault.message.clone())
                        .ok_or("CATCH reached without an error")?;
                    let value = self.allocate(&message)?;
                    self.store(name, value)?;
                }
            }
            Op::EndFinally => {
                let handler = self
                    .handlers
                    .pop()
                    .ok_or("END TRY reached without an active handler")?;
                match handler.pending {
                    Pending::Resume => {}
                    Pending::Leave { target, tries } => self.leave(target, tries)?,
                    Pending::Raise(fault) => {
                        self.error_origin = Some((fault.line, fault.file, fault.function));
                        self.assertion_failure = fault.assertion;
                        return Err(fault.message);
                    }
                }
            }
            Op::Throw => {
                let Value::String(reference) = self.pop()? else {
                    return Err("THROW requires a string message".into());
                };
                return Err(self.heap.get(reference)?.to_string());
            }
            Op::Rethrow(depth) => {
                let fault = self
                    .handlers
                    .len()
                    .checked_sub(depth + 1)
                    .and_then(|index| self.handlers[index].error.clone())
                    .ok_or("THROW without a message needs an active CATCH block")?;
                self.error_origin = Some((fault.line, fault.file, fault.function));
                self.assertion_failure = fault.assertion;
                return Err(fault.message);
            }
            Op::Collect => self.heap.collect(&self.roots())?,
            Op::Assert { has_message } => self.assert_op(has_message)?,
            Op::AssertEq { has_message } => self.assert_eq_op(has_message)?,
            Op::AssertNear { has_message } => self.assert_near_op(has_message)?,
            Op::OnEvent { kind, function } => self.register_handler(kind, function),
            Op::Halt => self.halt(),
            Op::Terminate { stop } => {
                if self.test.running && self.test.halted.is_none() {
                    let (line, file) = self
                        .program
                        .instructions
                        .get(self.pc.saturating_sub(1))
                        .map_or((0, 0), |instruction| (instruction.line, instruction.file));
                    self.test.halted = Some(testing::Halted {
                        keyword: if stop { "STOP" } else { "END" },
                        line,
                        file,
                        function: self.function_name(),
                    });
                }
                self.halt();
            }
        }
        Ok(())
    }

    /// Stops the program: drops every call, loop, handler and operand.
    fn halt(&mut self) {
        self.media.cancel_dispatch();
        self.loops.clear();
        self.frames.clear();
        self.stack.clear();
        self.handlers.clear();
        self.pc = self.program.instructions.len();
    }

    /// Pops the value to print and renders it.
    fn print_text(&mut self) -> Result<String, String> {
        match self.pop()? {
            Value::Number(n) => Ok(n.to_string()),
            Value::Int(n) => Ok(n.to_string()),
            Value::String(reference) => Ok(self.heap.get(reference)?.to_string()),
            Value::Collection(_) => Err("cannot PRINT a collection; print its elements".into()),
            Value::Record(_) => Err("cannot PRINT a record; print its fields".into()),
            Value::Nothing => Err("cannot PRINT NOTHING".into()),
        }
    }

    /// The file number kept on the stack beneath the values of `PRINT #`.
    fn peek_file_number(&self, who: &str) -> Result<usize, String> {
        if self.stack.len() <= self.stack_base() {
            return Err("bytecode stack underflow".into());
        }
        file_number(*self.stack.last().ok_or("bytecode stack underflow")?, who)
    }

    fn store_input(&mut self, name: String, text: String) -> Result<(), String> {
        let value = if name.ends_with('$') {
            self.allocate(&text)?
        } else if self.declared_type(&name) == Some(VarType::Integer) {
            let n = text
                .trim()
                .parse::<i64>()
                .map_err(|_| "input must be a whole number within the 64-bit range")?;
            Value::Int(n)
        } else {
            let n = text
                .trim()
                .parse::<f64>()
                .map_err(|_| "input must be a number")?;
            if !n.is_finite() {
                return Err("input must be finite".into());
            }
            Value::Number(n)
        };
        self.store(name, value)
    }

    fn jump(&mut self, target: usize) {
        self.loops
            .retain(|start, state| target > *start && target < state.end());
        self.pc = target;
    }

    fn next_iteration(&mut self, start: usize) -> Result<(), String> {
        let state = self
            .loops
            .get(&start)
            .cloned()
            .ok_or("NEXT reached without an active FOR")?;
        match state {
            LoopState::Numeric {
                variable,
                limit,
                step,
                ..
            } => {
                let current = number(self.load(&variable)?)?;
                let next = current + step;
                if !next.is_finite() {
                    return Err("FOR increment is not finite".into());
                }
                if next == current {
                    return Err("FOR STEP is too small to advance the loop variable".into());
                }
                self.store(variable, Value::Number(next))?;
                if in_range(next, limit, step) {
                    self.jump(start + 1);
                } else {
                    self.loops.remove(&start);
                }
            }
            LoopState::Integer {
                variable,
                limit,
                step,
                ..
            } => {
                let current = match self.load(&variable)? {
                    Value::Int(n) => Some(n),
                    Value::Number(n) => float_to_int(n),
                    _ => None,
                }
                .ok_or("FOR variable no longer holds a whole number")?;
                match current.checked_add(step) {
                    Some(next) if int_in_range(next, limit, step) => {
                        self.store(variable, Value::Int(next))?;
                        self.jump(start + 1);
                    }
                    // Past the limit (or past the integer range): the loop ends.
                    Some(next) => {
                        self.store(variable, Value::Int(next))?;
                        self.loops.remove(&start);
                    }
                    None => {
                        self.loops.remove(&start);
                    }
                }
            }
            LoopState::Each {
                variable,
                second,
                end,
                snapshot,
                index,
            } => {
                let (_, values) = self.heap.collection(snapshot)?;
                if let Some(value) = values.get(index).copied() {
                    self.store(variable.clone(), value)?;
                    let width = if let Some(second) = &second {
                        let paired = values
                            .get(index + 1)
                            .copied()
                            .ok_or("invalid FOR EACH snapshot")?;
                        self.store(second.clone(), paired)?;
                        2
                    } else {
                        1
                    };
                    self.loops.insert(
                        start,
                        LoopState::Each {
                            variable,
                            second,
                            end,
                            snapshot,
                            index: index + width,
                        },
                    );
                    self.jump(start + 1);
                } else {
                    self.loops.remove(&start);
                }
            }
        }
        Ok(())
    }

    fn map_pairs(&self, values: &[Value]) -> Result<Vec<Value>, String> {
        if !values.len().is_multiple_of(2) {
            return Err("MAP requires key/value pairs".into());
        }
        let mut pairs = Vec::new();
        for pair in values.as_chunks::<2>().0 {
            if let Some(position) = self.position(CollectionKind::Map, &pairs, pair[0])? {
                pairs[position] = pair[1];
            } else {
                pairs.extend_from_slice(pair);
            }
        }
        Ok(pairs)
    }

    fn position(
        &self,
        kind: CollectionKind,
        values: &[Value],
        index: Value,
    ) -> Result<Option<usize>, String> {
        if kind == CollectionKind::Map {
            let Value::String(reference) = index else {
                return Err("map keys must be strings".into());
            };
            let key = self.heap.get(reference)?;
            let (pairs, remainder) = values.as_chunks::<2>();
            if !remainder.is_empty() {
                return Err("invalid map payload length".into());
            }
            for (position, pair) in pairs.iter().enumerate() {
                let Value::String(reference) = pair[0] else {
                    return Err("invalid map key payload".into());
                };
                if self.heap.get(reference)? == key {
                    return Ok(Some(position * 2 + 1));
                }
            }
            Ok(None)
        } else {
            let index = number(index)?;
            if index < 1.0 || index.fract() != 0.0 || index > values.len() as f64 {
                return Err(
                    "index out of bounds: expected a one-based integer within the collection"
                        .into(),
                );
            }
            Ok(Some(index as usize - 1))
        }
    }

    fn binary(&mut self, op: BinaryOp, left: Value, right: Value) -> Result<Value, String> {
        if matches!(left, Value::Record(_) | Value::Nothing)
            || matches!(right, Value::Record(_) | Value::Nothing)
        {
            // Records compare by identity; anything else is simply different.
            let same = match (left, right) {
                (Value::Record(a), Value::Record(b)) => a == b,
                (Value::Nothing, Value::Nothing) => true,
                _ => false,
            };
            return match op {
                BinaryOp::Equal => Ok(boolean(same)),
                BinaryOp::NotEqual => Ok(boolean(!same)),
                _ => Err("operator is not supported for records or NOTHING".into()),
            };
        }
        if let (Value::String(a), Value::String(b)) = (left, right) {
            let a = self.heap.get(a)?;
            let b = self.heap.get(b)?;
            return match op {
                BinaryOp::Add => {
                    let text = [a, b].concat();
                    self.allocate(&text)
                }
                BinaryOp::Equal => Ok(boolean(a == b)),
                BinaryOp::NotEqual => Ok(boolean(a != b)),
                BinaryOp::Less => Ok(boolean(a < b)),
                BinaryOp::LessEqual => Ok(boolean(a <= b)),
                BinaryOp::Greater => Ok(boolean(a > b)),
                BinaryOp::GreaterEqual => Ok(boolean(a >= b)),
                _ => Err("operator requires numeric operands".into()),
            };
        }
        if let (Value::Int(a), Value::Int(b)) = (left, right) {
            return int_binary(op, a, b);
        }
        let a = number(left)?;
        let b = number(right)?;
        if matches!(
            op,
            BinaryOp::Equal
                | BinaryOp::NotEqual
                | BinaryOp::Less
                | BinaryOp::LessEqual
                | BinaryOp::Greater
                | BinaryOp::GreaterEqual
        ) {
            // Exact even when one side is an integer beyond 2^53.
            let ordering = cmp_numeric(left, right).ok_or("operands are not comparable")?;
            return Ok(compare_result(op, ordering));
        }
        if op == BinaryOp::IntDivide {
            return int_divide(exact_int(left)?, exact_int(right)?);
        }
        let result = match op {
            BinaryOp::Add => a + b,
            BinaryOp::Subtract => a - b,
            BinaryOp::Multiply => a * b,
            BinaryOp::Divide => {
                if b == 0.0 {
                    return Err("division by zero".into());
                }
                a / b
            }
            BinaryOp::Modulo => {
                if b == 0.0 {
                    return Err("MOD divisor must not be zero".into());
                }
                a % b
            }
            BinaryOp::Power => {
                let result = a.powf(b);
                if !result.is_finite() {
                    return Err("^ result is not a finite number".into());
                }
                result + 0.0
            }
            BinaryOp::And => return Ok(boolean(a != 0.0 && b != 0.0)),
            BinaryOp::Or => return Ok(boolean(a != 0.0 || b != 0.0)),
            _ => unreachable!("comparisons and integer division are handled above"),
        };
        if !result.is_finite() {
            return Err("arithmetic result is not finite".into());
        }
        // Adding 0.0 turns negative zero into positive zero.
        Ok(Value::Number(result + 0.0))
    }
}

fn compare_result(op: BinaryOp, ordering: Ordering) -> Value {
    boolean(match op {
        BinaryOp::Equal => ordering == Ordering::Equal,
        BinaryOp::NotEqual => ordering != Ordering::Equal,
        BinaryOp::Less => ordering == Ordering::Less,
        BinaryOp::LessEqual => ordering != Ordering::Greater,
        BinaryOp::Greater => ordering == Ordering::Greater,
        _ => ordering != Ordering::Less,
    })
}

/// A whole-number operand of `\`: an integer, or a whole double in range.
fn exact_int(value: Value) -> Result<i64, String> {
    match value {
        Value::Int(n) => Ok(n),
        Value::Number(n) => float_to_int(n).ok_or_else(|| {
            "\\ requires whole-number operands within the 64-bit integer range".to_string()
        }),
        other => number(other).map(|_| unreachable!("non-numbers fail above")),
    }
}

fn int_divide(a: i64, b: i64) -> Result<Value, String> {
    if b == 0 {
        return Err("integer division by zero".into());
    }
    a.checked_div(b)
        .map(Value::Int)
        .ok_or_else(|| "integer overflow in \\".into())
}

/// Arithmetic on two exact integers: `+ - * \ MOD` are checked, `/` and `^`
/// produce doubles.
fn int_binary(op: BinaryOp, a: i64, b: i64) -> Result<Value, String> {
    let overflow = |symbol: &str| format!("integer overflow in {symbol}");
    match op {
        BinaryOp::Add => a
            .checked_add(b)
            .map(Value::Int)
            .ok_or_else(|| overflow("+")),
        BinaryOp::Subtract => a
            .checked_sub(b)
            .map(Value::Int)
            .ok_or_else(|| overflow("-")),
        BinaryOp::Multiply => a
            .checked_mul(b)
            .map(Value::Int)
            .ok_or_else(|| overflow("*")),
        BinaryOp::IntDivide => int_divide(a, b),
        BinaryOp::Modulo => {
            if b == 0 {
                return Err("MOD divisor must not be zero".into());
            }
            // MIN MOD -1 is mathematically 0; wrapping avoids the overflow trap.
            Ok(Value::Int(a.wrapping_rem(b)))
        }
        BinaryOp::Divide => {
            if b == 0 {
                return Err("division by zero".into());
            }
            Ok(Value::Number(a as f64 / b as f64 + 0.0))
        }
        BinaryOp::Power => {
            let result = (a as f64).powf(b as f64);
            if !result.is_finite() {
                return Err("^ result is not a finite number".into());
            }
            Ok(Value::Number(result + 0.0))
        }
        BinaryOp::And => Ok(boolean(a != 0 && b != 0)),
        BinaryOp::Or => Ok(boolean(a != 0 || b != 0)),
        _ => Ok(compare_result(op, a.cmp(&b))),
    }
}

/// A file number: a whole number from 1 to 255.
fn file_number(value: Value, who: &str) -> Result<usize, String> {
    let n = number(value).map_err(|message| format!("{who}: {message}"))?;
    if n.fract() != 0.0 || n < 1.0 || n > MAX_FILES as f64 {
        return Err(format!(
            "{who}: file number must be a whole number from 1 to {MAX_FILES}"
        ));
    }
    Ok(n as usize)
}

fn check_type(name: &str, value: Value) -> Result<(), String> {
    if name.ends_with('$') != matches!(value, Value::String(_)) {
        return Err(format!(
            "type mismatch: {name} requires {}",
            if name.ends_with('$') {
                "a string"
            } else {
                "a number or collection"
            }
        ));
    }
    Ok(())
}

fn number(value: Value) -> Result<f64, String> {
    match value {
        Value::Number(n) => Ok(n),
        Value::Int(n) => Ok(n as f64),
        Value::String(_) => Err("expected a number, found a string".into()),
        Value::Collection(_) => Err("expected a number, found a collection".into()),
        Value::Record(_) => Err("expected a number, found a record".into()),
        Value::Nothing => Err("expected a number, found NOTHING".into()),
    }
}

fn int_in_range(value: i64, limit: Value, step: i64) -> bool {
    let ordering = cmp_numeric(Value::Int(value), limit);
    match ordering {
        Some(order) if step > 0 => order != Ordering::Greater,
        Some(order) => order != Ordering::Less,
        None => false,
    }
}

fn collection(value: Value) -> Result<ObjectRef, String> {
    match value {
        Value::Collection(reference) => Ok(reference),
        _ => Err("expected a collection".into()),
    }
}

fn in_range(value: f64, limit: f64, step: f64) -> bool {
    if step > 0.0 {
        value <= limit
    } else {
        value >= limit
    }
}

fn boolean(value: bool) -> Value {
    Value::Number(if value { 1.0 } else { 0.0 })
}
