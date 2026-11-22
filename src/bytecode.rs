use std::sync::Arc;

/// Extra call information for named arguments and BYREF parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct CallBinding {
    /// The parameter index of each pushed argument, in push order; empty
    /// means the arguments are positional (argument `i` is parameter `i`).
    pub slots: Vec<usize>,
    /// The BYREF arguments, ordered by argument position.
    pub refs: Vec<RefArg>,
}

/// A BYREF argument: where the callee's final parameter value is written back.
#[derive(Debug, Clone, PartialEq)]
pub struct RefArg {
    /// Position of the argument in push order.
    pub arg: usize,
    pub place: Place,
}

/// The caller-side lvalue of a BYREF argument. Operands of the place (the
/// container and index, or the record) are pushed before the argument value.
#[derive(Debug, Clone, PartialEq)]
pub enum Place {
    /// A variable (global, local or parameter of the calling scope).
    Variable(String),
    /// A list, map or array element: operands are `container, index`.
    Element,
    /// A record field: the operand is the record.
    Field(String),
}

impl Place {
    /// Number of operands pushed beneath the argument value.
    pub(crate) fn operands(&self) -> usize {
        match self {
            Self::Variable(_) => 0,
            Self::Element => 2,
            Self::Field(_) => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    /// Integer division (`\`), truncating toward zero.
    IntDivide,
    Modulo,
    Power,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Number(f64),
    /// Exact 64-bit integer constant.
    Int(i64),
    String(String),
    Load(String),
    Store(String),
    /// Declares a typed variable (`DIM`, typed `LOCAL`): pops the initial
    /// value (or uses the type's default), registers `ty` and stores it.
    Declare {
        name: String,
        ty: VarType,
        has_init: bool,
    },
    /// Pops `dims` dimension sizes and pushes a new, nested, filled array.
    DimArray {
        dims: usize,
        fill: VarType,
    },
    /// Pops `dims` sizes and the array beneath them and resizes it in place.
    Redim {
        dims: usize,
        preserve: bool,
        fill: VarType,
    },
    Negate,
    /// Logical NOT: pops a number and pushes 1 when it was zero, otherwise 0.
    Not,
    /// Normalizes the top number to 1 or 0.
    Truth,
    /// ANDALSO: pops a number; when zero pushes 0 and jumps past the right operand.
    AndJump(usize),
    /// ORELSE: pops a number; when non-zero pushes 1 and jumps past the right operand.
    OrJump(usize),
    /// Duplicates the top operand.
    Dup,
    /// Duplicates the top two operands, keeping their order.
    Dup2,
    /// Copies the operand below the top one onto the top (used by SELECT CASE ranges).
    Over,
    /// Discards the top operand.
    Pop,
    Binary(BinaryOp),
    Collection(CollectionKind, usize),
    IndexGet,
    IndexSet,
    Length,
    /// Pushes the `NOTHING` null reference.
    Nothing,
    /// Pops the field values named by `fields` (indexes into the type's
    /// field list, in stack order) and pushes a new instance of type `ty`.
    NewRecord {
        ty: usize,
        fields: Vec<usize>,
    },
    /// Pops a record and pushes the named field.
    GetField(String),
    /// Pops a value and a record and stores the value in the named field.
    SetField(String),
    Append,
    Remove,
    ForStart {
        variable: String,
        end: usize,
    },
    EachStart {
        variable: String,
        /// Second loop variable of `FOR EACH key, value IN map`.
        value_variable: Option<String>,
        end: usize,
    },
    ForNext {
        start: usize,
    },
    Call {
        function: usize,
        arguments: usize,
        discard_result: bool,
        /// Present only when a call uses named arguments or BYREF places.
        binding: Option<Arc<CallBinding>>,
    },
    Return,
    /// Calls a method by name on a receiver whose class is only known at run
    /// time. Operands: the receiver, then `arguments` positional arguments.
    CallMethod {
        name: String,
        arguments: usize,
        discard_result: bool,
    },
    /// Standard-library call; pushes a result unless `discard_result` is set
    /// (statement-form builtins such as SORT never produce one).
    Builtin {
        builtin: Builtin,
        arguments: usize,
        discard_result: bool,
    },
    Jump(usize),
    JumpIfFalse(usize),
    JumpIfTrue(usize),
    Print,
    PrintSeparator,
    PrintNewline,
    /// File forms of the print ops: the file number stays on the operand
    /// stack beneath the printed values until the trailing `Pop`.
    FilePrint,
    FilePrintSeparator,
    FilePrintNewline,
    Input(String),
    /// Pops a file number and reads one line from that file into the variable.
    FileInput(String),
    /// Opens an exception handler; `catch` and `finally` are entry addresses.
    TryBegin {
        catch: Option<usize>,
        finally: Option<usize>,
    },
    /// Leaves `tries` enclosing TRY blocks (running their FINALLY blocks, inner
    /// first) and then jumps to `target`.
    Leave {
        target: usize,
        tries: usize,
    },
    /// First instruction of a CATCH block: stores the error message.
    CatchStore(Option<String>),
    /// End of a FINALLY block: resumes the pending exit, jump or error.
    EndFinally,
    /// Raises a new error with the string on top of the stack.
    Throw,
    /// Raises the error of the CATCH block `depth` TRY blocks below the innermost.
    Rethrow(usize),
    Collect,
    /// `ON KEY|MOUSE|CLOSE CALL f` registers function `f`; `ON ... OFF`
    /// (`function: None` after linking) removes the handler.
    OnEvent {
        kind: EventKind,
        function: Option<usize>,
    },
    Halt,
}

/// The kinds of event a handler function can be registered for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Key,
    Mouse,
    Close,
}

impl EventKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Key => "KEY",
            Self::Mouse => "MOUSE",
            Self::Close => "CLOSE",
        }
    }

    /// Number of arguments the handler function receives.
    pub fn handler_parameters(self) -> usize {
        match self {
            Self::Key => 1,
            Self::Mouse => 3,
            Self::Close => 0,
        }
    }

    pub(crate) fn index(self) -> usize {
        self as usize
    }
}

