//! Execution of the standard-library builtins. Argument values are popped
//! before any heap allocation, so every managed value that must survive an
//! allocation (and thus a possible collection) is either copied out first or
//! reachable through a collection payload / the operand stack.

use std::cmp::Ordering;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{Vm, boolean, file_number, number};
use crate::files::{MAX_READ_BYTES, Mode, describe};
use crate::heap::ObjectRef;
use crate::stdlib;
use crate::value::{Value, cmp_numeric, float_to_int};
use crate::{Builtin, CollectionKind};

type Result<T> = std::result::Result<T, String>;

fn num(value: Value, who: &str) -> Result<f64> {
    number(value).map_err(|message| format!("{who}: {message}"))
}

/// A non-negative whole number (counts and lengths).
fn count(value: Value, who: &str, what: &str) -> Result<usize> {
    let n = num(value, who)?;
    if n < 0.0 || n.fract() != 0.0 {
        return Err(format!("{who}: {what} must be a non-negative whole number"));
    }
    Ok(n as usize)
}

/// A one-based character position (at least 1).
fn start_position(value: Value, who: &str) -> Result<usize> {
    let n = num(value, who)?;
    if n < 1.0 || n.fract() != 0.0 {
        return Err(format!("{who}: start must be a whole number of at least 1"));
    }
    Ok(n as usize)
}

fn finite(who: &str, value: f64) -> Result<Value> {
    if value.is_finite() {
        // `+ 0.0` turns negative zero into zero.
        Ok(Value::Number(value + 0.0))
    } else {
        Err(format!("{who}: result is not a finite number"))
    }
}

fn now_seconds() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .map_err(|_| "system clock is before 1970".to_string())
}

fn round(x: f64, digits: i64) -> f64 {
    let scale = 10f64.powi(digits.unsigned_abs() as i32);
    if digits >= 0 {
        let scaled = x * scale;
        // Overflow means x is already far beyond the requested precision.
        if scaled.is_finite() {
            scaled.round() / scale
        } else {
            x
        }
    } else {
        (x / scale).round() * scale
    }
}

impl Vm {
    fn text(&self, value: Value, who: &str) -> Result<String> {
        match value {
            Value::String(reference) => Ok(self.heap.get(reference)?.to_string()),
            _ => Err(format!("{who}: expected a string argument")),
        }
    }

    fn collection_argument(
        &self,
        value: Value,
        who: &str,
    ) -> Result<(ObjectRef, CollectionKind, Vec<Value>)> {
        let Value::Collection(reference) = value else {
            return Err(format!("{who}: expected a collection argument"));
        };
        let (kind, values) = self.heap.collection(reference)?;
        Ok((reference, kind, values))
    }

    fn sequence_argument(
        &self,
        value: Value,
        who: &str,
    ) -> Result<(ObjectRef, CollectionKind, Vec<Value>)> {
        let (reference, kind, values) = self.collection_argument(value, who)?;
        if kind == CollectionKind::Map {
            return Err(format!("{who}: expected an array or list, found a map"));
        }
        Ok((reference, kind, values))
    }

    fn map_argument(&self, value: Value, who: &str) -> Result<Vec<Value>> {
        let (_, kind, values) = self.collection_argument(value, who)?;
        if kind != CollectionKind::Map {
            return Err(format!("{who}: expected a map"));
        }
        Ok(values)
    }

    fn new_list(&mut self, items: &[Value]) -> Result<Value> {
        let roots = self.roots();
        self.heap
            .new_collection(CollectionKind::List, items, &roots)
            .map(Value::Collection)
    }

    fn store_items(&mut self, reference: ObjectRef, items: &[Value]) -> Result<()> {
        let roots = self.roots();
        self.heap.replace_collection(reference, items, &roots)
    }

    fn same_value(&self, left: Value, right: Value) -> Result<bool> {
        Ok(match (left, right) {
            (Value::String(a), Value::String(b)) => self.heap.get(a)? == self.heap.get(b)?,
            (Value::Collection(a), Value::Collection(b)) => a == b,
            (Value::Record(a), Value::Record(b)) => a == b,
            (Value::Nothing, Value::Nothing) => true,
            (Value::String(_) | Value::Collection(_) | Value::Record(_) | Value::Nothing, _)
            | (_, Value::String(_) | Value::Collection(_) | Value::Record(_) | Value::Nothing) => {
                false
            }
            _ => cmp_numeric(left, right) == Some(Ordering::Equal),
        })
    }