macro_rules! builtins {
    ($($variant:ident => $name:literal, $min:expr, $max:expr;)*) => {
        /// Standard-library routines. Function forms are recognised by name
        /// only when followed by `(`; user functions of the same name win.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Builtin {
            $($variant,)*
        }

        impl Builtin {
            const ALL: &'static [Builtin] = &[$(Builtin::$variant,)*];

            pub fn name(self) -> &'static str {
                match self { $(Builtin::$variant => $name,)* }
            }

            /// Inclusive (minimum, maximum) argument counts.
            pub fn arity(self) -> (usize, usize) {
                match self { $(Builtin::$variant => ($min, $max),)* }
            }
        }
    };
}

builtins! {
    Abs => "ABS", 1, 1;
    Sqrt => "SQRT", 1, 1;
    Min => "MIN", 2, usize::MAX;
    Max => "MAX", 2, usize::MAX;
    Round => "ROUND", 1, 2;
    Int => "INT", 1, 1;
    Fix => "FIX", 1, 1;
    Sin => "SIN", 1, 1;
    Cos => "COS", 1, 1;
    Log => "LOG", 1, 1;
    Exp => "EXP", 1, 1;
    Tan => "TAN", 1, 1;
    Atn => "ATN", 1, 1;
    Atn2 => "ATN2", 2, 2;
    Pi => "PI", 0, 0;
    Rnd => "RND", 0, 0;
    Left => "LEFT$", 2, 2;
    Right => "RIGHT$", 2, 2;
    Mid => "MID$", 2, 3;
    Instr => "INSTR", 2, 3;
    Replace => "REPLACE$", 3, 3;
    Trim => "TRIM$", 1, 1;
    Ucase => "UCASE$", 1, 1;
    Lcase => "LCASE$", 1, 1;
    Str => "STR$", 1, 1;
    Val => "VAL", 1, 1;
    Split => "SPLIT", 2, 2;
    Join => "JOIN$", 2, 2;
    Pop => "POP", 1, 1;
    Contains => "CONTAINS", 2, 2;
    Keys => "KEYS", 1, 1;
    Values => "VALUES", 1, 1;
    HasKey => "HASKEY", 2, 2;
    Timer => "TIMER", 0, 0;
    UnixTime => "UNIXTIME", 0, 0;
    Now => "NOW$", 0, 0;
    Date => "DATE$", 0, 0;
    Time => "TIME$", 0, 0;
    Argc => "ARGC", 0, 0;
    Arg => "ARG$", 1, 1;
    Env => "ENV$", 1, 1;
    Insert => "INSERT", 3, 3;
    Sort => "SORT", 1, 1;
    Reverse => "REVERSE", 1, 1;
    Randomize => "RANDOMIZE", 0, 1;
    Open => "OPEN", 3, 3;
    Close => "CLOSE", 0, usize::MAX;
    Put => "PUT", 2, 2;
    Seek => "SEEK", 2, 2;
    Kill => "KILL", 1, 1;
    Eof => "EOF", 1, 1;
    Lof => "LOF", 1, 1;
    Loc => "LOC", 1, 1;
    Freefile => "FREEFILE", 0, 0;
    IsOpen => "ISOPEN", 1, 1;
    FileExists => "FILEEXISTS", 1, 1;
    Get => "GET", 2, 2;
    ErrMessage => "ERR$", 0, 0;
    ErrLine => "ERRLINE", 0, 0;
    ErrFunc => "ERRFUNC$", 0, 0;
    ToInt => "CINT", 1, 1;
    ToInt64 => "INT64", 1, 1;
    ToDouble => "CDBL", 1, 1;
    ToBool => "CBOOL", 1, 1;
    ToStr => "CSTR", 1, 1;
    TypeOf => "TYPEOF", 1, 1;
    TypeName => "TYPENAME$", 1, 1;
    IsNumeric => "ISNUMERIC", 1, 1;
    IsInteger => "ISINTEGER", 1, 1;
    IsString => "ISSTRING", 1, 1;
    IsArray => "ISARRAY", 1, 1;
    IsList => "ISLIST", 1, 1;
    IsMap => "ISMAP", 1, 1;
    UBound => "UBOUND", 1, 2;
    LBound => "LBOUND", 1, 2;
    Copy => "COPY", 1, 1;
    Fields => "FIELDS", 1, 1;
    GetField => "GETFIELD", 2, 2;
    SetField => "SETFIELD", 3, 3;
    IsRecord => "ISRECORD", 1, 1;
    Cls => "CLS", 0, 0;
    Color => "COLOR", 1, 2;
    Locate => "LOCATE", 1, 2;
    Sleep => "SLEEP", 1, 1;
    Screen => "SCREEN", 1, 3;
    Gcls => "GCLS", 0, 1;
    Gcolor => "GCOLOR", 1, 1;
    Pset => "PSET", 2, 3;
    Line => "LINE", 4, 5;
    LineBox => "LINE B", 4, 5;
    LineFill => "LINE BF", 4, 5;
    Circle => "CIRCLE", 3, 4;
    CircleFill => "CIRCLE FILL", 3, 4;
    DrawText => "TEXT", 3, 4;
    Flip => "FLIP", 0, 0;
    Present => "PRESENT", 0, 0;
    DoEvents => "DOEVENTS", 0, 0;
    Rgb => "RGB", 3, 3;
    Point => "POINT", 2, 2;
    Inkey => "INKEY$", 0, 0;
    GetKey => "GETKEY$", 0, 0;
    KeyDown => "KEYDOWN", 1, 1;
    MouseX => "MOUSEX", 0, 0;
    MouseY => "MOUSEY", 0, 0;
    MouseB => "MOUSEB", 0, 0;
    Closed => "CLOSED", 0, 0;
    IsGraphics => "ISGRAPHICS", 0, 0;
    WindowOpen => "WINDOWOPEN", 0, 0;
    ScreenW => "SCREENW", 0, 0;
    ScreenH => "SCREENH", 0, 0;
}

impl Builtin {
    /// Console and graphics routines, executed by the VM's media layer.
    pub(crate) fn is_media(self) -> bool {
        use Builtin::*;
        matches!(
            self,
            Cls | Color
                | Locate
                | Sleep
                | Screen
                | Gcls
                | Gcolor
                | Pset
                | Line
                | LineBox
                | LineFill
                | Circle
                | CircleFill
                | DrawText
                | Flip
                | Present
                | DoEvents
                | Rgb
                | Point
                | Inkey
                | GetKey
                | KeyDown
                | MouseX
                | MouseY
                | MouseB
                | Closed
                | IsGraphics
                | WindowOpen
                | ScreenW
                | ScreenH
        )
    }

    /// Statement-form builtins are parsed as statements, never as functions.
    pub fn is_statement(self) -> bool {
        use Builtin::*;
        matches!(
            self,
            Cls | Color
                | Locate
                | Sleep
                | Screen
                | Gcls
                | Gcolor
                | Pset
                | Line
                | LineBox
                | LineFill
                | Circle
                | CircleFill
                | DrawText
                | Flip
                | Present
                | DoEvents
        ) || matches!(
            self,
            Self::Insert
                | Self::Sort
                | Self::Reverse
                | Self::Randomize
                | Self::Open
                | Self::Close
                | Self::Put
                | Self::Seek
                | Self::Kill
                | Self::SetField
        )
    }