    pub(super) fn builtin(
        &mut self,
        builtin: Builtin,
        argument_count: usize,
    ) -> Result<Option<Value>> {
        let args = self.pop_arguments(argument_count)?;
        let who = builtin.name();
        let value = match builtin {
            Builtin::Abs | Builtin::Int | Builtin::Fix if matches!(args[0], Value::Int(_)) => {
                let Value::Int(n) = args[0] else {
                    unreachable!("guard checked the variant")
                };
                if builtin == Builtin::Abs {
                    Value::Int(n.checked_abs().ok_or("ABS: integer overflow")?)
                } else {
                    Value::Int(n)
                }
            }
            Builtin::Abs
            | Builtin::Sqrt
            | Builtin::Int
            | Builtin::Fix
            | Builtin::Sin
            | Builtin::Cos
            | Builtin::Tan
            | Builtin::Atn
            | Builtin::Log
            | Builtin::Exp => {
                let x = num(args[0], who)?;
                let result = match builtin {
                    Builtin::Abs => x.abs(),
                    Builtin::Sqrt if x < 0.0 => {
                        return Err("SQRT: argument must not be negative".into());
                    }
                    Builtin::Sqrt => x.sqrt(),
                    Builtin::Int => x.floor(),
                    Builtin::Fix => x.trunc(),
                    Builtin::Sin => x.sin(),
                    Builtin::Cos => x.cos(),
                    Builtin::Tan => x.tan(),
                    Builtin::Atn => x.atan(),
                    Builtin::Log if x <= 0.0 => {
                        return Err("LOG: argument must be greater than zero".into());
                    }
                    Builtin::Log => x.ln(),
                    _ => x.exp(),
                };
                finite(who, result)?
            }
            Builtin::Atn2 => {
                // `+ 0.0` turns -0 into 0, so ATN2(0, 0) and ATN2(0, -0) are
                // both 0 and the result is always in (-pi, pi].
                let y = num(args[0], who)? + 0.0;
                let x = num(args[1], who)? + 0.0;
                finite(who, y.atan2(x))?
            }
            Builtin::Pi => Value::Number(std::f64::consts::PI),
            Builtin::Min | Builtin::Max => {
                num(args[0], who)?;
                let mut best = args[0];
                for argument in &args[1..] {
                    num(*argument, who)?;
                    let order =
                        cmp_numeric(*argument, best).ok_or("operands are not comparable")?;
                    let better = if builtin == Builtin::Min {
                        order == Ordering::Less
                    } else {
                        order == Ordering::Greater
                    };
                    if better {
                        best = *argument;
                    }
                }
                match best {
                    Value::Number(x) => finite(who, x)?,
                    other => other,
                }
            }
            Builtin::Round if matches!(args[0], Value::Int(_)) => {
                let digits = match args.get(1) {
                    Some(value) => num(*value, who)?,
                    None => 0.0,
                };
                if digits >= 0.0 {
                    args[0]
                } else {
                    let x = num(args[0], who)?;
                    finite(who, round(x, digits as i64))?
                }
            }
            Builtin::Round => {
                let x = num(args[0], who)?;
                let digits = match args.get(1) {
                    Some(value) => {
                        let digits = num(*value, who)?;
                        if digits.fract() != 0.0 || !(-308.0..=308.0).contains(&digits) {
                            return Err(
                                "ROUND: digits must be a whole number from -308 to 308".into()
                            );
                        }
                        digits as i64
                    }
                    None => 0,
                };
                finite(who, round(x, digits))?
            }
            Builtin::Rnd => Value::Number(self.rng.next_unit()),
            Builtin::Randomize => {
                let seed = match args.first() {
                    Some(value) => (num(*value, who)? + 0.0).to_bits(),
                    None => SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(|_| "system clock is before 1970")?
                        .as_nanos() as u64,
                };
                self.rng.reseed(seed);
                return Ok(None);
            }
            Builtin::Left | Builtin::Right => {
                let text = self.text(args[0], who)?;
                let length = count(args[1], who, "length")?;
                let piece = if builtin == Builtin::Left {
                    stdlib::left(&text, length)
                } else {
                    stdlib::right(&text, length)
                };
                self.allocate(piece)?
            }
            Builtin::Mid => {
                let text = self.text(args[0], who)?;
                let start = start_position(args[1], who)?;
                let length = match args.get(2) {
                    Some(value) => Some(count(*value, who, "length")?),
                    None => None,
                };
                self.allocate(stdlib::mid(&text, start, length))?
            }
            Builtin::Instr => {
                let text = self.text(args[0], who)?;
                let needle = self.text(args[1], who)?;
                let start = match args.get(2) {
                    Some(value) => start_position(*value, who)?,
                    None => 1,
                };
                Value::Number(stdlib::instr(&text, &needle, start) as f64)
            }
            Builtin::Replace => {
                let text = self.text(args[0], who)?;
                let find = self.text(args[1], who)?;
                let replacement = self.text(args[2], who)?;
                if find.is_empty() {
                    return Err("REPLACE$: the search text must not be empty".into());
                }
                self.allocate(&text.replace(&find, &replacement))?
            }
            Builtin::Trim | Builtin::Ucase | Builtin::Lcase => {
                let text = self.text(args[0], who)?;
                let result = match builtin {
                    Builtin::Trim => text.trim().to_string(),
                    Builtin::Ucase => text.to_uppercase(),
                    _ => text.to_lowercase(),
                };
                self.allocate(&result)?
            }
            Builtin::Str => match args[0] {
                Value::Int(n) => self.allocate(&n.to_string())?,
                other => {
                    let x = num(other, who)?;
                    self.allocate(&x.to_string())?
                }
            },
            Builtin::Val => {
                let text = self.text(args[0], who)?;
                let trimmed = text.trim();
                // Large plain integers stay exact; everything else is a double.
                if let Ok(n) = trimmed.parse::<i64>()
                    && n.unsigned_abs() > (1u64 << 53)
                {
                    Value::Int(n)
                } else {
                    match trimmed.parse::<f64>() {
                        Ok(x) if x.is_finite() => Value::Number(x + 0.0),
                        _ => return Err(format!("VAL: {text:?} is not a finite number")),
                    }
                }
            }
            Builtin::Split => {
                let text = self.text(args[0], who)?;
                let separator = self.text(args[1], who)?;
                if separator.is_empty() {
                    return Err("SPLIT: the separator must not be empty".into());
                }
                // Each new piece is only reachable from `roots` until the
                // list payload is allocated, so root it as we go.
                let base = self.roots();
                let mut roots = base.clone();
                let mut pieces = Vec::new();
                for piece in text.split(separator.as_str()) {
                    let reference = self.heap.allocate(piece, &roots)?;
                    roots.push(reference);
                    pieces.push(Value::String(reference));
                }
                self.heap
                    .new_collection(CollectionKind::List, &pieces, &base)
                    .map(Value::Collection)?
            }
            Builtin::Join => {
                let (_, _, items) = self.sequence_argument(args[0], who)?;
                let separator = self.text(args[1], who)?;
                let mut parts = Vec::with_capacity(items.len());
                for item in items {
                    let Value::String(reference) = item else {
                        return Err("JOIN$: every element must be a string".into());
                    };
                    parts.push(self.heap.get(reference)?.to_string());
                }
                self.allocate(&parts.join(&separator))?
            }
            Builtin::Insert => {
                let (reference, kind, mut items) = self.collection_argument(args[0], who)?;
                if kind != CollectionKind::List {
                    return Err("INSERT: expected a list".into());
                }
                let index = count(args[1], who, "index")?;
                if index < 1 || index > items.len() + 1 {
                    return Err(
                        "INSERT: index out of bounds: expected 1 through LEN(list) + 1".into(),
                    );
                }
                items.insert(index - 1, args[2]);
                self.store_items(reference, &items)?;
                return Ok(None);
            }
            Builtin::Pop => {
                let (reference, kind, mut items) = self.collection_argument(args[0], who)?;
                if kind != CollectionKind::List {
                    return Err("POP: expected a list".into());
                }
                let value = items.pop().ok_or("POP: the list is empty")?;
                // The removed element is no longer in the payload; keep it
                // rooted on the operand stack while the list is rewritten.
                self.stack.push(value);
                let stored = self.store_items(reference, &items);
                self.stack.pop();
                stored?;
                value
            }
            Builtin::Sort => {
                let (reference, _, items) = self.sequence_argument(args[0], who)?;
                let sorted = self.sorted(items)?;
                self.store_items(reference, &sorted)?;
                return Ok(None);
            }
            Builtin::Reverse => {
                let (reference, _, mut items) = self.sequence_argument(args[0], who)?;
                items.reverse();
                self.store_items(reference, &items)?;
                return Ok(None);
            }
            Builtin::Contains => {
                let (_, kind, items) = self.collection_argument(args[0], who)?;
                let step = if kind == CollectionKind::Map { 2 } else { 1 };
                let mut found = false;
                for item in items.into_iter().skip(step - 1).step_by(step) {
                    if self.same_value(item, args[1])? {
                        found = true;
                        break;
                    }
                }
                boolean(found)
            }
            Builtin::Keys => {
                let pairs = self.map_argument(args[0], who)?;
                let keys: Vec<Value> = pairs.iter().step_by(2).copied().collect();
                self.new_list(&keys)?
            }
            Builtin::Values => {
                let pairs = self.map_argument(args[0], who)?;
                let values: Vec<Value> = pairs.iter().skip(1).step_by(2).copied().collect();
                self.new_list(&values)?
            }
            Builtin::HasKey => {
                let pairs = self.map_argument(args[0], who)?;
                let position = self.position(CollectionKind::Map, &pairs, args[1])?;
                boolean(position.is_some())
            }
            Builtin::Timer => Value::Number(self.started.elapsed().as_secs_f64()),
            Builtin::UnixTime => Value::Number(now_seconds()? as f64),
            Builtin::Now | Builtin::Date | Builtin::Time => {
                let (date, time) = stdlib::format_utc(now_seconds()? as i64);
                let text = match builtin {
                    Builtin::Date => date,
                    Builtin::Time => time,
                    _ => format!("{date} {time}"),
                };
                self.allocate(&text)?
            }
            Builtin::Argc => Value::Number(self.arguments.len() as f64),
            Builtin::Arg => {
                let index = count(args[0], who, "index")?;
                let text = match index.checked_sub(1).and_then(|i| self.arguments.get(i)) {
                    Some(argument) => argument.clone(),
                    None => {
                        return Err(if self.arguments.is_empty() {
                            format!(
                                "ARG$: index {index} out of range (no command-line arguments; ARGC is 0)"
                            )
                        } else {
                            format!(
                                "ARG$: index {index} out of range (ARGC is {})",
                                self.arguments.len()
                            )
                        });
                    }
                };
                self.allocate(&text)?
            }
            Builtin::Env => {
                let name = self.text(args[0], who)?;
                if name.is_empty() || name.contains(['=', '\0']) {
                    return Err("ENV$: invalid environment variable name".into());
                }
                let text = match std::env::var(&name) {
                    Ok(text) => text,
                    Err(std::env::VarError::NotPresent) => String::new(),
                    Err(std::env::VarError::NotUnicode(_)) => {
                        return Err("ENV$: the value is not valid Unicode".into());
                    }
                };
                self.allocate(&text)?
            }
            Builtin::Open => {
                let path = self.text(args[0], who)?;
                let file = file_number(args[1], who)?;
                let mode = Mode::from_code(num(args[2], who)?).ok_or("OPEN: invalid mode")?;
                self.files
                    .open(file, &path, mode)
                    .map_err(|e| format!("{who}: {e}"))?;
                return Ok(None);
            }
            Builtin::Close => {
                let result = if args.is_empty() {
                    self.files.close_all()
                } else {
                    // Validate every number first so CLOSE is never partially applied.
                    let mut numbers = Vec::with_capacity(args.len());
                    for argument in args.iter() {
                        let number = file_number(*argument, who)?;
                        if !self.files.is_open(number) {
                            return Err(format!("{who}: file #{number} is not open"));
                        }
                        if numbers.contains(&number) {
                            return Err(format!("{who}: file #{number} is listed twice"));
                        }
                        numbers.push(number);
                    }
                    numbers
                        .into_iter()
                        .try_for_each(|number| self.files.close(number))
                };
                result.map_err(|e| format!("{who}: {e}"))?;
                return Ok(None);
            }
            Builtin::Put => {
                let file = file_number(args[0], who)?;
                let (_, _, items) = self.sequence_argument(args[1], who)?;
                let mut bytes = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    let byte = match item {
                        Value::Number(n) if n.fract() == 0.0 && (0.0..=255.0).contains(n) => {
                            Some(*n as u8)
                        }
                        Value::Int(n) => u8::try_from(*n).ok(),
                        _ => None,
                    };
                    match byte {
                        Some(byte) => bytes.push(byte),
                        None => {
                            return Err(format!(
                                "PUT: element {} is not a whole number from 0 to 255",
                                index + 1
                            ));
                        }
                    }
                }
                self.files
                    .put_bytes(file, &bytes)
                    .map_err(|e| format!("{who}: {e}"))?;
                return Ok(None);
            }
            Builtin::Seek => {
                let file = file_number(args[0], who)?;
                let position = count(args[1], who, "position")?;
                self.files
                    .seek(file, position as u64)
                    .map_err(|e| format!("{who}: {e}"))?;
                return Ok(None);
            }
            Builtin::Kill => {
                let path = self.text(args[0], who)?;
                if let Some(file) = self.files.open_as(Path::new(&path)) {
                    return Err(format!(
                        "KILL: \"{path}\" is open as file #{file}; CLOSE it first"
                    ));
                }
                std::fs::remove_file(&path)
                    .map_err(|e| format!("KILL: cannot delete \"{path}\": {}", describe(&e)))?;
                return Ok(None);
            }
            Builtin::Eof => {
                let file = file_number(args[0], who)?;
                boolean(self.files.at_end(file).map_err(|e| format!("{who}: {e}"))?)
            }
            Builtin::Lof | Builtin::Loc => {
                let file = file_number(args[0], who)?;
                let result = if builtin == Builtin::Lof {
                    self.files.length(file)
                } else {
                    self.files.position(file)
                };
                Value::Number(result.map_err(|e| format!("{who}: {e}"))? as f64)
            }
            Builtin::Freefile => Value::Number(
                self.files
                    .free_number()
                    .ok_or("FREEFILE: all file numbers are in use")? as f64,
            ),
            Builtin::IsOpen => boolean(self.files.is_open(file_number(args[0], who)?)),
            Builtin::FileExists => {
                let path = self.text(args[0], who)?;
                boolean(Path::new(&path).is_file())
            }
            Builtin::Get => {
                let file = file_number(args[0], who)?;
                let wanted = count(args[1], who, "count")?;
                if wanted > MAX_READ_BYTES {
                    return Err(format!(
                        "GET: count must not exceed {MAX_READ_BYTES}; read larger files in chunks"
                    ));
                }
                let bytes = self
                    .files
                    .read_bytes(file, wanted)
                    .map_err(|e| format!("{who}: {e}"))?;
                let items: Vec<Value> =
                    bytes.iter().map(|b| Value::Number(f64::from(*b))).collect();
                self.new_list(&items)?
            }
            Builtin::ToInt | Builtin::ToInt64 => {
                let strict = builtin == Builtin::ToInt64;
                match args[0] {
                    Value::Int(n) => Value::Int(n),
                    Value::Number(x) => {
                        let whole = if strict { x } else { x.round() };
                        Value::Int(float_to_int(whole).ok_or_else(|| {
                            if strict {
                                format!("{who}: {x} is not a whole number in the 64-bit range")
                            } else {
                                format!("{who}: {x} is outside the 64-bit integer range")
                            }
                        })?)
                    }
                    Value::String(_) => {
                        let text = self.text(args[0], who)?;
                        Value::Int(text.trim().parse::<i64>().map_err(|_| {
                            format!("{who}: {text:?} is not a whole number in the 64-bit range")
                        })?)
                    }
                    Value::Collection(_) | Value::Record(_) | Value::Nothing => {
                        return Err(format!("{who}: expected a number or string"));
                    }
                }
            }
            Builtin::ToDouble => match args[0] {
                Value::Number(x) => Value::Number(x),
                Value::Int(n) => Value::Number(n as f64),
                Value::String(_) => {
                    let text = self.text(args[0], who)?;
                    match text.trim().parse::<f64>() {
                        Ok(x) if x.is_finite() => Value::Number(x + 0.0),
                        _ => return Err(format!("{who}: {text:?} is not a finite number")),
                    }
                }
                Value::Collection(_) | Value::Record(_) | Value::Nothing => {
                    return Err(format!("{who}: expected a number or string"));
                }
            },
            Builtin::ToBool => match args[0] {
                Value::Number(x) => boolean(x != 0.0),
                Value::Int(n) => boolean(n != 0),
                Value::String(_) => {
                    let text = self.text(args[0], who)?;
                    match text.trim().to_uppercase().as_str() {
                        "TRUE" => boolean(true),
                        "FALSE" => boolean(false),
                        _ => return Err(format!("{who}: {text:?} is not TRUE or FALSE")),
                    }
                }
                Value::Collection(_) | Value::Record(_) | Value::Nothing => {
                    return Err(format!("{who}: expected a number or string"));
                }
            },
            Builtin::ToStr => match args[0] {
                Value::Number(x) => self.allocate(&x.to_string())?,
                Value::Int(n) => self.allocate(&n.to_string())?,
                Value::String(_) => args[0],
                Value::Collection(_) | Value::Record(_) | Value::Nothing => {
                    return Err(format!("{who}: expected a number or string"));
                }
            },
            Builtin::TypeOf => {
                let name = self.type_name(args[0])?;
                self.allocate(name)?
            }
            Builtin::TypeName => match args[0] {
                Value::Record(reference) => {
                    let name = self.program.types[self.heap.record_type(reference)?]
                        .name
                        .clone();
                    self.allocate(&name)?
                }
                other => {
                    let name = self.type_name(other)?;
                    self.allocate(name)?
                }
            },
            Builtin::IsRecord => boolean(matches!(args[0], Value::Record(_))),
            Builtin::Copy => match args[0] {
                Value::Record(reference) => {
                    let ty = self.heap.record_type(reference)?;
                    let fields = self.heap.record_fields(reference)?;
                    let roots = self.roots();
                    Value::Record(self.heap.new_record(ty, &fields, &roots)?)
                }
                Value::Collection(reference) => {
                    let (kind, items) = self.heap.collection(reference)?;
                    let roots = self.roots();
                    Value::Collection(self.heap.new_collection(kind, &items, &roots)?)
                }
                other => other,
            },
            Builtin::Fields => {
                let Value::Record(reference) = args[0] else {
                    return Err(format!("{who}: expected a record argument"));
                };
                let names: Vec<String> = self.program.types[self.heap.record_type(reference)?]
                    .fields
                    .iter()
                    .map(|field| field.name.clone())
                    .collect();
                // Each new name is only reachable from `roots` until the
                // list payload is allocated, so root it as we go.
                let base = self.roots();
                let mut roots = base.clone();
                let mut items = Vec::with_capacity(names.len());
                for name in &names {
                    let reference = self.heap.allocate(name, &roots)?;
                    roots.push(reference);
                    items.push(Value::String(reference));
                }
                self.heap
                    .new_collection(CollectionKind::List, &items, &base)
                    .map(Value::Collection)?
            }
            Builtin::GetField => {
                let name = self.text(args[1], who)?.to_ascii_uppercase();
                self.get_field(args[0], &name)
                    .map_err(|message| format!("{who}: {message}"))?
            }
            Builtin::SetField => {
                let name = self.text(args[1], who)?.to_ascii_uppercase();
                self.set_field(args[0], &name, args[2])
                    .map_err(|message| format!("{who}: {message}"))?;
                return Ok(None);
            }
            Builtin::IsNumeric => boolean(matches!(args[0], Value::Number(_) | Value::Int(_))),
            Builtin::IsInteger => boolean(matches!(args[0], Value::Int(_))),
            Builtin::IsString => boolean(matches!(args[0], Value::String(_))),
            Builtin::IsArray | Builtin::IsList | Builtin::IsMap => {
                let wanted = match builtin {
                    Builtin::IsArray => CollectionKind::Array,
                    Builtin::IsList => CollectionKind::List,
                    _ => CollectionKind::Map,
                };
                boolean(
                    matches!(args[0], Value::Collection(r) if self.heap.collection_kind(r)? == wanted),
                )
            }
            Builtin::UBound | Builtin::LBound => {
                let (_, kind, items) = self.collection_argument(args[0], who)?;
                if kind == CollectionKind::Map {
                    return Err(format!("{who}: expected an array or list, found a map"));
                }
                let dimension = match args.get(1) {
                    Some(value) => count(*value, who, "dimension")?,
                    None => 1,
                };
                if dimension < 1 {
                    return Err(format!("{who}: dimension must be at least 1"));
                }
                let mut length = items.len();
                let mut first = items.first().copied();
                for _ in 1..dimension {
                    let Some(Value::Collection(row)) = first else {
                        return Err(format!("{who}: the array has no dimension {dimension}"));
                    };
                    let (row_kind, row_items) = self.heap.collection(row)?;
                    if row_kind != CollectionKind::Array {
                        return Err(format!("{who}: the array has no dimension {dimension}"));
                    }
                    length = row_items.len();
                    first = row_items.first().copied();
                }
                // Arrays are one-based; LBOUND is 1 for every dimension.
                Value::Number(if builtin == Builtin::LBound {
                    1.0
                } else {
                    length as f64
                })
            }
            Builtin::ErrMessage | Builtin::ErrLine | Builtin::ErrFunc => {
                let (line, message, function) = {
                    let fault = self
                        .current_error()
                        .ok_or_else(|| format!("{who}: no error is being handled"))?;
                    (
                        fault.line,
                        fault.message.clone(),
                        fault.function.clone().unwrap_or_default(),
                    )
                };
                match builtin {
                    Builtin::ErrLine => Value::Number(line as f64),
                    Builtin::ErrMessage => self.allocate(&message)?,
                    _ => self.allocate(&function)?,
                }
            }
            // Routed to `media_builtin` by the VM before reaching here.
            _ => unreachable!("media builtins are dispatched separately"),
        };
        Ok(Some(value))
    }

    /// The runtime type name reported by TYPEOF. Extend here for new kinds.
    fn type_name(&self, value: Value) -> Result<&'static str> {
        Ok(match value {
            Value::Int(_) => "INTEGER",
            Value::Number(_) => "DOUBLE",
            Value::String(_) => "STRING",
            Value::Collection(reference) => match self.heap.collection_kind(reference)? {
                CollectionKind::List => "LIST",
                CollectionKind::Map => "MAP",
                CollectionKind::Array => "ARRAY",
            },
            Value::Record(_) => "RECORD",
            Value::Nothing => "NOTHING",
        })
    }

    /// Stable sort of all-number or all-string items; strings by code point.
    fn sorted(&self, items: Vec<Value>) -> Result<Vec<Value>> {
        if items
            .iter()
            .all(|item| matches!(item, Value::Number(_) | Value::Int(_)))
        {
            let mut items = items;
            items.sort_by(|a, b| cmp_numeric(*a, *b).unwrap_or(Ordering::Equal));
            return Ok(items);
        }
        let mut keyed = Vec::with_capacity(items.len());
        for item in items {
            let Value::String(reference) = item else {
                return Err("SORT: elements must be all numbers or all strings".into());
            };
            keyed.push((self.heap.get(reference)?.to_string(), item));
        }
        keyed.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(keyed.into_iter().map(|(_, item)| item).collect())
    }
}