    pub(crate) fn function(name: &str) -> Option<Self> {
        let name = if name == "SQR" { "SQRT" } else { name };
        Self::ALL
            .iter()
            .copied()
            .find(|builtin| !builtin.is_statement() && builtin.name() == name)
    }

    pub(crate) fn statement(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|builtin| builtin.is_statement() && builtin.name() == name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionKind {
    Array,
    List,
    Map,
}

/// Declared variable types (`DIM x AS ...`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarType {
    /// Exact 64-bit integer (`INTEGER`, `LONG`).
    Integer,
    Double,
    /// Numeric 1/0, normalised on assignment.
    Boolean,
    String,
    List,
    Map,
    Array,
    /// Dynamic: holds any value.
    Variant,
    /// A record type, by index into `Program::types()`. Holds an instance of
    /// that type or `NOTHING`.
    Record(usize),
}

impl VarType {
    /// The keyword spelling; record types report `RECORD` (their own names
    /// live in the type table).
    pub fn name(self) -> &'static str {
        match self {
            Self::Integer => "INTEGER",
            Self::Double => "DOUBLE",
            Self::Boolean => "BOOLEAN",
            Self::String => "STRING",
            Self::List => "LIST",
            Self::Map => "MAP",
            Self::Array => "ARRAY",
            Self::Variant => "VARIANT",
            Self::Record(_) => "RECORD",
        }
    }

    pub(crate) fn parse(word: &str) -> Option<Self> {
        Some(match word {
            "INTEGER" | "LONG" => Self::Integer,
            "DOUBLE" => Self::Double,
            "BOOLEAN" => Self::Boolean,
            "STRING" => Self::String,
            "LIST" => Self::List,
            "MAP" => Self::Map,
            "ARRAY" => Self::Array,
            "VARIANT" => Self::Variant,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Instruction {
    pub op: Op,
    pub line: usize,
    /// Index into `Program::files()`; 0 is the main program.
    pub file: usize,
}

#[derive(Debug, Clone)]
pub struct Program {
    pub(crate) instructions: Vec<Instruction>,
    pub(crate) functions: Vec<Function>,
    pub(crate) types: Vec<RecordType>,
    /// Display paths of the source files; entry 0 (the main program) is empty.
    pub(crate) files: Vec<String>,
}

/// A literal default of a record field (`field AS INTEGER = 3`).
#[derive(Debug, Clone, PartialEq)]
pub enum FieldDefault {
    Number(f64),
    Int(i64),
    Text(String),
    Nothing,
    /// A global `CONST`, read when an instance is created.
    Constant(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordField {
    pub name: String,
    pub ty: VarType,
    pub default: Option<FieldDefault>,
}

/// One entry of the program's type table. An instance is a managed object
/// tagged with its index here, so everything about an object's shape lives
/// in this descriptor.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordType {
    pub name: String,
    /// Fields in declaration order; the payload of an instance has one value
    /// per entry, in the same order.
    pub fields: Vec<RecordField>,
    /// `(method name, index into the function table)` pairs of a `CLASS`. A
    /// method is an ordinary function whose first parameter is the instance
    /// (`SELF`) typed `VarType::Record` of this type. Always empty for plain
    /// `TYPE` records.
    pub methods: Vec<(String, usize)>,
    /// Declared with `CLASS` rather than `TYPE`.
    pub is_class: bool,
    /// Source line of the declaration.
    pub line: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct Function {
    /// The name as written in its own module; also its result variable.
    pub(crate) name: String,
    /// The module-qualified name (`ALIAS.F`; equal to `name` in the main program).
    pub(crate) qualified: String,
    pub(crate) entry: usize,
    pub(crate) parameters: Vec<String>,
    /// Passing mode and default of each parameter.
    pub(crate) params: Vec<ParamInfo>,
    pub(crate) locals: Vec<String>,
    /// Declared types of parameters, locals and the result (by name).
    pub(crate) types: std::collections::HashMap<String, VarType>,
    /// The class (type id) this function is a method of; its first
    /// parameter is `SELF`.
    pub(crate) class: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParamInfo {
    pub(crate) by_ref: bool,
    pub(crate) optional: bool,
    /// Literal default of an optional parameter; `None` means the type's default.
    pub(crate) default: Option<FieldDefault>,
}

impl Program {
    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }

    /// The display path of a source file index (`Instruction::file`);
    /// empty for the main program.
    pub fn file_name(&self, file: usize) -> &str {
        self.files.get(file).map_or("", String::as_str)
    }

    /// The record type table (`TYPE ... END TYPE` declarations).
    pub fn types(&self) -> &[RecordType] {
        &self.types
    }
}
