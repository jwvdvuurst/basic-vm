use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use crate::Error;
use crate::bytecode::{
    BinaryOp, Builtin, CallBinding, CollectionKind, EventKind, FieldDefault, Function, Instruction,
    Op, ParamInfo, Place, Program, RecordField, RecordType, RefArg, TestCase, VarType,
};
use crate::lexer::{Token, lex, spelled_after};
use crate::value::float_to_int;

/// The record types of a program, gathered before the main pass so that
/// types can be used textually before their declaration.
struct TypeTable {
    types: Vec<RecordType>,
    ids: HashMap<String, usize>,
    /// Header line of each `TYPE` block -> its `END TYPE` line.
    blocks: HashMap<usize, usize>,
    /// Global constants used as field defaults, checked after the main pass.
    constants: Vec<(String, usize)>,
    /// Header line of each `CLASS` block -> its `END CLASS` line.
    class_blocks: HashMap<usize, usize>,
    /// Lines of a class body that declare no code (header, fields, END CLASS);
    /// the main pass skips them, but compiles the methods between them.
    skips: HashSet<usize>,
    /// `FUNCTION` header line of each method -> the class's type id.
    method_headers: HashMap<usize, usize>,
    /// Method signatures (including the leading `SELF`) by type id and name.
    methods: HashMap<usize, HashMap<String, Signature>>,
}

struct RawType {
    name: String,
    line: usize,
    end: usize,
    is_class: bool,
    body: Vec<(usize, Vec<Token>)>,
    /// Class methods: header line and the tokens after `FUNCTION`.
    methods: Vec<(usize, Vec<Token>)>,
}

/// A possibly module-qualified type name (`Name` or `Alias.Name`) at the
/// start of `tokens`; returns the lookup key and the number of tokens used.
fn qualified_name(tokens: &[Token]) -> Option<(String, usize)> {
    match tokens {
        [Token::Word(alias), Token::Dot, Token::Word(name), ..] => {
            Some((format!("{alias}.{name}"), 3))
        }
        [Token::Word(name), ..] => Some((name.clone(), 1)),
        _ => None,
    }
}

/// Parses the type name after `AS`; returns the type and the tokens used.
fn type_at(
    tokens: &[Token],
    line: usize,
    ids: &HashMap<String, usize>,
) -> Result<(VarType, usize), Error> {
    let Some((word, used)) = qualified_name(tokens) else {
        return Err(Error::new(line, "expected a type name after AS"));
    };
    VarType::parse(&word)
        .or_else(|| ids.get(&word).map(|id| VarType::Record(*id)))
        .map(|ty| (ty, used))
        .ok_or_else(|| {
            Error::new(
                line,
                format!(
                    "unknown type {word}; expected INTEGER, LONG, DOUBLE, BOOLEAN, \
                     STRING, LIST, MAP, ARRAY, VARIANT or a TYPE name"
                ),
            )
        })
}

/// Whether a literal default can initialise a value of the declared type.
fn default_fits(ty: VarType, default: &FieldDefault) -> bool {
    match (ty, default) {
        (VarType::Variant, _) | (_, FieldDefault::Constant(_)) => {
            !matches!(ty, VarType::List | VarType::Map | VarType::Array)
        }
        (VarType::Integer, FieldDefault::Int(_)) => true,
        (VarType::Integer, FieldDefault::Number(n)) => float_to_int(*n).is_some(),
        (VarType::Double | VarType::Boolean, FieldDefault::Number(_) | FieldDefault::Int(_)) => {
            true
        }
        (VarType::String, FieldDefault::Text(_)) => true,
        (VarType::Record(_), FieldDefault::Nothing) => true,
        _ => false,
    }
}

/// A line is a type declaration only when it is exactly `TYPE name` or
/// `CLASS name`, which is never a valid legacy statement, so variables named
/// TYPE or CLASS keep working.
fn collect_types(
    source: &str,
    offset: usize,
    prefix: &str,
    imported: HashMap<String, usize>,
) -> Result<TypeTable, Error> {
    let mut raws: Vec<RawType> = Vec::new();
    let mut open: Option<RawType> = None;
    let mut in_method = false;
    let mut ids = imported;
    for (index, text) in source.lines().enumerate() {
        let line = index + 1;
        let tokens = match lex(text, line) {
            Ok(tokens) => tokens,
            // The main pass reports lexical errors outside TYPE blocks.
            Err(error) if open.is_some() => return Err(error),
            Err(_) => continue,
        };
        if let Some(raw) = open.as_mut() {
            if let [Token::Word(test), Token::Word(next), ..] = tokens.as_slice()
                && test == "TEST"
                && next != "AS"
                && !in_method
            {
                return Err(Error::new(
                    line,
                    format!(
                        "TEST blocks must be at the top level, not inside {} {}",
                        if raw.is_class { "CLASS" } else { "TYPE" },
                        raw.name
                    ),
                ));
            }
            if raw.is_class {
                if in_method {
                    in_method = tokens != [Token::Word("FEND".into())];
                } else if matches!(tokens.as_slice(), [Token::Word(end), Token::Word(ty)]
                    if end == "END" && ty == "CLASS")
                {
                    raw.end = line;
                    raws.extend(open.take());
                } else if let [Token::Word(keyword), rest @ ..] = tokens.as_slice()
                    && keyword == "FUNCTION"
                    && matches!(rest.first(), Some(Token::Word(_)))
                {
                    raw.methods.push((line, rest.to_vec()));
                    in_method = true;
                } else if !tokens.is_empty() {
                    raw.body.push((line, tokens));
                }
            } else if matches!(tokens.as_slice(), [Token::Word(end), Token::Word(ty)]
                if end == "END" && ty == "TYPE")
            {
                raw.end = line;
                raws.extend(open.take());
            } else if !tokens.is_empty() {
                raw.body.push((line, tokens));
            }
            continue;
        }
        let [Token::Word(keyword), Token::Word(name)] = tokens.as_slice() else {
            continue;
        };
        if keyword != "TYPE" && keyword != "CLASS" {
            continue;
        }
        let is_class = keyword == "CLASS";
        if is_keyword(name) || name.ends_with('$') || VarType::parse(name).is_some() {
            return Err(Error::new(line, format!("invalid record type name {name}")));
        }
        if ids.insert(name.clone(), offset + raws.len()).is_some() {
            return Err(Error::new(line, format!("duplicate record type {name}")));
        }
        open = Some(RawType {
            name: name.clone(),
            line,
            end: line,
            is_class,
            body: Vec::new(),
            methods: Vec::new(),
        });
        // Reserve the index now: `raws.len()` is the index once it closes.
        // Nothing can be pushed between here and the matching END TYPE.
    }
    if let Some(raw) = open {
        return Err(if in_method {
            Error::new(
                raw.line,
                "unterminated CLASS: a method is missing FEND (expected END CLASS)",
            )
        } else if raw.is_class {
            Error::new(raw.line, "unterminated CLASS; expected END CLASS")
        } else {
            Error::new(raw.line, "unterminated TYPE; expected END TYPE")
        });
    }
    let mut table = TypeTable {
        types: Vec::new(),
        ids,
        blocks: HashMap::new(),
        constants: Vec::new(),
        class_blocks: HashMap::new(),
        skips: HashSet::new(),
        method_headers: HashMap::new(),
        methods: HashMap::new(),
    };
    for raw in raws {
        let id = table.ids[&raw.name];
        let kind = if raw.is_class { "CLASS" } else { "TYPE" };
        if raw.is_class {
            table.class_blocks.insert(raw.line, raw.end);
            table.skips.insert(raw.line);
            table.skips.insert(raw.end);
        } else {
            table.blocks.insert(raw.line, raw.end);
        }
        let full_name = if prefix.is_empty() {
            raw.name.clone()
        } else {
            format!("{prefix}.{}", raw.name)
        };
        let mut fields: Vec<RecordField> = Vec::new();
        for (line, tokens) in &raw.body {
            table.skips.insert(*line);
            let (field, constant) = parse_field(*line, tokens, &table.ids, kind)?;
            if fields.iter().any(|existing| existing.name == field.name) {
                return Err(Error::new(
                    *line,
                    format!("duplicate field {} in {kind} {}", field.name, raw.name),
                ));
            }
            if let Some(constant) = constant {
                table.constants.push((constant, *line));
            }
            fields.push(field);
        }
        let mut methods: Vec<(String, usize)> = Vec::new();
        let mut signatures: HashMap<String, Signature> = HashMap::new();
        for (line, tokens) in &raw.methods {
            let (signature, _) = method_signature(tokens, *line, &table.ids, &full_name, id)?;
            if signatures.contains_key(&signature.name) {
                return Err(Error::new(
                    *line,
                    format!(
                        "duplicate method {} in CLASS {} (methods cannot be overloaded)",
                        signature.name, raw.name
                    ),
                ));
            }
            if fields.iter().any(|field| field.name == signature.name) {
                return Err(Error::new(
                    *line,
                    format!(
                        "CLASS {} has both a field and a method named {}",
                        raw.name, signature.name
                    ),
                ));
            }
            table.method_headers.insert(*line, id);
            // The function index is filled in once the method is compiled.
            methods.push((signature.name.clone(), usize::MAX));
            signatures.insert(signature.name.clone(), signature);
        }
        table.methods.insert(id, signatures);
        table.types.push(RecordType {
            name: full_name,
            fields,
            methods,
            is_class: raw.is_class,
            line: raw.line,
        });
    }
    Ok(table)
}

/// Like `parse_signature` for a method: prepends the implicit `SELF`
/// parameter. `class_name` is the class's full (module-prefixed) name, so
/// the method is qualified `Class.Method`.
fn method_signature(
    tokens: &[Token],
    line: usize,
    ids: &HashMap<String, usize>,
    class_name: &str,
    class: usize,
) -> Result<(Signature, usize), Error> {
    let (mut signature, used) = parse_signature(tokens, line, ids, class_name)?;
    if signature.name == "SELF" || signature.parameters.iter().any(|name| name == "SELF") {
        return Err(Error::new(
            line,
            "SELF is reserved inside methods; it cannot name a method or parameter",
        ));
    }
    if signature.name == "NEW" && signature.types.contains_key("NEW") {
        return Err(Error::new(
            line,
            "a constructor (FUNCTION NEW) cannot declare a result type",
        ));
    }
    signature.parameters.insert(0, "SELF".to_string());
    signature.infos.insert(
        0,
        ParamInfo {
            by_ref: false,
            optional: false,
            default: None,
        },
    );
    signature
        .types
        .insert("SELF".to_string(), VarType::Record(class));
    Ok((signature, used))
}

/// `name AS type [= default]`; also returns a global constant used as default.
fn parse_field(
    line: usize,
    tokens: &[Token],
    ids: &HashMap<String, usize>,
    kind: &str,
) -> Result<(RecordField, Option<String>), Error> {
    let shape = format!("expected `field AS type [= default]` inside {kind}");
    let [Token::Word(name), Token::Word(as_word), type_tokens @ ..] = tokens else {
        return Err(Error::new(line, shape));
    };
    if as_word != "AS" {
        return Err(Error::new(line, shape));
    }
    let (ty, used) = type_at(type_tokens, line, ids)?;
    let rest = &type_tokens[used..];
    if let Some(message) = name_type_mismatch(name, ty) {
        return Err(Error::new(line, message));
    }
    let mut constant = None;
    let default = match rest {
        [] => None,
        [Token::Equal, value @ ..] => {
            let default = match value {
                [Token::String(text)] => FieldDefault::Text(text.clone()),
                [Token::Number(n)] => FieldDefault::Number(*n),
                [Token::Int(n)] => FieldDefault::Int(*n),
                [Token::Minus, Token::Number(n)] => FieldDefault::Number(-*n + 0.0),
                [Token::Minus, Token::Int(n)] => FieldDefault::Int(-*n),
                [Token::Minus, Token::MinInt] => FieldDefault::Int(i64::MIN),
                [Token::Word(word)] if word == "TRUE" => FieldDefault::Number(1.0),
                [Token::Word(word)] if word == "FALSE" => FieldDefault::Number(0.0),
                [Token::Word(word)] if word == "NOTHING" => FieldDefault::Nothing,
                [Token::Word(word)] if !is_keyword(word) => {
                    constant = Some(word.clone());
                    FieldDefault::Constant(word.clone())
                }
                _ => {
                    return Err(Error::new(
                        line,
                        "a field default must be a literal, NOTHING or a global CONST name",
                    ));
                }
            };
            Some(default)
        }
        _ => {
            return Err(Error::new(
                line,
                format!("unexpected tokens after the field type inside {kind}"),
            ));
        }
    };
    if let Some(default) = &default
        && !default_fits(ty, default)
    {
        return Err(Error::new(
            line,
            format!("the default of field {name} does not fit its declared type"),
        ));
    }
    Ok((
        RecordField {
            name: name.clone(),
            ty,
            default,
        },
        constant,
    ))
}

/// String variables (names ending in `$`) are exactly the STRING-typed ones.
fn name_type_mismatch(name: &str, ty: VarType) -> Option<String> {
    if name.ends_with('$') && ty != VarType::String {
        Some(format!("{name} ends in $ and must be declared AS STRING"))
    } else if !name.ends_with('$') && ty == VarType::String {
        Some(format!(
            "{name} is declared AS STRING; the name must end in $"
        ))
    } else {
        None
    }
}

/// The declared shape of a function: everything a call site needs to know.
#[derive(Clone)]
struct Signature {
    name: String,
    qualified: String,
    parameters: Vec<String>,
    infos: Vec<ParamInfo>,
    types: HashMap<String, VarType>,
}

impl Signature {
    fn required(&self) -> usize {
        self.infos.iter().filter(|info| !info.optional).count()
    }
}

/// A literal parameter default: `[-] number`, a string, TRUE, FALSE or NOTHING.
fn literal_default(tokens: &[Token]) -> Option<FieldDefault> {
    Some(match tokens {
        [Token::String(text)] => FieldDefault::Text(text.clone()),
        [Token::Number(n)] => FieldDefault::Number(*n),
        [Token::Int(n)] => FieldDefault::Int(*n),
        [Token::Minus, Token::Number(n)] => FieldDefault::Number(-*n + 0.0),
        [Token::Minus, Token::Int(n)] => FieldDefault::Int(-*n),
        [Token::Minus, Token::MinInt] => FieldDefault::Int(i64::MIN),
        [Token::Word(word)] if word == "TRUE" => FieldDefault::Number(1.0),
        [Token::Word(word)] if word == "FALSE" => FieldDefault::Number(0.0),
        [Token::Word(word)] if word == "NOTHING" => FieldDefault::Nothing,
        _ => return None,
    })
}

/// Parses the tokens after `FUNCTION`:
/// `name [(PARAMETER [BYVAL|BYREF] [OPTIONAL] p [AS type] [= default], ...)] [AS type]`.
/// Returns the signature and the number of tokens used.
fn parse_signature(
    tokens: &[Token],
    line: usize,
    ids: &HashMap<String, usize>,
    prefix: &str,
) -> Result<(Signature, usize), Error> {
    let Some(Token::Word(name)) = tokens
        .first()
        .filter(|t| matches!(t, Token::Word(n) if !is_keyword(n)))
    else {
        return Err(Error::new(line, "expected variable name"));
    };
    let name = name.clone();
    let mut position = 1;
    let mut parameters: Vec<String> = Vec::new();
    let mut infos: Vec<ParamInfo> = Vec::new();
    let mut types = HashMap::new();
    if tokens.get(position) == Some(&Token::LeftParen) {
        position += 1;
        if tokens.get(position) != Some(&Token::RightParen) {
            loop {
                if tokens.get(position) != Some(&Token::Word("PARAMETER".into())) {
                    return Err(Error::new(line, "expected Word(\"PARAMETER\")"));
                }
                position += 1;
                let mut by_ref: Option<bool> = None;
                let mut optional = false;
                while let (Some(Token::Word(modifier)), Some(Token::Word(next))) =
                    (tokens.get(position), tokens.get(position + 1))
                {
                    if next == "AS" || !matches!(modifier.as_str(), "BYVAL" | "BYREF" | "OPTIONAL")
                    {
                        break;
                    }
                    if modifier == "OPTIONAL" {
                        if optional {
                            return Err(Error::new(line, "duplicate OPTIONAL modifier"));
                        }
                        optional = true;
                    } else {
                        if by_ref.is_some() {
                            return Err(Error::new(
                                line,
                                "a parameter takes only one of BYVAL and BYREF",
                            ));
                        }
                        by_ref = Some(modifier == "BYREF");
                    }
                    position += 1;
                }
                let Some(Token::Word(parameter)) = tokens
                    .get(position)
                    .filter(|t| matches!(t, Token::Word(n) if !is_keyword(n)))
                else {
                    return Err(Error::new(line, "expected variable name"));
                };
                position += 1;
                if *parameter == name || parameters.contains(parameter) {
                    return Err(Error::new(
                        line,
                        format!("duplicate parameter or result name {parameter}"),
                    ));
                }
                let mut declared = None;
                if tokens.get(position) == Some(&Token::Word("AS".into())) {
                    let (ty, used) = type_at(&tokens[position + 1..], line, ids)?;
                    position += 1 + used;
                    if let Some(message) = name_type_mismatch(parameter, ty) {
                        return Err(Error::new(line, message));
                    }
                    types.insert(parameter.clone(), ty);
                    declared = Some(ty);
                }
                let mut default = None;
                if tokens.get(position) == Some(&Token::Equal) {
                    position += 1;
                    let end = tokens[position..]
                        .iter()
                        .position(|t| matches!(t, Token::Comma | Token::RightParen))
                        .map_or(tokens.len(), |offset| position + offset);
                    let literal = literal_default(&tokens[position..end]).ok_or_else(|| {
                        Error::new(
                            line,
                            "a parameter default must be a literal, TRUE, FALSE or NOTHING",
                        )
                    })?;
                    position = end;
                    let fits = match declared {
                        Some(ty) => default_fits(ty, &literal),
                        None => {
                            parameter.ends_with('$') == matches!(literal, FieldDefault::Text(_))
                        }
                    };
                    if !fits {
                        return Err(Error::new(
                            line,
                            format!("the default of parameter {parameter} does not fit its type"),
                        ));
                    }
                    default = Some(literal);
                    optional = true;
                }
                let by_ref = by_ref == Some(true);
                if by_ref && optional {
                    return Err(Error::new(
                        line,
                        format!("BYREF parameter {parameter} cannot be OPTIONAL or have a default"),
                    ));
                }
                if !optional && infos.iter().any(|info| info.optional) {
                    return Err(Error::new(
                        line,
                        format!("required parameter {parameter} cannot follow an optional one"),
                    ));
                }
                parameters.push(parameter.clone());
                infos.push(ParamInfo {
                    by_ref,
                    optional,
                    default,
                });
                if tokens.get(position) != Some(&Token::Comma) {
                    break;
                }
                position += 1;
            }
        }
        if tokens.get(position) != Some(&Token::RightParen) {
            return Err(Error::new(line, "expected RightParen"));
        }
        position += 1;
    }
    if tokens.get(position) == Some(&Token::Word("AS".into())) {
        let (ty, used) = type_at(&tokens[position + 1..], line, ids)?;
        position += 1 + used;
        if let Some(message) = name_type_mismatch(&name, ty) {
            return Err(Error::new(line, message));
        }
        types.insert(name.clone(), ty);
    }
    let qualified = if prefix.is_empty() {
        name.clone()
    } else {
        format!("{prefix}.{name}")
    };
    Ok((
        Signature {
            name,
            qualified,
            parameters,
            infos,
            types,
        },
        position,
    ))
}

/// Signatures of every parsable `FUNCTION` header, gathered before the main
/// pass so calls to later (and forward-declared) functions are fully known.
fn scan_signatures(
    source: &str,
    type_blocks: &HashMap<usize, usize>,
    ids: &HashMap<String, usize>,
    prefix: &str,
) -> HashMap<String, Signature> {
    let mut signatures = HashMap::new();
    let mut skip_until = 0;
    for (index, text) in source.lines().enumerate() {
        let line = index + 1;
        if line <= skip_until {
            continue;
        }
        if let Some(end) = type_blocks.get(&line) {
            skip_until = *end;
            continue;
        }
        let Ok(tokens) = lex(text, line) else {
            continue;
        };
        let [Token::Word(keyword), rest @ ..] = tokens.as_slice() else {
            continue;
        };
        if keyword != "FUNCTION" || !matches!(rest.first(), Some(Token::Word(_))) {
            continue;
        }
        if let Ok((signature, _)) = parse_signature(rest, line, ids, prefix) {
            signatures
                .entry(signature.name.clone())
                .or_insert(signature);
        }
    }
    signatures
}

/// One `IMPORT "path" [AS alias]` line.
struct Import {
    line: usize,
    path: String,
    alias: String,
    /// The alias as written (or the file stem), for display in test names.
    spelled: String,
}

fn scan_imports(source: &str) -> Result<Vec<Import>, Error> {
    let mut imports = Vec::new();
    let mut in_function = false;
    for (index, text) in source.lines().enumerate() {
        let line = index + 1;
        let Ok(tokens) = lex(text, line) else {
            continue;
        };
        match tokens.as_slice() {
            [Token::Word(word), Token::Word(_), ..] if word == "FUNCTION" || word == "TEST" => {
                in_function = true;
            }
            [Token::Word(word)] if word == "FEND" => in_function = false,
            [Token::Word(end), Token::Word(test)] if end == "END" && test == "TEST" => {
                in_function = false;
            }
            [Token::Word(word), Token::String(path), rest @ ..] if word == "IMPORT" => {
                if in_function {
                    // The main pass reports IMPORT inside a function.
                    continue;
                }
                let alias = match rest {
                    [] => default_alias(path, line)?,
                    [Token::Word(as_word), Token::Word(alias)] if as_word == "AS" => {
                        if is_keyword(alias)
                            || alias.ends_with('$')
                            || VarType::parse(alias).is_some()
                        {
                            return Err(Error::new(line, format!("invalid module alias {alias}")));
                        }
                        alias.clone()
                    }
                    _ => {
                        return Err(Error::new(line, "expected IMPORT \"path\" [AS alias]"));
                    }
                };
                let spelled = match rest {
                    [] => Path::new(path)
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or("")
                        .to_string(),
                    _ => spelled_after(text, "AS").unwrap_or_else(|| alias.clone()),
                };
                imports.push(Import {
                    line,
                    path: path.clone(),
                    alias,
                    spelled,
                });
            }
            _ => {}
        }
    }
    Ok(imports)
}

/// The upper-cased file stem, when it is a valid identifier.
fn default_alias(path: &str, line: usize) -> Result<String, Error> {
    let stem = Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("")
        .to_ascii_uppercase();
    let valid = stem
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        && stem
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        && !is_keyword(&stem)
        && VarType::parse(&stem).is_none();
    if valid {
        Ok(stem)
    } else {
        Err(Error::new(
            line,
            format!("cannot derive a module alias from \"{path}\"; use IMPORT \"{path}\" AS name"),
        ))
    }
}

/// What an importing module can see of a compiled module.
struct Exports {
    /// Name prefix of the module's globals, functions and types.
    prefix: String,
    functions: HashMap<String, Signature>,
    /// Global variables and constants (unqualified names).
    members: HashSet<String>,
    constants: HashSet<String>,
    /// The module's own record types (unqualified name -> type id).
    types: HashMap<String, usize>,
    var_types: HashMap<String, VarType>,
}

struct PendingCall {
    index: usize,
    /// The qualified function name for user calls, the builtin name otherwise.
    name: String,
    user: bool,
    /// Set for a statically resolved method call: class type id and method name.
    method: Option<(usize, String)>,
    arguments: usize,
    line: usize,
    file: usize,
}

/// Program-wide compilation state shared by all modules.
#[derive(Default)]
struct Linker {
    instructions: Vec<Instruction>,
    functions: Vec<Function>,
    types: Vec<RecordType>,
    /// Method signatures by class type id, for every module compiled so far.
    methods: HashMap<usize, HashMap<String, Signature>>,
    calls: Vec<PendingCall>,
    /// Display paths per file id; 0 is the main program.
    files: Vec<String>,
    /// Loaded modules by canonical path (each file loads once).
    modules: HashMap<PathBuf, Rc<Exports>>,
    /// Canonical path and display path of the modules being loaded.
    loading: Vec<(PathBuf, String)>,
    prefixes: HashSet<String>,
    /// `TEST` blocks of every module compiled so far.
    tests: Vec<TestCase>,
    /// Where the main program's own code starts (after the imported modules).
    main_start: usize,
    /// Instruction ranges of the main program's top-level `CONST`/`DIM`.
    prelude: Vec<std::ops::Range<usize>>,
}

/// A `TEST` block recorded by the parser of one module.
struct RawTest {
    /// Upper-cased name, for duplicate detection.
    key: String,
    /// The name as written.
    name: String,
    line: usize,
    function: usize,
}

/// Renames the variable names of an instruction; calls pass their BYREF
/// variable places too.
fn rename_op(op: &mut Op, rename: &mut dyn FnMut(&mut String)) {
    match op {
        Op::Load(name) | Op::Store(name) | Op::Input(name) | Op::FileInput(name) => rename(name),
        Op::Declare { name, .. } => rename(name),
        Op::ForStart { variable, .. } => rename(variable),
        Op::EachStart {
            variable,
            value_variable,
            ..
        } => {
            rename(variable);
            if let Some(second) = value_variable {
                rename(second);
            }
        }
        Op::CatchStore(Some(name)) => rename(name),
        Op::Call {
            binding: Some(binding),
            ..
        } => {
            for reference in &mut Arc::make_mut(binding).refs {
                if let Place::Variable(name) = &mut reference.place {
                    rename(name);
                }
            }
        }
        _ => {}
    }
}

/// Prefixes the global names (those not in `locals`) in `instructions`.
fn qualify_globals(instructions: &mut [Instruction], prefix: &str, locals: &HashSet<String>) {
    for instruction in instructions {
        rename_op(&mut instruction.op, &mut |name| {
            if !name.contains('.') && !locals.contains(name.as_str()) {
                name.insert_str(0, &format!("{prefix}."));
            }
        });
    }
}

/// Compiles a program from source text. `IMPORT` paths in a string source
/// resolve relative to the process's current directory.
pub fn compile(source: &str) -> Result<Program, Error> {
    compile_unit(source, None)
}

/// Compiles `source`, which was read from `path`: imports resolve relative
/// to that file's directory.
pub fn compile_from(source: &str, path: &Path) -> Result<Program, Error> {
    compile_unit(source, Some(path))
}

/// Reads and compiles the program in `path`.
pub fn compile_file(path: &Path) -> Result<Program, Error> {
    let source = fs::read_to_string(path)
        .map_err(|e| Error::new(0, format!("cannot read {}: {e}", path.display())))?;
    compile_unit(&source, Some(path))
}

fn compile_unit(source: &str, path: Option<&Path>) -> Result<Program, Error> {
    let mut linker = Linker {
        files: vec![String::new()],
        ..Linker::default()
    };
    let directory = path
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_default();
    if let Some(path) = path
        && let Ok(canonical) = fs::canonicalize(path)
    {
        linker.loading.push((canonical, display_path(path)));
    }
    linker.compile_module(source, &directory, "", "", 0)?;
    linker.finish(source.lines().count().max(1))
}

/// Formats a path for diagnostics using the platform's native separator
/// only, so `dir\sub/mod.bas` never mixes styles.
fn display_path(path: &Path) -> String {
    let text = path.display().to_string();
    if std::path::MAIN_SEPARATOR == '\\' {
        text.replace('/', "\\")
    } else {
        text
    }
}

impl Linker {
    fn error(&self, file: usize, line: usize, message: impl Into<String>) -> Error {
        let mut error = Error::new(line, message);
        if file != 0 {
            error.file = self.files.get(file).cloned();
        }
        error
    }

    /// Loads (or reuses) an imported module.
    fn load_import(&mut self, import: &Import, directory: &Path) -> Result<Rc<Exports>, Error> {
        let fail = |detail: String| {
            Error::new(
                import.line,
                format!("cannot import \"{}\": {detail}", import.path),
            )
        };
        let resolved = directory.join(&import.path);
        let canonical = fs::canonicalize(&resolved).map_err(|e| fail(e.to_string()))?;
        if let Some(found) = self.modules.get(&canonical) {
            return Ok(Rc::clone(found));
        }
        let display = display_path(&resolved);
        if let Some(position) = self.loading.iter().position(|(path, _)| *path == canonical) {
            let mut chain: Vec<String> = self.loading[position..]
                .iter()
                .map(|(_, name)| name.clone())
                .collect();
            chain.push(display);
            return Err(Error::new(
                import.line,
                format!("import cycle: {}", chain.join(" -> ")),
            ));
        }
        let text = fs::read_to_string(&canonical).map_err(|e| fail(e.to_string()))?;
        let mut prefix = import.alias.clone();
        let mut copies = 1;
        while !self.prefixes.insert(prefix.clone()) {
            copies += 1;
            prefix = format!("{}#{copies}", import.alias);
        }
        let file = self.files.len();
        // The alias as written, with the `#n` suffix of repeated imports.
        let shown = format!("{}{}", import.spelled, &prefix[import.alias.len()..]);
        self.files.push(display.clone());
        self.loading.push((canonical.clone(), display.clone()));
        let module_directory = resolved.parent().map(Path::to_path_buf).unwrap_or_default();
        let result = self.compile_module(&text, &module_directory, &prefix, &shown, file);
        self.loading.pop();
        let exports = Rc::new(result.map_err(|mut error| {
            error.file.get_or_insert(display);
            error
        })?);
        self.modules.insert(canonical, Rc::clone(&exports));
        Ok(exports)
    }

    /// Resolves the pending calls and appends the final `Halt`.
    fn finish(mut self, last_line: usize) -> Result<Program, Error> {
        self.instructions.push(Instruction {
            op: Op::Halt,
            line: last_line,
            file: 0,
        });
        let calls = std::mem::take(&mut self.calls);
        for call in calls {
            let PendingCall {
                index,
                name,
                user,
                method,
                arguments,
                line,
                file,
            } = call;
            let function = if let Some((class, method)) = method {
                self.types[class]
                    .methods
                    .iter()
                    .find(|(name, _)| *name == method)
                    .map(|(_, function)| *function)
            } else if user {
                self.functions
                    .iter()
                    .position(|function| function.class.is_none() && function.qualified == name)
            } else {
                None
            };
            let Some(function) = function else {
                // Not a user function: fall back to the standard library.
                let builtin = Builtin::function(&name)
                    .filter(|_| !user)
                    .ok_or_else(|| self.error(file, line, format!("unknown function {name}")))?;
                let (minimum, maximum) = builtin.arity();
                if arguments < minimum || arguments > maximum {
                    return Err(self.error(
                        file,
                        line,
                        format!(
                            "built-in {name} requires {} arguments, got {arguments}",
                            arity_text(minimum, maximum)
                        ),
                    ));
                }
                let Op::Call { discard_result, .. } = self.instructions[index].op else {
                    return Err(self.error(file, line, "invalid function call instruction"));
                };
                self.instructions[index].op = Op::Builtin {
                    builtin,
                    arguments,
                    discard_result,
                };
                continue;
            };
            if let Op::OnEvent {
                function: target, ..
            } = &mut self.instructions[index].op
            {
                *target = Some(function);
                continue;
            }
            let Op::Call {
                function: target, ..
            } = &mut self.instructions[index].op
            else {
                return Err(self.error(file, line, "invalid function call instruction"));
            };
            *target = function;
        }
        self.tests.sort_by_key(|test| test.file);
        Ok(Program {
            instructions: self.instructions,
            functions: self.functions,
            types: self.types,
            files: self.files,
            tests: self.tests,
            main_start: self.main_start,
            prelude: self.prelude,
        })
    }

    fn compile_module(
        &mut self,
        source: &str,
        directory: &Path,
        prefix: &str,
        shown_prefix: &str,
        file: usize,
    ) -> Result<Exports, Error> {
        let mut aliases: HashMap<String, Rc<Exports>> = HashMap::new();
        let mut imported_types = HashMap::new();
        for import in scan_imports(source)? {
            if aliases.contains_key(&import.alias) {
                return Err(Error::new(
                    import.line,
                    format!("duplicate module alias {}", import.alias),
                ));
            }
            let exports = self.load_import(&import, directory)?;
            for (name, id) in &exports.types {
                imported_types.insert(format!("{}.{name}", import.alias), *id);
            }
            aliases.insert(import.alias, exports);
        }
        let module_start = (self.instructions.len(), self.functions.len());
        if file == 0 {
            self.main_start = module_start.0;
        }
        let table = collect_types(source, self.types.len(), prefix, imported_types)?;
        let TypeTable {
            types: new_types,
            ids,
            blocks: type_blocks,
            constants: field_constants,
            class_blocks,
            skips: class_skips,
            method_headers,
            methods: new_methods,
        } = table;
        let type_start = self.types.len();
        self.types.extend(new_types);
        self.methods.extend(new_methods);
        let mut declaration_blocks = type_blocks.clone();
        declaration_blocks.extend(class_blocks.iter().map(|(start, end)| (*start, *end)));
        let signatures = scan_signatures(source, &declaration_blocks, &ids, prefix);
        let mut skip_until = 0;
        let mut labels = HashMap::new();
        let mut jumps = Vec::new();
        let mut blocks = Vec::new();
        let mut current_function = None;
        let mut global_constants = HashSet::new();
        let mut try_labels = HashSet::new();
        let mut declarations = Declarations::default();
        let mut function_ranges: Vec<(usize, usize)> = Vec::new();
        let mut raw_tests: Vec<RawTest> = Vec::new();
        for (index, source_line) in source.lines().enumerate() {
            let line = index + 1;
            if line <= skip_until {
                continue;
            }
            if let Some(end) = type_blocks.get(&line) {
                if current_function.is_some() || !blocks.is_empty() {
                    return Err(Error::new(
                        line,
                        "TYPE declarations must be at the top level, outside functions and blocks",
                    ));
                }
                skip_until = *end;
                continue;
            }
            if class_blocks.contains_key(&line)
                && (current_function.is_some() || !blocks.is_empty())
            {
                return Err(Error::new(
                    line,
                    "CLASS declarations must be at the top level, outside functions and blocks",
                ));
            }
            if class_skips.contains(&line) {
                continue;
            }
            let mut tokens = lex(source_line, line)?;
            if matches!(tokens.first(), Some(Token::Number(_))) {
                let label = line_number(&tokens.remove(0), line)?;
                let scope = current_function
                    .as_ref()
                    .map(|context: &FunctionContext| context.index);
                if labels
                    .insert((scope, label), self.instructions.len())
                    .is_some()
                {
                    return Err(Error::new(line, format!("duplicate line number {label}")));
                }
                if blocks
                    .iter()
                    .any(|block| matches!(block, Block::Try { .. }))
                {
                    try_labels.insert((scope, label));
                }
            }
            // Top-level CONST/DIM of the main program are re-run before each
            // of its tests (see `Program::prelude`).
            let starts_prelude = file == 0
                && current_function.is_none()
                && blocks.is_empty()
                && match tokens.as_slice() {
                    [Token::Word(word), ..] if word == "CONST" => true,
                    [Token::Word(word), next, ..] if word == "DIM" => {
                        !matches!(next, Token::Equal | Token::LeftBracket | Token::Dot)
                    }
                    _ => false,
                };
            let before = self.instructions.len();
            let mut parser = Parser {
                tokens: &tokens,
                text: source_line,
                tests: &mut raw_tests,
                position: 0,
                line,
                file,
                instructions: &mut self.instructions,
                jumps: &mut jumps,
                blocks: &mut blocks,
                functions: &mut self.functions,
                current_function: &mut current_function,
                calls: &mut self.calls,
                global_constants: &mut global_constants,
                declarations: &mut declarations,
                types: &self.types,
                methods: &self.methods,
                class: method_headers.get(&line).copied(),
                statement_call: false,
                last_method_call: None,
                type_ids: &ids,
                prefix,
                aliases: &aliases,
                signatures: &signatures,
                function_ranges: &mut function_ranges,
                module_start,
            };
            if !tokens.is_empty() {
                parser.statement(true)?;
                if parser.peek().is_some() {
                    return Err(Error::new(line, "unexpected tokens after statement"));
                }
            }
            if starts_prelude {
                self.prelude.push(before..self.instructions.len());
            }
        }
        if let Some(block) = blocks.last() {
            return Err(Error::new(block.line(), block.unterminated()));
        }
        if let Some(context) = current_function {
            return Err(Error::new(
                context.line,
                if context.test {
                    "unterminated TEST; expected END TEST"
                } else {
                    "unterminated FUNCTION; expected FEND"
                },
            ));
        }
        for test in raw_tests {
            self.tests.push(TestCase {
                name: test.name,
                module: shown_prefix.to_string(),
                file,
                line: test.line,
                function: test.function,
            });
        }
        for (index, label, line, scope) in jumps {
            let target = labels.get(&(scope, label)).copied().ok_or_else(|| {
                Error::new(
                    line,
                    format!("unknown line number {label} in current scope"),
                )
            })?;
            if try_labels.contains(&(scope, label)) {
                return Err(Error::new(
                    line,
                    format!("cannot jump to line number {label}: it is inside a TRY block"),
                ));
            }
            self.instructions[index].op = Op::Jump(target);
        }
        for (name, line) in &field_constants {
            if !global_constants.contains(name) {
                return Err(Error::new(
                    *line,
                    format!("field default {name} is not a global CONST"),
                ));
            }
        }
        for id in type_start..self.types.len() {
            for entry in 0..self.types[id].methods.len() {
                let name = self.types[id].methods[entry].0.clone();
                let function = self.functions[module_start.1..]
                    .iter()
                    .position(|function| function.class == Some(id) && function.name == name)
                    .map(|position| position + module_start.1)
                    .ok_or_else(|| {
                        Error::new(
                            self.types[id].line,
                            format!("internal error: method {name} was not compiled"),
                        )
                    })?;
                self.types[id].methods[entry].1 = function;
            }
        }
        let mut members: HashSet<String> = global_constants.clone();
        members.extend(declarations.globals.keys().cloned());
        if !prefix.is_empty() {
            // Function bodies were qualified at FEND; qualify the top-level
            // code (everything outside function bodies) and the constant
            // defaults of this module's record fields.
            let mut in_function = vec![false; self.instructions.len()];
            for (start, end) in &function_ranges {
                in_function[*start..=*end].fill(true);
            }
            let none = HashSet::new();
            for index in module_start.0..self.instructions.len() {
                if !in_function[index] {
                    qualify_globals(&mut self.instructions[index..=index], prefix, &none);
                }
            }
            for record in &mut self.types[type_start..] {
                for field in &mut record.fields {
                    if let Some(FieldDefault::Constant(name)) = &mut field.default {
                        name.insert_str(0, &format!("{prefix}."));
                    }
                }
            }
            let dotted = format!("{prefix}.");
            for instruction in &mut self.instructions[module_start.0..] {
                rename_op(&mut instruction.op, &mut |name| {
                    if let Some(member) = name.strip_prefix(&dotted) {
                        members.insert(member.to_string());
                    }
                });
            }
        }
        Ok(Exports {
            prefix: prefix.to_string(),
            functions: signatures,
            members,
            constants: global_constants,
            types: ids
                .into_iter()
                .filter(|(name, id)| !name.contains('.') && *id >= type_start)
                .collect(),
            var_types: declarations.types,
        })
    }
}

fn arity_text(minimum: usize, maximum: usize) -> String {
    if maximum == usize::MAX {
        format!("at least {minimum}")
    } else if minimum == maximum {
        minimum.to_string()
    } else {
        format!("{minimum} to {maximum}")
    }
}

fn line_number(token: &Token, line: usize) -> Result<u32, Error> {
    match token {
        Token::Number(n) if *n >= 1.0 && *n <= u32::MAX as f64 && n.fract() == 0.0 => Ok(*n as u32),
        _ => Err(Error::new(
            line,
            "line number must be an integer from 1 to 4294967295",
        )),
    }
}

struct Parser<'a> {
    tokens: &'a [Token],
    /// The source text of the line, for the original spelling of names.
    text: &'a str,
    /// `TEST` blocks of the module being compiled.
    tests: &'a mut Vec<RawTest>,
    position: usize,
    line: usize,
    /// File id of the module being compiled (0 is the main program).
    file: usize,
    instructions: &'a mut Vec<Instruction>,
    jumps: &'a mut Vec<(usize, u32, usize, Option<usize>)>,
    blocks: &'a mut Vec<Block>,
    functions: &'a mut Vec<Function>,
    current_function: &'a mut Option<FunctionContext>,
    calls: &'a mut Vec<PendingCall>,
    global_constants: &'a mut HashSet<String>,
    declarations: &'a mut Declarations,
    types: &'a [RecordType],
    /// Method signatures by class type id.
    methods: &'a HashMap<usize, HashMap<String, Signature>>,
    /// Set on the `FUNCTION` header line of a method: the class's type id.
    class: Option<usize>,
    /// Compiling the target of a `CALL` statement (a method call needs no parentheses).
    statement_call: bool,
    /// Instruction index of the most recent method call.
    last_method_call: Option<usize>,
    type_ids: &'a HashMap<String, usize>,
    /// Name prefix of this module (empty for the main program).
    prefix: &'a str,
    aliases: &'a HashMap<String, Rc<Exports>>,
    signatures: &'a HashMap<String, Signature>,
    /// Instruction ranges (inclusive) of this module's function bodies.
    function_ranges: &'a mut Vec<(usize, usize)>,
    /// Instruction and function counts when this module started.
    module_start: (usize, usize),
}

/// Compile-time declaration state of the main program.
#[derive(Default)]
struct Declarations {
    /// `OPTION EXPLICIT` is active.
    explicit: bool,
    /// Declared global variables, with the element type of `DIM` arrays.
    globals: HashMap<String, Option<VarType>>,
    /// Declared types of global variables.
    types: HashMap<String, VarType>,
}

struct FunctionContext {
    index: usize,
    /// A hidden `TEST` function rather than a user `FUNCTION`.
    test: bool,
    skip: usize,
    line: usize,
    exits: Vec<usize>,
    constants: HashSet<String>,
    /// Element types of `DIM` arrays declared in this function.
    fills: HashMap<String, VarType>,
}

enum Target {
    Variable,
    Index,
    Field(String),
    /// The chain ends in a method call (already emitted).
    Call,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LoopKind {
    For,
    While,
    Do,
}

impl LoopKind {
    fn name(self) -> &'static str {
        match self {
            Self::For => "FOR",
            Self::While => "WHILE",
            Self::Do => "DO",
        }
    }
}

/// Pending EXIT/CONTINUE jumps of one loop, patched when the loop closes.
#[derive(Default)]
struct LoopJumps {
    exits: Vec<usize>,
    continues: Vec<usize>,
}

enum Block {
    While {
        start: usize,
        branch: usize,
        line: usize,
        jumps: LoopJumps,
    },
    For {
        start: usize,
        variable: String,
        line: usize,
        jumps: LoopJumps,
    },
    Do {
        start: usize,
        has_condition: bool,
        line: usize,
        jumps: LoopJumps,
    },
    If {
        pending: Option<usize>,
        end_jumps: Vec<usize>,
        seen_else: bool,
        line: usize,
    },
    Select {
        next: Option<usize>,
        end_jumps: Vec<usize>,
        seen_case: bool,
        seen_else: bool,
        line: usize,
    },
    Try {
        begin: usize,
        phase: TryPhase,
        has_catch: bool,
        has_finally: bool,
        /// `Leave` instructions that end a section; patched to the TRY's end.
        leaves: Vec<usize>,
        line: usize,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TryPhase {
    Body,
    Catch,
    Finally,
}

impl Block {
    fn line(&self) -> usize {
        match self {
            Self::While { line, .. }
            | Self::For { line, .. }
            | Self::Do { line, .. }
            | Self::If { line, .. }
            | Self::Select { line, .. }
            | Self::Try { line, .. } => *line,
        }
    }

    fn unterminated(&self) -> &'static str {
        match self {
            Self::While { .. } | Self::For { .. } | Self::Do { .. } => "unterminated loop",
            Self::If { .. } => "unterminated block IF; expected END IF",
            Self::Select { .. } => "unterminated SELECT CASE; expected END SELECT",
            Self::Try { .. } => "unterminated TRY; expected END TRY",
        }
    }

    fn kind(&self) -> Option<LoopKind> {
        match self {
            Self::While { .. } => Some(LoopKind::While),
            Self::For { .. } => Some(LoopKind::For),
            Self::Do { .. } => Some(LoopKind::Do),
            _ => None,
        }
    }

    fn jumps_mut(&mut self) -> Option<&mut LoopJumps> {
        match self {
            Self::While { jumps, .. } | Self::For { jumps, .. } | Self::Do { jumps, .. } => {
                Some(jumps)
            }
            _ => None,
        }
    }
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn next(&mut self) -> Option<Token> {
        let result = self.peek().cloned();
        self.position += usize::from(result.is_some());
        result
    }

    fn emit(&mut self, op: Op) -> usize {
        let index = self.instructions.len();
        self.instructions.push(Instruction {
            op,
            line: self.line,
            file: self.file,
        });
        index
    }

    fn expect(&mut self, expected: Token) -> Result<(), Error> {
        if self.next() == Some(expected.clone()) {
            Ok(())
        } else {
            Err(Error::new(self.line, format!("expected {expected:?}")))
        }
    }

    fn variable(&mut self) -> Result<String, Error> {
        match self.next() {
            Some(Token::Word(name)) if !is_keyword(&name) => Ok(name),
            _ => Err(Error::new(self.line, "expected variable name")),
        }
    }

    /// Parses an optional `AS type` (the type may be `Alias.Name`).
    fn optional_as(&mut self) -> Result<Option<VarType>, Error> {
        if self.peek() != Some(&Token::Word("AS".into())) {
            return Ok(None);
        }
        self.next();
        let (ty, used) = type_at(&self.tokens[self.position..], self.line, self.type_ids)?;
        self.position += used;
        Ok(Some(ty))
    }

    /// String variables (names ending in `$`) are exactly the STRING-typed ones.
    fn check_type_name(&self, name: &str, ty: VarType) -> Result<(), Error> {
        match name_type_mismatch(name, ty) {
            Some(message) => Err(Error::new(self.line, message)),
            None => Ok(()),
        }
    }

    /// Parses an optional `= expression`; true when one was emitted.
    fn initializer(&mut self) -> Result<bool, Error> {
        if self.peek() != Some(&Token::Equal) {
            return Ok(false);
        }
        self.next();
        self.expression(0)?;
        Ok(true)
    }

    fn is_declared(&self, name: &str) -> bool {
        if !self.declarations.explicit
            || self.global_constants.contains(name)
            || self.declarations.globals.contains_key(name)
        {
            return true;
        }
        self.current_function.as_ref().is_some_and(|context| {
            let function = &self.functions[context.index];
            function.name == name
                || function
                    .parameters
                    .iter()
                    .any(|parameter| parameter == name)
                || function.locals.iter().any(|local| local == name)
        })
    }

    /// Under `OPTION EXPLICIT`, a variable must have been declared earlier.
    fn require_declared(&self, name: &str) -> Result<(), Error> {
        if self.is_declared(name) {
            Ok(())
        } else {
            Err(Error::new(
                self.line,
                format!("undeclared variable {name} (OPTION EXPLICIT is active)"),
            ))
        }
    }

    /// FOR, FOR EACH, INPUT and CATCH variables declare themselves under
    /// `OPTION EXPLICIT`: as function locals inside a function, else globals.
    fn implicit_declare(&mut self, name: &str) {
        if self.is_declared(name) {
            return;
        }
        match self.current_function.as_ref().map(|context| context.index) {
            Some(index) => self.functions[index].locals.push(name.to_string()),
            None => {
                self.declarations.globals.insert(name.to_string(), None);
            }
        }
    }

    /// Records a typed declaration: a function local, or a main-program global.
    fn declare_variable(
        &mut self,
        name: &str,
        ty: VarType,
        fill: Option<VarType>,
    ) -> Result<(), Error> {
        self.check_assignable(name)?;
        if let Some(context) = self.current_function.as_mut() {
            let function = &mut self.functions[context.index];
            if name == function.name
                || function
                    .parameters
                    .iter()
                    .any(|parameter| parameter == name)
                || function.locals.iter().any(|local| local == name)
            {
                return Err(Error::new(
                    self.line,
                    format!("duplicate local or parameter {name}"),
                ));
            }
            function.locals.push(name.to_string());
            function.types.insert(name.to_string(), ty);
            if let Some(fill) = fill {
                context.fills.insert(name.to_string(), fill);
            }
        } else {
            if self.declarations.globals.contains_key(name) {
                return Err(Error::new(
                    self.line,
                    format!("variable {name} is already declared"),
                ));
            }
            self.declarations.globals.insert(name.to_string(), fill);
            self.declarations.types.insert(name.to_string(), ty);
        }
        Ok(())
    }

    /// The record type a variable was declared with, when known statically.
    fn static_record_type(&self, name: &str) -> Option<usize> {
        let declared = match self.current_function.as_ref() {
            Some(context) => {
                let function = &self.functions[context.index];
                if name == function.name
                    || function
                        .parameters
                        .iter()
                        .any(|parameter| parameter == name)
                    || function.locals.iter().any(|local| local == name)
                {
                    function.types.get(name)
                } else {
                    self.declarations.types.get(name)
                }
            }
            None => self.declarations.types.get(name),
        };
        match declared {
            Some(VarType::Record(id)) => Some(*id),
            _ => None,
        }
    }

    /// Reads a field name after `.` and, when the record type is known,
    /// checks the field exists; returns the field's record type, if any.
    fn field_access(&mut self, record: Option<usize>) -> Result<(String, Option<usize>), Error> {
        let Some(Token::Word(field)) = self.next() else {
            return Err(Error::new(self.line, "expected a field name after '.'"));
        };
        let Some(id) = record else {
            return Ok((field, None));
        };
        let definition = &self.types[id];
        let Some(found) = definition.fields.iter().find(|f| f.name == field) else {
            return Err(Error::new(
                self.line,
                format!("record type {} has no field {field}", definition.name),
            ));
        };
        let next = match found.ty {
            VarType::Record(next) => Some(next),
            _ => None,
        };
        Ok((field, next))
    }

    /// `(n, m, ...)` of DIM/REDIM; returns the number of dimensions.
    fn dimensions(&mut self) -> Result<usize, Error> {
        self.expect(Token::LeftParen)?;
        let mut count = 0;
        loop {
            self.expression(0)?;
            count += 1;
            if self.peek() != Some(&Token::Comma) {
                break;
            }
            self.next();
        }
        self.expect(Token::RightParen)?;
        if count > MAX_DIMENSIONS {
            return Err(Error::new(
                self.line,
                format!("arrays have at most {MAX_DIMENSIONS} dimensions"),
            ));
        }
        Ok(count)
    }

    fn element_type(&self, ty: Option<VarType>) -> Result<VarType, Error> {
        match ty {
            None => Ok(VarType::Variant),
            Some(
                ty @ (VarType::Integer
                | VarType::Double
                | VarType::Boolean
                | VarType::String
                | VarType::Variant
                | VarType::Record(_)),
            ) => Ok(ty),
            Some(_) => Err(Error::new(
                self.line,
                "array element type must be INTEGER, DOUBLE, BOOLEAN, STRING, VARIANT or a TYPE name",
            )),
        }
    }

    /// `DIM name [AS type] [= value]` or `DIM name(n, m, ...) [AS element-type]`.
    fn dim_statement(&mut self) -> Result<(), Error> {
        let name = self.variable()?;
        if self.peek() == Some(&Token::LeftParen) {
            if name.ends_with('$') {
                return Err(Error::new(
                    self.line,
                    "array variable names must not end in $",
                ));
            }
            let dims = self.dimensions()?;
            let fill = self.optional_as()?;
            let fill = self.element_type(fill)?;
            self.declare_variable(&name, VarType::Array, Some(fill))?;
            self.emit(Op::DimArray { dims, fill });
            self.emit(Op::Declare {
                name,
                ty: VarType::Array,
                has_init: true,
            });
            return Ok(());
        }
        let ty = match self.optional_as()? {
            Some(ty) => ty,
            None if name.ends_with('$') => VarType::String,
            None => VarType::Variant,
        };
        self.check_type_name(&name, ty)?;
        let has_init = self.initializer()?;
        let has_init = self.default_instance(ty, has_init)?;
        self.declare_variable(&name, ty, None)?;
        self.emit(Op::Declare { name, ty, has_init });
        Ok(())
    }

    /// `REDIM [PRESERVE] name(n, m, ...) [AS element-type]`.
    fn redim_statement(&mut self) -> Result<(), Error> {
        let preserve = self.peek() == Some(&Token::Word("PRESERVE".into()))
            && matches!(self.tokens.get(self.position + 1), Some(Token::Word(_)));
        if preserve {
            self.next();
        }
        let name = self.variable()?;
        self.require_declared(&name)?;
        self.emit(Op::Load(name.clone()));
        let dims = self.dimensions()?;
        let known = match self.current_function.as_ref() {
            Some(context) => context.fills.get(&name).copied(),
            None => None,
        }
        .or(self.declarations.globals.get(&name).copied().flatten());
        let fill = match self.optional_as()? {
            Some(ty) => self.element_type(Some(ty))?,
            None => known.unwrap_or(VarType::Variant),
        };
        self.emit(Op::Redim {
            dims,
            preserve,
            fill,
        });
        Ok(())
    }

    fn statement(&mut self, top_level: bool) -> Result<(), Error> {
        if self.peek() == Some(&Token::ColonEqual)
            || (matches!(self.peek(), Some(Token::Word(_)))
                && self.tokens.get(self.position + 1) == Some(&Token::ColonEqual))
        {
            return Err(Error::new(
                self.line,
                "':=' is only valid in named arguments",
            ));
        }
        if !top_level
            && matches!(self.peek(), Some(Token::Word(word))
            if matches!(word.as_str(), "FUNCTION" | "FEND" | "LOCAL" | "CONST"))
        {
            return Err(Error::new(
                self.line,
                "FUNCTION, FEND, LOCAL and CONST must be on their own lines",
            ));
        }
        let ends_block = self.peek() == Some(&Token::Word("END".into()))
            && matches!(self.tokens.get(self.position + 1),
                Some(Token::Word(word)) if word == "IF" || word == "SELECT" || word == "TRY" || word == "TEST");
        let legacy_name = matches!(self.peek(), Some(Token::Word(word))
            if matches!(word.as_str(), "TRY" | "CATCH" | "FINALLY"))
            && self.assignment_follows();
        if !top_level
            && !legacy_name
            && (ends_block
                || matches!(self.peek(), Some(Token::Word(word))
            if matches!(word.as_str(),
                "FOR" | "NEXT" | "WHILE" | "WEND" | "DO" | "LOOP" | "SELECT" | "CASE"
                | "ELSEIF" | "ELSE" | "TRY" | "CATCH" | "FINALLY")))
        {
            return Err(Error::new(
                self.line,
                "block and loop delimiters must be on their own lines",
            ));
        }
        if matches!(
            self.blocks.last(),
            Some(Block::Select {
                seen_case: false,
                ..
            })
        ) && !ends_block
            && self.peek() != Some(&Token::Word("CASE".into()))
        {
            return Err(Error::new(
                self.line,
                "expected CASE or END SELECT after SELECT CASE",
            ));
        }
        match self.peek() {
            Some(Token::Word(word)) if word == "FUNCTION" => {
                self.next();
                if self.current_function.is_some() || !self.blocks.is_empty() {
                    return Err(Error::new(
                        self.line,
                        "FUNCTION declarations must be at global scope outside loops",
                    ));
                }
                let (signature, used) = match self.class {
                    Some(class) => {
                        let class_name = self.types[class].name.clone();
                        method_signature(
                            &self.tokens[self.position..],
                            self.line,
                            self.type_ids,
                            &class_name,
                            class,
                        )?
                    }
                    None => parse_signature(
                        &self.tokens[self.position..],
                        self.line,
                        self.type_ids,
                        self.prefix,
                    )?,
                };
                self.position += used;
                if self.class.is_none()
                    && self.functions.iter().any(|function| {
                        function.class.is_none() && function.qualified == signature.qualified
                    })
                {
                    return Err(Error::new(
                        self.line,
                        format!("duplicate function {}", signature.name),
                    ));
                }
                let skip = self.emit(Op::Jump(0));
                let index = self.functions.len();
                self.functions.push(Function {
                    name: signature.name,
                    qualified: signature.qualified,
                    entry: self.instructions.len(),
                    parameters: signature.parameters,
                    params: signature.infos,
                    locals: Vec::new(),
                    types: signature.types,
                    class: self.class,
                });
                *self.current_function = Some(FunctionContext {
                    index,
                    test: false,
                    skip,
                    line: self.line,
                    exits: Vec::new(),
                    constants: HashSet::new(),
                    fills: HashMap::new(),
                });
            }
            Some(Token::Word(word)) if word == "TEST" && !self.assignment_follows() => {
                self.next();
                self.test_header(top_level)?;
            }
            Some(Token::Word(word)) if word == "FEND" => {
                self.next();
                let context = self
                    .current_function
                    .as_ref()
                    .ok_or_else(|| Error::new(self.line, "FEND without FUNCTION"))?;
                if context.test {
                    return Err(Error::new(
                        self.line,
                        "FEND cannot close a TEST; use END TEST",
                    ));
                }
                if !self.blocks.is_empty() {
                    return Err(Error::new(
                        self.line,
                        "FEND cannot close an unfinished loop or block",
                    ));
                }
                self.end_function();
            }
            Some(Token::Word(word))
                if matches!(word.as_str(), "ASSERT" | "ASSERTEQ" | "ASSERTNEAR")
                    && !self.assignment_follows() =>
            {
                let keyword = word.clone();
                self.next();
                self.assert_statement(&keyword)?;
            }
            Some(Token::Word(word)) if word == "LOCAL" => {
                self.next();
                let name = self.variable()?;
                if self.current_function.is_none() {
                    return Err(Error::new(
                        self.line,
                        "LOCAL is only valid inside a function",
                    ));
                }
                if let Some(ty) = self.optional_as()? {
                    self.check_type_name(&name, ty)?;
                    let has_init = self.initializer()?;
                    let has_init = self.default_instance(ty, has_init)?;
                    self.declare_variable(&name, ty, None)?;
                    self.emit(Op::Declare { name, ty, has_init });
                    return Ok(());
                }
                if self.peek() == Some(&Token::Equal) {
                    return Err(Error::new(
                        self.line,
                        "a LOCAL initializer requires a type: LOCAL X AS type = value",
                    ));
                }
                let context = self.current_function.as_ref().expect("checked above");
                let function = &mut self.functions[context.index];
                if name == function.name
                    || function.parameters.contains(&name)
                    || function.locals.contains(&name)
                {
                    return Err(Error::new(
                        self.line,
                        format!("duplicate local or parameter {name}"),
                    ));
                }
                function.locals.push(name);
            }
            Some(Token::Word(word)) if word == "OPTION" && !self.assignment_follows() => {
                self.next();
                if self.next() != Some(Token::Word("EXPLICIT".into())) {
                    return Err(Error::new(self.line, "expected OPTION EXPLICIT"));
                }
                if !top_level
                    || self.instructions.len() != self.module_start.0
                    || self.functions.len() != self.module_start.1
                {
                    return Err(Error::new(
                        self.line,
                        "OPTION EXPLICIT must be the first statement of the program",
                    ));
                }
                self.declarations.explicit = true;
            }
            Some(Token::Word(word)) if word == "DIM" && !self.assignment_follows() => {
                self.next();
                self.dim_statement()?;
            }
            Some(Token::Word(word)) if word == "REDIM" && !self.assignment_follows() => {
                self.next();
                self.redim_statement()?;
            }
            Some(Token::Word(word)) if word == "CONST" => {
                self.next();
                if !self.blocks.is_empty() {
                    return Err(Error::new(
                        self.line,
                        "CONST must not appear inside a loop or block",
                    ));
                }
                let name = self.variable()?;
                if let Some(context) = self.current_function.as_mut() {
                    let function = &mut self.functions[context.index];
                    if name == function.name
                        || function.parameters.contains(&name)
                        || function.locals.contains(&name)
                    {
                        return Err(Error::new(
                            self.line,
                            format!("duplicate constant, local or parameter {name}"),
                        ));
                    }
                    function.locals.push(name.clone());
                    context.constants.insert(name.clone());
                } else if !self.global_constants.insert(name.clone()) {
                    return Err(Error::new(
                        self.line,
                        format!("constant {name} is already declared"),
                    ));
                }
                self.expect(Token::Equal)?;
                self.expression(0)?;
                self.emit(Op::Store(name));
            }
            Some(Token::Word(word)) if word == "EXIT" || word == "CONTINUE" => {
                let exit = self.next() == Some(Token::Word("EXIT".into()));
                let keyword = if exit { "EXIT" } else { "CONTINUE" };
                let kind = match self.next() {
                    Some(Token::Word(word)) if word == "FOR" => LoopKind::For,
                    Some(Token::Word(word)) if word == "WHILE" => LoopKind::While,
                    Some(Token::Word(word)) if word == "DO" => LoopKind::Do,
                    Some(Token::Word(word)) if exit && word == "FUNCTION" => {
                        match self.current_function.as_ref() {
                            None => {
                                return Err(Error::new(
                                    self.line,
                                    "EXIT FUNCTION is only valid inside a function",
                                ));
                            }
                            Some(context) if context.test => {
                                return Err(Error::new(
                                    self.line,
                                    "EXIT FUNCTION is not valid inside a TEST; use EXIT TEST",
                                ));
                            }
                            Some(_) => {}
                        }
                        let jump = self.emit_leave(0);
                        if let Some(context) = self.current_function.as_mut() {
                            context.exits.push(jump);
                        }
                        return Ok(());
                    }
                    Some(Token::Word(word)) if exit && word == "TEST" => {
                        if !self
                            .current_function
                            .as_ref()
                            .is_some_and(|context| context.test)
                        {
                            return Err(Error::new(
                                self.line,
                                "EXIT TEST is only valid inside a TEST",
                            ));
                        }
                        let jump = self.emit_leave(0);
                        if let Some(context) = self.current_function.as_mut() {
                            context.exits.push(jump);
                        }
                        return Ok(());
                    }
                    _ => {
                        return Err(Error::new(
                            self.line,
                            if exit {
                                "EXIT must be followed by FOR, WHILE, DO or FUNCTION"
                            } else {
                                "CONTINUE must be followed by FOR, WHILE or DO"
                            },
                        ));
                    }
                };
                let Some(position) = self
                    .blocks
                    .iter()
                    .rposition(|block| block.kind() == Some(kind))
                else {
                    return Err(Error::new(
                        self.line,
                        format!("{keyword} {name} outside a {name} loop", name = kind.name()),
                    ));
                };
                let jump = self.emit_leave(position + 1);
                if let Some(jumps) = self.blocks[position].jumps_mut() {
                    if exit {
                        jumps.exits.push(jump);
                    } else {
                        jumps.continues.push(jump);
                    }
                }
            }
            Some(Token::Word(word)) if word == "CALL" => {
                self.next();
                let name = self.variable()?;
                let qualifier = self.qualified_member(&name)?;
                let has_parens = self.peek() == Some(&Token::LeftParen);
                match qualifier {
                    Some((alias, exports, member)) => {
                        if self.peek() == Some(&Token::Dot) && exports.members.contains(&member) {
                            let record = match exports.var_types.get(&member) {
                                Some(VarType::Record(id)) => Some(*id),
                                _ => None,
                            };
                            self.call_object(format!("{}.{member}", exports.prefix), record)?
                        } else {
                            self.call_site(member, Some((alias, exports)), true, has_parens)?;
                        }
                    }
                    None if matches!(self.peek(), Some(Token::Dot | Token::LeftBracket)) => {
                        self.require_declared(&name)?;
                        let record = self.static_record_type(&name);
                        self.call_object(name, record)?;
                    }
                    None => {
                        self.call_site(name, None, true, has_parens)?;
                    }
                }
            }
            Some(Token::Word(word))
                if word == "IMPORT"
                    && matches!(self.tokens.get(self.position + 1), Some(Token::String(_))) =>
            {
                self.next();
                if !top_level || self.current_function.is_some() || !self.blocks.is_empty() {
                    return Err(Error::new(
                        self.line,
                        "IMPORT must be at the top level, outside functions and blocks",
                    ));
                }
                // The module was already loaded before this module's own code
                // (see `Linker::compile_module`); only the syntax is left.
                self.next();
                if self.peek().is_some() {
                    if self.next() != Some(Token::Word("AS".into())) {
                        return Err(Error::new(self.line, "expected IMPORT \"path\" [AS alias]"));
                    }
                    self.variable()?;
                }
            }
            Some(Token::Word(word)) if word == "WHILE" => {
                self.next();
                let start = self.instructions.len();
                self.expression(0)?;
                let branch = self.emit(Op::JumpIfFalse(0));
                self.blocks.push(Block::While {
                    start,
                    branch,
                    line: self.line,
                    jumps: LoopJumps::default(),
                });
            }
            Some(Token::Word(word)) if word == "WEND" => {
                self.next();
                let Some(Block::While {
                    start,
                    branch,
                    jumps,
                    ..
                }) = self.blocks.pop()
                else {
                    return Err(Error::new(self.line, "WEND must match the innermost WHILE"));
                };
                self.emit(Op::Jump(start));
                let end = self.instructions.len();
                self.patch(branch, end);
                self.patch_loop(jumps, start, end);
            }
            Some(Token::Word(word)) if word == "DO" => {
                self.next();
                let start = self.instructions.len();
                let mut jumps = LoopJumps::default();
                let mut has_condition = false;
                if let Some(until) = self.loop_condition()? {
                    has_condition = true;
                    jumps.exits.push(self.emit(if until {
                        Op::JumpIfTrue(0)
                    } else {
                        Op::JumpIfFalse(0)
                    }));
                }
                self.blocks.push(Block::Do {
                    start,
                    has_condition,
                    line: self.line,
                    jumps,
                });
            }
            Some(Token::Word(word)) if word == "LOOP" => {
                self.next();
                let Some(Block::Do {
                    start,
                    has_condition,
                    jumps,
                    ..
                }) = self.blocks.pop()
                else {
                    return Err(Error::new(self.line, "LOOP must match the innermost DO"));
                };
                let condition_at = self.instructions.len();
                let continue_at = match self.loop_condition()? {
                    Some(until) => {
                        if has_condition {
                            return Err(Error::new(
                                self.line,
                                "DO loop cannot have both a DO and a LOOP condition",
                            ));
                        }
                        self.emit(if until {
                            Op::JumpIfFalse(start)
                        } else {
                            Op::JumpIfTrue(start)
                        });
                        condition_at
                    }
                    None => {
                        self.emit(Op::Jump(start));
                        start
                    }
                };
                let end = self.instructions.len();
                self.patch_loop(jumps, continue_at, end);
            }
            Some(Token::Word(word)) if word == "FOR" => {
                self.next();
                let each = self.peek() == Some(&Token::Word("EACH".into()));
                if each {
                    self.next();
                }
                let variable = self.variable()?;
                self.check_assignable(&variable)?;
                self.implicit_declare(&variable);
                let start = if each {
                    let mut value_variable = None;
                    if self.peek() == Some(&Token::Comma) {
                        self.next();
                        let second = self.variable()?;
                        self.check_assignable(&second)?;
                        if second == variable {
                            return Err(Error::new(
                                self.line,
                                "FOR EACH key and value variables must differ",
                            ));
                        }
                        self.implicit_declare(&second);
                        value_variable = Some(second);
                    }
                    self.expect(Token::Word("IN".into()))?;
                    self.expression(0)?;
                    self.emit(Op::EachStart {
                        variable: variable.clone(),
                        value_variable,
                        end: 0,
                    })
                } else {
                    if variable.ends_with('$') {
                        return Err(Error::new(
                            self.line,
                            "numeric FOR requires a numeric variable",
                        ));
                    }
                    self.expect(Token::Equal)?;
                    self.expression(0)?;
                    self.expect(Token::Word("TO".into()))?;
                    self.expression(0)?;
                    if self.peek() == Some(&Token::Word("STEP".into())) {
                        self.next();
                        self.expression(0)?;
                    } else {
                        self.emit(Op::Number(1.0));
                    }
                    self.emit(Op::ForStart {
                        variable: variable.clone(),
                        end: 0,
                    })
                };
                self.blocks.push(Block::For {
                    start,
                    variable,
                    line: self.line,
                    jumps: LoopJumps::default(),
                });
            }
            Some(Token::Word(word)) if word == "NEXT" => {
                self.next();
                let Some(Block::For {
                    start,
                    variable,
                    jumps,
                    ..
                }) = self.blocks.pop()
                else {
                    return Err(Error::new(self.line, "NEXT must match the innermost FOR"));
                };
                if self.peek().is_some() && self.variable()? != variable {
                    return Err(Error::new(self.line, "NEXT variable does not match FOR"));
                }
                let next_at = self.emit(Op::ForNext { start });
                let target = self.instructions.len();
                match &mut self.instructions[start].op {
                    Op::ForStart { end, .. } | Op::EachStart { end, .. } => *end = target,
                    _ => unreachable!("FOR block must refer to a loop start"),
                }
                self.patch_loop(jumps, next_at, target);
            }
            Some(Token::Word(word)) if word == "APPEND" || word == "REMOVE" => {
                let append = self.next() == Some(Token::Word("APPEND".into()));
                self.expression(0)?;
                self.expect(Token::Comma)?;
                self.expression(0)?;
                self.emit(if append { Op::Append } else { Op::Remove });
            }
            Some(Token::Word(word)) if word == "TRY" && !self.assignment_follows() => {
                self.next();
                let begin = self.emit(Op::TryBegin {
                    catch: None,
                    finally: None,
                });
                self.blocks.push(Block::Try {
                    begin,
                    phase: TryPhase::Body,
                    has_catch: false,
                    has_finally: false,
                    leaves: Vec::new(),
                    line: self.line,
                });
            }
            Some(Token::Word(word)) if word == "CATCH" && !self.assignment_follows() => {
                self.next();
                self.catch_clause()?;
            }
            Some(Token::Word(word)) if word == "FINALLY" && !self.assignment_follows() => {
                self.next();
                self.finally_clause()?;
            }
            Some(Token::Word(word)) if word == "THROW" && !self.assignment_follows() => {
                self.next();
                if self.peek().is_none() {
                    let mut depth = 0;
                    let mut found = None;
                    for block in self.blocks.iter().rev() {
                        if let Block::Try { phase, .. } = block {
                            if *phase == TryPhase::Catch {
                                found = Some(depth);
                                break;
                            }
                            depth += 1;
                        }
                    }
                    let depth = found.ok_or_else(|| {
                        Error::new(
                            self.line,
                            "THROW without a message is only valid inside a CATCH block",
                        )
                    })?;
                    self.emit(Op::Rethrow(depth));
                } else {
                    self.expression(0)?;
                    self.emit(Op::Throw);
                }
            }
            Some(Token::Word(word))
                if matches!(word.as_str(), "OPEN" | "CLOSE" | "PUT" | "SEEK")
                    && !matches!(
                        self.tokens.get(self.position + 1),
                        Some(Token::Equal | Token::LeftBracket | Token::Dot)
                    ) =>
            {
                let word = word.clone();
                self.next();
                self.file_statement(&word)?;
            }
            Some(Token::Word(word))
                if word == "LINE"
                    && self.tokens.get(self.position + 1) == Some(&Token::Word("INPUT".into())) =>
            {
                self.next();
                self.next();
                self.file_number()?;
                self.expect(Token::Comma)?;
                let name = self.variable()?;
                if !name.ends_with('$') {
                    return Err(Error::new(
                        self.line,
                        "LINE INPUT requires a string variable",
                    ));
                }
                self.check_assignable(&name)?;
                self.implicit_declare(&name);
                self.emit(Op::FileInput(name));
            }
            Some(Token::Word(word))
                if word == "ON"
                    && matches!(self.tokens.get(self.position + 1),
                        Some(Token::Word(kind)) if matches!(kind.as_str(), "KEY" | "MOUSE" | "CLOSE")) =>
            {
                self.next();
                let kind = match self.next() {
                    Some(Token::Word(kind)) if kind == "KEY" => EventKind::Key,
                    Some(Token::Word(kind)) if kind == "MOUSE" => EventKind::Mouse,
                    _ => EventKind::Close,
                };
                let label = kind.name();
                match self.next() {
                    Some(Token::Word(word)) if word == "OFF" => {
                        if self.peek().is_some() {
                            return Err(Error::new(
                                self.line,
                                format!("unexpected text after ON {label} OFF"),
                            ));
                        }
                        self.emit(Op::OnEvent {
                            kind,
                            function: None,
                        });
                    }
                    Some(Token::Word(word)) if word == "CALL" => {
                        let Some(Token::Word(name)) = self.next() else {
                            return Err(Error::new(
                                self.line,
                                format!("expected a function name after ON {label} CALL"),
                            ));
                        };
                        if self.peek().is_some() {
                            return Err(Error::new(
                                self.line,
                                format!("unexpected text after ON {label} CALL {name}"),
                            ));
                        }
                        let signature = self.signatures.get(&name).cloned().ok_or_else(|| {
                            Error::new(
                                self.line,
                                format!("ON {label} CALL: unknown function {name} (it must be a FUNCTION of this file)"),
                            )
                        })?;
                        let wanted = kind.handler_parameters();
                        if signature.required() > wanted || signature.parameters.len() < wanted {
                            return Err(Error::new(
                                self.line,
                                format!(
                                    "ON {label} CALL: function {name} must accept exactly {wanted} argument(s)"
                                ),
                            ));
                        }
                        let index = self.emit(Op::OnEvent {
                            kind,
                            function: None,
                        });
                        self.calls.push(PendingCall {
                            index,
                            name: signature.qualified,
                            user: true,
                            method: None,
                            arguments: wanted,
                            line: self.line,
                            file: self.file,
                        });
                    }
                    _ => {
                        return Err(Error::new(
                            self.line,
                            format!("expected CALL function or OFF after ON {label}"),
                        ));
                    }
                }
            }
            Some(Token::Word(word))
                if Builtin::statement(word).is_some()
                    && !matches!(
                        self.tokens.get(self.position + 1),
                        Some(Token::Equal | Token::LeftBracket | Token::Dot)
                    ) =>
            {
                // Statement-form builtins stay usable as variable names when
                // the line is an assignment (`SORT = 1`, `REVERSE[1] = 2`).
                let builtin = Builtin::statement(word).expect("checked above");
                self.next();
                let mut builtin = builtin;
                let mut arguments = 0;
                // FLIP / PRESENT take an optional trailing CLEAR word.
                if matches!(builtin, Builtin::Flip | Builtin::Present)
                    && self.position + 1 == self.tokens.len()
                    && matches!(self.peek(), Some(Token::Word(word)) if word == "CLEAR")
                {
                    self.next();
                    builtin = Builtin::FlipClear;
                }
                // LINE, CIRCLE and SCREENSHOT take a trailing B/BF, FILL or
                // VISIBLE flag word.
                let (fixed, flags): (usize, &[&str]) = match builtin {
                    Builtin::Line => (4, &["B", "BF"]),
                    Builtin::Circle => (3, &["FILL"]),
                    Builtin::Screenshot => (1, &["VISIBLE"]),
                    _ => (0, &[]),
                };
                let mut flag = None;
                // WAVE takes a waveform name as a bare keyword (`WAVE SINE`)
                // or any string expression (`WAVE "sine"`, `WAVE name$`).
                let bare_waveform = builtin == Builtin::Wave
                    && self.position + 1 == self.tokens.len()
                    && matches!(
                        self.peek(),
                        Some(Token::Word(word))
                            if matches!(word.as_str(), "SQUARE" | "SINE" | "TRIANGLE" | "SAW" | "NOISE")
                    );
                if bare_waveform {
                    if let Some(Token::Word(word)) = self.next() {
                        self.emit(Op::String(word.to_lowercase()));
                    }
                    arguments = 1;
                } else if self.peek().is_some() || builtin.arity().0 > 0 {
                    loop {
                        if fixed > 0
                            && arguments >= fixed
                            && self.position + 1 == self.tokens.len()
                            && let Some(Token::Word(word)) = self.peek()
                            && flags.contains(&word.as_str())
                        {
                            flag = Some(word.clone());
                            self.next();
                            break;
                        }
                        if fixed > 0 && arguments > fixed {
                            // After the colour only the flag word may follow.
                            let message = match builtin {
                                Builtin::Line => "LINE flag must be B or BF",
                                Builtin::Circle => "CIRCLE flag must be FILL",
                                _ => "SCREENSHOT flag must be VISIBLE",
                            };
                            return Err(Error::new(self.line, message));
                        }
                        if builtin == Builtin::Screenshot && arguments == fixed {
                            // Only the VISIBLE word may follow the file name.
                            return Err(Error::new(self.line, "SCREENSHOT flag must be VISIBLE"));
                        }
                        if fixed > 0 && arguments == fixed && self.peek() == Some(&Token::Comma) {
                            // `LINE x1, y1, x2, y2, , B`: the colour is omitted.
                            self.emit(Op::Nothing);
                        } else {
                            self.expression(0)?;
                        }
                        arguments += 1;
                        if self.peek() != Some(&Token::Comma) {
                            break;
                        }
                        self.next();
                    }
                }
                builtin = match (builtin, flag.as_deref()) {
                    (Builtin::Line, Some("B")) => Builtin::LineBox,
                    (Builtin::Line, Some(_)) => Builtin::LineFill,
                    (Builtin::Circle, Some(_)) => Builtin::CircleFill,
                    (Builtin::Screenshot, Some(_)) => Builtin::ScreenshotVisible,
                    (other, _) => other,
                };
                let (minimum, maximum) = builtin.arity();
                if arguments < minimum || arguments > maximum {
                    return Err(Error::new(
                        self.line,
                        format!(
                            "{} requires {} arguments, got {arguments}",
                            builtin.name(),
                            arity_text(minimum, maximum)
                        ),
                    ));
                }
                self.emit(Op::Builtin {
                    builtin,
                    arguments,
                    discard_result: true,
                });
            }
            Some(Token::Word(word)) if word == "PRINT" => {
                self.next();
                let file = self.peek() == Some(&Token::Hash);
                if file {
                    self.file_number()?;
                    if self.peek() == Some(&Token::Comma) {
                        self.next();
                    } else if self.peek().is_some() {
                        return Err(Error::new(self.line, "expected ',' after the file number"));
                    }
                }
                let (print, separator, newline) = if file {
                    (Op::FilePrint, Op::FilePrintSeparator, Op::FilePrintNewline)
                } else {
                    (Op::Print, Op::PrintSeparator, Op::PrintNewline)
                };
                if self.peek().is_none() {
                    self.emit(newline);
                } else {
                    loop {
                        self.expression(0)?;
                        self.emit(print.clone());
                        match self.peek() {
                            Some(Token::Semicolon) | Some(Token::Comma) => {
                                if self.next() == Some(Token::Comma) {
                                    self.emit(separator.clone());
                                }
                                if self.peek().is_none() {
                                    break;
                                }
                            }
                            _ => {
                                self.emit(newline);
                                break;
                            }
                        }
                    }
                }
                if file {
                    // Discard the file number kept beneath the printed values.
                    self.emit(Op::Pop);
                }
            }
            Some(Token::Word(word)) if word == "GOTO" => {
                self.next();
                self.goto()?;
            }
            Some(Token::Word(word)) if word == "IF" => {
                self.next();
                self.expression(0)?;
                self.expect(Token::Word("THEN".into()))?;
                let branch = self.emit(Op::JumpIfFalse(0));
                if self.peek().is_none() {
                    if !top_level {
                        return Err(Error::new(
                            self.line,
                            "block IF must be on its own line; an inline IF needs a statement after THEN",
                        ));
                    }
                    self.blocks.push(Block::If {
                        pending: Some(branch),
                        end_jumps: Vec::new(),
                        seen_else: false,
                        line: self.line,
                    });
                    return Ok(());
                }
                if matches!(self.peek(), Some(Token::Number(_))) {
                    self.goto()?;
                } else {
                    self.statement(false)?;
                }
                self.instructions[branch].op = Op::JumpIfFalse(self.instructions.len());
            }
            Some(Token::Word(word)) if word == "ELSEIF" || word == "ELSE" => {
                let elseif = self.next() == Some(Token::Word("ELSEIF".into()));
                let keyword = if elseif { "ELSEIF" } else { "ELSE" };
                let Some(Block::If {
                    pending, seen_else, ..
                }) = self.blocks.last_mut()
                else {
                    return Err(Error::new(
                        self.line,
                        format!("{keyword} without a matching block IF"),
                    ));
                };
                if *seen_else {
                    return Err(Error::new(
                        self.line,
                        format!("{keyword} after ELSE in the same IF"),
                    ));
                }
                *seen_else = !elseif;
                let previous = pending.take();
                let skip = self.emit(Op::Jump(0));
                if let Some(previous) = previous {
                    self.patch(previous, skip + 1);
                }
                if elseif {
                    self.expression(0)?;
                    self.expect(Token::Word("THEN".into()))?;
                    if self.peek().is_some() {
                        return Err(Error::new(self.line, "ELSEIF ... THEN must end the line"));
                    }
                    let branch = self.emit(Op::JumpIfFalse(0));
                    if let Some(Block::If {
                        pending, end_jumps, ..
                    }) = self.blocks.last_mut()
                    {
                        *pending = Some(branch);
                        end_jumps.push(skip);
                    }
                } else if let Some(Block::If { end_jumps, .. }) = self.blocks.last_mut() {
                    end_jumps.push(skip);
                }
            }
            Some(Token::Word(word)) if word == "SELECT" => {
                self.next();
                self.expect(Token::Word("CASE".into()))?;
                self.expression(0)?;
                self.blocks.push(Block::Select {
                    next: None,
                    end_jumps: Vec::new(),
                    seen_case: false,
                    seen_else: false,
                    line: self.line,
                });
            }
            Some(Token::Word(word)) if word == "CASE" => {
                self.next();
                self.case_clause()?;
            }
            Some(Token::Word(word)) if word == "END" && ends_block => {
                self.next();
                let kind = self.next();
                if kind == Some(Token::Word("TRY".into())) {
                    self.end_try()?;
                } else if kind == Some(Token::Word("TEST".into())) {
                    let Some(context) = self.current_function.as_ref() else {
                        return Err(Error::new(self.line, "END TEST without a matching TEST"));
                    };
                    if !context.test {
                        return Err(Error::new(
                            self.line,
                            "END TEST cannot close a FUNCTION; expected FEND",
                        ));
                    }
                    if !self.blocks.is_empty() {
                        return Err(Error::new(
                            self.line,
                            "END TEST cannot close an unfinished loop or block",
                        ));
                    }
                    self.end_function();
                } else if kind == Some(Token::Word("IF".into())) {
                    let Some(Block::If {
                        pending, end_jumps, ..
                    }) = self.blocks.pop()
                    else {
                        return Err(Error::new(self.line, "END IF without a matching block IF"));
                    };
                    let end = self.instructions.len();
                    if let Some(pending) = pending {
                        self.patch(pending, end);
                    }
                    for jump in end_jumps {
                        self.patch(jump, end);
                    }
                } else {
                    let Some(Block::Select {
                        next,
                        mut end_jumps,
                        seen_case,
                        seen_else,
                        ..
                    }) = self.blocks.pop()
                    else {
                        return Err(Error::new(
                            self.line,
                            "END SELECT without a matching SELECT CASE",
                        ));
                    };
                    if !seen_else {
                        if seen_case {
                            end_jumps.push(self.emit(Op::Jump(0)));
                        }
                        if let Some(next) = next {
                            self.patch(next, self.instructions.len());
                        }
                        // No clause matched: discard the selector.
                        self.emit(Op::Pop);
                    }
                    let end = self.instructions.len();
                    for jump in end_jumps {
                        self.patch(jump, end);
                    }
                }
            }
            Some(Token::Word(word)) if word == "END" || word == "STOP" => {
                let stop = word == "STOP";
                self.next();
                self.emit(Op::Terminate { stop });
            }
            Some(Token::Word(word)) if word == "GC" => {
                self.next();
                self.emit(Op::Collect);
            }
            Some(Token::Word(word)) if word == "INPUT" => {
                self.next();
                if self.peek() == Some(&Token::Hash) {
                    self.file_number()?;
                    self.expect(Token::Comma)?;
                    let name = self.variable()?;
                    self.check_assignable(&name)?;
                    self.implicit_declare(&name);
                    self.emit(Op::FileInput(name));
                    return Ok(());
                }
                let name = self.variable()?;
                self.check_assignable(&name)?;
                self.implicit_declare(&name);
                self.emit(Op::Input(name));
            }
            _ => {
                if self.peek() == Some(&Token::Word("LET".into())) {
                    self.next();
                }
                let name = self.variable()?;
                let qualifier = self.qualified_member(&name)?;
                let (name, qualifier) = match qualifier {
                    Some((alias, exports, member)) => {
                        if self.peek() == Some(&Token::LeftParen) {
                            return Err(Error::new(
                                self.line,
                                format!("call module functions with CALL {alias}.{member}(...)"),
                            ));
                        }
                        if !exports.members.contains(&member) {
                            return Err(Error::new(
                                self.line,
                                format!("module {alias} has no variable {member}"),
                            ));
                        }
                        (
                            format!("{}.{member}", exports.prefix),
                            Some((exports, member)),
                        )
                    }
                    None => (name, None),
                };
                let postfix = matches!(self.peek(), Some(Token::LeftBracket | Token::Dot));
                if !postfix {
                    match &qualifier {
                        Some((exports, member)) if exports.constants.contains(member) => {
                            return Err(Error::new(
                                self.line,
                                format!("cannot assign to constant {member}"),
                            ));
                        }
                        Some(_) => {}
                        None => self.check_assignable(&name)?,
                    }
                }
                if qualifier.is_none() {
                    self.require_declared(&name)?;
                }
                // The last `[index]` or `.field` of the target is assigned;
                // everything before it is read.
                let mut target = Target::Variable;
                if postfix {
                    self.emit(Op::Load(name.clone()));
                    let record = match &qualifier {
                        Some((exports, member)) => match exports.var_types.get(member) {
                            Some(VarType::Record(id)) => Some(*id),
                            _ => None,
                        },
                        None => self.static_record_type(&name),
                    };
                    target = self.postfix_steps(record)?;
                    if matches!(target, Target::Call) {
                        return Err(Error::new(
                            self.line,
                            "a method call is not assignable; run a method as a statement with CALL obj.Method(...)",
                        ));
                    }
                }
                self.expect(Token::Equal)?;
                self.expression(0)?;
                self.emit(match target {
                    Target::Index => Op::IndexSet,
                    Target::Field(field) => Op::SetField(field),
                    Target::Variable => Op::Store(name),
                    Target::Call => unreachable!("method call targets are rejected above"),
                });
            }
        }
        Ok(())
    }

    /// `CALL obj.path.Method(...)`: the base variable `name` is loaded and
    /// the chain must end in a method call whose result is discarded.
    fn call_object(&mut self, name: String, record: Option<usize>) -> Result<(), Error> {
        self.emit(Op::Load(name));
        self.statement_call = true;
        let target = self.postfix_steps(record);
        self.statement_call = false;
        let target = target?;
        let last = self.instructions.len() - 1;
        if !matches!(target, Target::Call)
            || self.peek().is_some()
            || self.last_method_call != Some(last)
        {
            return Err(Error::new(
                self.line,
                "CALL on an object must end in a method call: CALL obj.Method(...)",
            ));
        }
        match &mut self.instructions[last].op {
            Op::Call { discard_result, .. } | Op::CallMethod { discard_result, .. } => {
                *discard_result = true;
            }
            _ => unreachable!("a method call instruction was just emitted"),
        }
        Ok(())
    }

    /// Parses `[index]` and `.field` steps after a variable that is already
    /// loaded. Every step but the last is read; the last one is returned
    /// unread, with its container (and index) left on the operand stack.
    fn postfix_steps(&mut self, mut record: Option<usize>) -> Result<Target, Error> {
        let mut target = Target::Variable;
        loop {
            let step = match self.peek() {
                Some(Token::LeftBracket) => true,
                Some(Token::Dot) => false,
                _ => break,
            };
            match std::mem::replace(&mut target, Target::Variable) {
                Target::Index => {
                    self.emit(Op::IndexGet);
                }
                Target::Field(field) => {
                    self.emit(Op::GetField(field));
                }
                Target::Variable | Target::Call => {}
            }
            self.next();
            if step {
                self.expression(0)?;
                while self.peek() == Some(&Token::Comma) {
                    self.next();
                    self.emit(Op::IndexGet);
                    self.expression(0)?;
                }
                self.expect(Token::RightBracket)?;
                record = None;
                target = Target::Index;
            } else if self.method_step() {
                record = self.method_call(record)?;
                target = Target::Call;
            } else {
                let (field, next) = self.field_access(record)?;
                record = next;
                target = Target::Field(field);
            }
        }
        Ok(target)
    }

    /// After a name: when it is a module alias followed by `.member`,
    /// consumes the member and returns the module (aliases win over
    /// variables of the same name for `name.member`).
    fn qualified_member(
        &mut self,
        name: &str,
    ) -> Result<Option<(String, Rc<Exports>, String)>, Error> {
        if self.peek() != Some(&Token::Dot) {
            return Ok(None);
        }
        let Some(exports) = self.aliases.get(name) else {
            return Ok(None);
        };
        let Some(Token::Word(member)) = self.tokens.get(self.position + 1) else {
            return Ok(None);
        };
        let result = (name.to_string(), Rc::clone(exports), member.clone());
        self.position += 2;
        Ok(Some(result))
    }

    fn patch(&mut self, index: usize, target: usize) {
        self.instructions[index].op = match self.instructions[index].op {
            Op::Jump(_) => Op::Jump(target),
            Op::Leave { tries, .. } => Op::Leave { target, tries },
            Op::JumpIfFalse(_) => Op::JumpIfFalse(target),
            Op::JumpIfTrue(_) => Op::JumpIfTrue(target),
            Op::AndJump(_) => Op::AndJump(target),
            Op::OrJump(_) => Op::OrJump(target),
            _ => unreachable!("only jump instructions can be patched"),
        };
    }

    /// True when the next token after the current word is `=` or `[`, i.e. the
    /// word is being used as a legacy variable name rather than a statement.
    fn assignment_follows(&self) -> bool {
        matches!(
            self.tokens.get(self.position + 1),
            Some(Token::Equal | Token::LeftBracket | Token::Dot)
        )
    }

    fn file_number(&mut self) -> Result<(), Error> {
        self.expect(Token::Hash)?;
        self.expression(0)
    }

    fn emit_builtin(&mut self, builtin: Builtin, arguments: usize) {
        self.emit(Op::Builtin {
            builtin,
            arguments,
            discard_result: true,
        });
    }

    /// OPEN, CLOSE, PUT and SEEK (the word itself is already consumed).
    fn file_statement(&mut self, word: &str) -> Result<(), Error> {
        match word {
            "OPEN" => {
                self.expression(0)?;
                self.expect(Token::Word("FOR".into()))?;
                let mode = match self.next() {
                    Some(Token::Word(mode)) if mode == "INPUT" => 0.0,
                    Some(Token::Word(mode)) if mode == "OUTPUT" => 1.0,
                    Some(Token::Word(mode)) if mode == "APPEND" => 2.0,
                    Some(Token::Word(mode)) if mode == "BINARY" => 3.0,
                    _ => {
                        return Err(Error::new(
                            self.line,
                            "OPEN mode must be INPUT, OUTPUT, APPEND or BINARY",
                        ));
                    }
                };
                self.expect(Token::Word("AS".into()))?;
                self.file_number()?;
                self.emit(Op::Number(mode));
                self.emit_builtin(Builtin::Open, 3);
            }
            "CLOSE" => {
                let mut count = 0;
                if self.peek().is_some() {
                    loop {
                        self.file_number()?;
                        count += 1;
                        if self.peek() != Some(&Token::Comma) {
                            break;
                        }
                        self.next();
                    }
                }
                self.emit_builtin(Builtin::Close, count);
            }
            _ => {
                self.file_number()?;
                self.expect(Token::Comma)?;
                self.expression(0)?;
                let builtin = if word == "PUT" {
                    Builtin::Put
                } else {
                    Builtin::Seek
                };
                self.emit_builtin(builtin, 2);
            }
        }
        Ok(())
    }

    fn try_block(&mut self, clause: &str) -> Result<&mut Block, Error> {
        let line = self.line;
        match self.blocks.last_mut() {
            Some(block @ Block::Try { .. }) => Ok(block),
            _ => Err(Error::new(line, format!("{clause} without a matching TRY"))),
        }
    }

    fn set_handler(&mut self, begin: usize, catch: Option<usize>, finally: Option<usize>) {
        if let Op::TryBegin {
            catch: old_catch,
            finally: old_finally,
        } = &mut self.instructions[begin].op
        {
            *old_catch = catch.or(*old_catch);
            *old_finally = finally.or(*old_finally);
        }
    }

    fn catch_clause(&mut self) -> Result<(), Error> {
        let line = self.line;
        let Block::Try { begin, phase, .. } = self.try_block("CATCH")? else {
            unreachable!("try_block returns TRY blocks");
        };
        if *phase != TryPhase::Body {
            return Err(Error::new(
                line,
                "CATCH must follow the TRY body and may appear only once, before FINALLY",
            ));
        }
        let begin = *begin;
        *phase = TryPhase::Catch;
        let leave = self.emit(Op::Leave {
            target: 0,
            tries: 1,
        });
        let catch_at = self.instructions.len();
        if let Some(Block::Try {
            leaves, has_catch, ..
        }) = self.blocks.last_mut()
        {
            leaves.push(leave);
            *has_catch = true;
        }
        self.set_handler(begin, Some(catch_at), None);
        let variable = if self.peek().is_some() {
            let name = self.variable()?;
            if !name.ends_with('$') {
                return Err(Error::new(
                    self.line,
                    "the CATCH variable must be a string variable ending in $",
                ));
            }
            self.check_assignable(&name)?;
            self.implicit_declare(&name);
            Some(name)
        } else {
            None
        };
        self.emit(Op::CatchStore(variable));
        Ok(())
    }

    fn finally_clause(&mut self) -> Result<(), Error> {
        let line = self.line;
        let Block::Try { begin, phase, .. } = self.try_block("FINALLY")? else {
            unreachable!("try_block returns TRY blocks");
        };
        if *phase == TryPhase::Finally {
            return Err(Error::new(line, "TRY may have only one FINALLY block"));
        }
        let begin = *begin;
        *phase = TryPhase::Finally;
        let leave = self.emit(Op::Leave {
            target: 0,
            tries: 1,
        });
        let finally_at = self.instructions.len();
        if let Some(Block::Try {
            leaves,
            has_finally,
            ..
        }) = self.blocks.last_mut()
        {
            leaves.push(leave);
            *has_finally = true;
        }
        self.set_handler(begin, None, Some(finally_at));
        Ok(())
    }

    /// `TEST name` header (after the keyword): opens a hidden function.
    fn test_header(&mut self, top_level: bool) -> Result<(), Error> {
        if !top_level || self.current_function.is_some() || !self.blocks.is_empty() {
            let nested = self
                .current_function
                .as_ref()
                .is_some_and(|context| context.test);
            return Err(Error::new(
                self.line,
                if nested {
                    "TEST blocks cannot be nested; close the TEST with END TEST first"
                } else {
                    "TEST blocks must be at the top level, outside functions, classes and blocks"
                },
            ));
        }
        let key = match self.next() {
            None => {
                return Err(Error::new(self.line, "TEST requires a name: TEST name"));
            }
            Some(Token::Word(name))
                if !is_keyword(&name)
                    && !name.ends_with('$')
                    && VarType::parse(&name).is_none() =>
            {
                name
            }
            Some(_) => {
                return Err(Error::new(
                    self.line,
                    "invalid test name; use a single identifier of letters, digits and _",
                ));
            }
        };
        if self.peek().is_some() {
            return Err(Error::new(
                self.line,
                "TEST name must be the last thing on the line",
            ));
        }
        let name = crate::lexer::spelled_after(self.text, "TEST").unwrap_or_else(|| key.clone());
        if self.tests.iter().any(|test| test.key == key) {
            return Err(Error::new(self.line, format!("duplicate test {name}")));
        }
        let skip = self.emit(Op::Jump(0));
        let index = self.functions.len();
        let hidden = format!("TEST {key}");
        self.functions.push(Function {
            qualified: if self.prefix.is_empty() {
                hidden.clone()
            } else {
                format!("{}.{hidden}", self.prefix)
            },
            name: hidden,
            entry: self.instructions.len(),
            parameters: Vec::new(),
            params: Vec::new(),
            locals: Vec::new(),
            types: HashMap::new(),
            class: None,
        });
        self.tests.push(RawTest {
            key,
            name,
            line: self.line,
            function: index,
        });
        *self.current_function = Some(FunctionContext {
            index,
            test: true,
            skip,
            line: self.line,
            exits: Vec::new(),
            constants: HashSet::new(),
            fills: HashMap::new(),
        });
        Ok(())
    }

    /// Closes the current function or test: emits the return, patches the
    /// exits and the skip jump, and qualifies a module's global names.
    fn end_function(&mut self) {
        let context = self
            .current_function
            .as_mut()
            .expect("the caller checked for an open function");
        let skip = context.skip;
        let index = context.index;
        let exits = std::mem::take(&mut context.exits);
        let return_at = self.emit(Op::Return);
        for exit in exits {
            self.patch(exit, return_at);
        }
        self.instructions[skip].op = Op::Jump(self.instructions.len());
        let entry = self.functions[index].entry;
        self.function_ranges.push((entry, return_at));
        if !self.prefix.is_empty() {
            // Names that are not locals of this function are globals
            // of this module: give them the module's prefix.
            let function = &self.functions[index];
            let mut locals: HashSet<String> = function.locals.iter().cloned().collect();
            locals.extend(function.parameters.iter().cloned());
            locals.insert(function.name.clone());
            qualify_globals(
                &mut self.instructions[entry..=return_at],
                self.prefix,
                &locals,
            );
        }
        *self.current_function = None;
    }

    /// True when the statement's arguments start with a parenthesis whose
    /// matching `)` encloses a top-level comma, as in `ASSERTEQ (1, 2)`.
    fn arguments_wrapped_in_parentheses(&self) -> bool {
        if self.peek() != Some(&Token::LeftParen) {
            return false;
        }
        let mut depth = 0usize;
        for token in &self.tokens[self.position..] {
            match token {
                Token::LeftParen => depth += 1,
                Token::RightParen => {
                    depth -= 1;
                    if depth == 0 {
                        return false;
                    }
                }
                Token::Comma if depth == 1 => return true,
                _ => {}
            }
        }
        false
    }

    /// `ASSERT`, `ASSERTEQ` and `ASSERTNEAR` (after the keyword).
    fn assert_statement(&mut self, keyword: &str) -> Result<(), Error> {
        let (minimum, maximum) = match keyword {
            "ASSERT" => (1, 2),
            "ASSERTEQ" => (2, 3),
            _ => (3, 4),
        };
        let mut count = 0;
        if self.peek().is_some() {
            let wrapped = self.arguments_wrapped_in_parentheses();
            loop {
                if let Err(mut error) = self.expression(0) {
                    if wrapped {
                        let form = match keyword {
                            "ASSERT" => "ASSERT condition, message$",
                            "ASSERTEQ" => "ASSERTEQ expected, actual",
                            _ => "ASSERTNEAR expected, actual, epsilon",
                        };
                        error.message = format!(
                            "{}; write {form} without parentheses around the argument list",
                            error.message
                        );
                    }
                    return Err(error);
                }
                count += 1;
                if self.peek() == Some(&Token::Comma) {
                    self.next();
                } else {
                    break;
                }
            }
        }
        if count < minimum || count > maximum {
            return Err(Error::new(
                self.line,
                format!(
                    "{keyword} requires {} arguments, got {count}",
                    arity_text(minimum, maximum)
                ),
            ));
        }
        let has_message = count == maximum;
        self.emit(match keyword {
            "ASSERT" => Op::Assert { has_message },
            "ASSERTEQ" => Op::AssertEq { has_message },
            _ => Op::AssertNear { has_message },
        });
        Ok(())
    }

    fn end_try(&mut self) -> Result<(), Error> {
        let Some(Block::Try {
            phase,
            has_catch,
            has_finally,
            mut leaves,
            ..
        }) = (match self.blocks.last() {
            Some(Block::Try { .. }) => self.blocks.pop(),
            _ => None,
        })
        else {
            return Err(Error::new(self.line, "END TRY without a matching TRY"));
        };
        if !has_catch && !has_finally {
            return Err(Error::new(
                self.line,
                "TRY requires a CATCH block, a FINALLY block, or both",
            ));
        }
        if phase == TryPhase::Finally {
            self.emit(Op::EndFinally);
        } else {
            leaves.push(self.emit(Op::Leave {
                target: 0,
                tries: 1,
            }));
        }
        let end = self.instructions.len();
        for leave in leaves {
            self.patch(leave, end);
        }
        Ok(())
    }

    /// Emits an unpatched jump out of the blocks from index `from` upward; it
    /// becomes a `Leave` when TRY blocks are crossed so FINALLY blocks run.
    fn emit_leave(&mut self, from: usize) -> usize {
        let tries = self.blocks[from..]
            .iter()
            .filter(|block| matches!(block, Block::Try { .. }))
            .count();
        self.emit(if tries == 0 {
            Op::Jump(0)
        } else {
            Op::Leave { target: 0, tries }
        })
    }

    fn inside_try(&self) -> bool {
        self.blocks
            .iter()
            .any(|block| matches!(block, Block::Try { .. }))
    }

    fn patch_loop(&mut self, jumps: LoopJumps, continue_at: usize, end: usize) {
        for exit in jumps.exits {
            self.patch(exit, end);
        }
        for jump in jumps.continues {
            self.patch(jump, continue_at);
        }
    }

    /// Parses an optional `WHILE cond` / `UNTIL cond`; returns `Some(is_until)`.
    fn loop_condition(&mut self) -> Result<Option<bool>, Error> {
        let until = match self.peek() {
            Some(Token::Word(word)) if word == "WHILE" => false,
            Some(Token::Word(word)) if word == "UNTIL" => true,
            _ => return Ok(None),
        };
        self.next();
        self.expression(0)?;
        Ok(Some(until))
    }

    fn check_assignable(&self, name: &str) -> Result<(), Error> {
        if name == "SELF"
            && self
                .current_function
                .as_ref()
                .is_some_and(|context| self.functions[context.index].class.is_some())
        {
            return Err(Error::new(
                self.line,
                "SELF cannot be assigned; assign its fields (SELF.field = value)",
            ));
        }
        let constant = match self.current_function.as_ref() {
            Some(context) => {
                let function = &self.functions[context.index];
                if name == function.name
                    || function
                        .parameters
                        .iter()
                        .any(|parameter| parameter == name)
                    || function.locals.iter().any(|local| local == name)
                {
                    context.constants.contains(name)
                } else {
                    self.global_constants.contains(name)
                }
            }
            None => self.global_constants.contains(name),
        };
        if constant {
            Err(Error::new(
                self.line,
                format!("cannot assign to constant {name}"),
            ))
        } else {
            Ok(())
        }
    }

    fn case_clause(&mut self) -> Result<(), Error> {
        let Some(Block::Select {
            next,
            seen_case,
            seen_else,
            ..
        }) = self.blocks.last_mut()
        else {
            return Err(Error::new(self.line, "CASE outside SELECT CASE"));
        };
        if *seen_else {
            return Err(Error::new(self.line, "CASE after CASE ELSE"));
        }
        let had_case = std::mem::replace(seen_case, true);
        let previous_next = next.take();
        if had_case {
            let jump = self.emit(Op::Jump(0));
            if let Some(Block::Select { end_jumps, .. }) = self.blocks.last_mut() {
                end_jumps.push(jump);
            }
        }
        if let Some(previous) = previous_next {
            self.patch(previous, self.instructions.len());
        }
        if self.peek() == Some(&Token::Word("ELSE".into())) {
            self.next();
            self.emit(Op::Pop);
            if let Some(Block::Select { seen_else, .. }) = self.blocks.last_mut() {
                *seen_else = true;
            }
            return Ok(());
        }
        let mut hits = Vec::new();
        loop {
            self.emit(Op::Dup);
            if self.peek() == Some(&Token::Word("IS".into())) {
                self.next();
                let op = match self.next() {
                    Some(Token::Equal) => BinaryOp::Equal,
                    Some(Token::NotEqual) => BinaryOp::NotEqual,
                    Some(Token::Less) => BinaryOp::Less,
                    Some(Token::LessEqual) => BinaryOp::LessEqual,
                    Some(Token::Greater) => BinaryOp::Greater,
                    Some(Token::GreaterEqual) => BinaryOp::GreaterEqual,
                    _ => {
                        return Err(Error::new(
                            self.line,
                            "CASE IS requires a comparison operator",
                        ));
                    }
                };
                self.expression(4)?;
                self.emit(Op::Binary(op));
            } else {
                self.expression(4)?;
                if self.peek() == Some(&Token::Word("TO".into())) {
                    self.next();
                    self.emit(Op::Binary(BinaryOp::GreaterEqual));
                    self.emit(Op::Over);
                    self.expression(4)?;
                    self.emit(Op::Binary(BinaryOp::LessEqual));
                    self.emit(Op::Binary(BinaryOp::And));
                } else {
                    self.emit(Op::Binary(BinaryOp::Equal));
                }
            }
            hits.push(self.emit(Op::JumpIfTrue(0)));
            if self.peek() != Some(&Token::Comma) {
                break;
            }
            self.next();
        }
        let no_match = self.emit(Op::Jump(0));
        for hit in hits {
            self.patch(hit, self.instructions.len());
        }
        // Matched: discard the selector before running the clause body.
        self.emit(Op::Pop);
        if let Some(Block::Select { next, .. }) = self.blocks.last_mut() {
            *next = Some(no_match);
        }
        Ok(())
    }

    fn goto(&mut self) -> Result<(), Error> {
        if self.inside_try() {
            return Err(Error::new(
                self.line,
                "GOTO is not allowed inside a TRY, CATCH or FINALLY block",
            ));
        }
        let token = self
            .next()
            .ok_or_else(|| Error::new(self.line, "expected line number"))?;
        let label = line_number(&token, self.line)?;
        let index = self.emit(Op::Jump(0));
        let scope = self.current_function.as_ref().map(|context| context.index);
        self.jumps.push((index, label, self.line, scope));
        Ok(())
    }

    fn arguments(&mut self) -> Result<usize, Error> {
        self.expect(Token::LeftParen)?;
        let mut count = 0;
        if self.peek() != Some(&Token::RightParen) {
            loop {
                if matches!(
                    (self.peek(), self.tokens.get(self.position + 1)),
                    (Some(Token::Word(_)), Some(Token::ColonEqual))
                ) {
                    return Err(Error::new(
                        self.line,
                        "named arguments are only supported for user-defined functions",
                    ));
                }
                self.expression(0)?;
                count += 1;
                if self.peek() != Some(&Token::Comma) {
                    break;
                }
                self.next();
            }
        }
        self.expect(Token::RightParen)?;
        Ok(count)
    }

    fn call(&mut self, name: String, arguments: usize, discard_result: bool) {
        let index = self.emit(Op::Call {
            function: 0,
            arguments,
            discard_result,
            binding: None,
        });
        self.calls.push(PendingCall {
            index,
            name,
            user: false,
            method: None,
            arguments,
            line: self.line,
            file: self.file,
        });
    }

    /// Compiles a call: the callee name (already stripped of its module
    /// alias) and its parenthesised arguments, when present. Returns the
    /// record type of the result when the function declares one.
    fn call_site(
        &mut self,
        name: String,
        module: Option<(String, Rc<Exports>)>,
        discard_result: bool,
        has_parens: bool,
    ) -> Result<Option<usize>, Error> {
        let (display, signature) = match &module {
            Some((alias, exports)) => {
                let signature = exports.functions.get(&name).ok_or_else(|| {
                    Error::new(self.line, format!("module {alias} has no function {name}"))
                })?;
                (format!("{alias}.{name}"), Some(signature.clone()))
            }
            None => (name.clone(), self.signatures.get(&name).cloned()),
        };
        let Some(signature) = signature else {
            let count = if has_parens { self.arguments()? } else { 0 };
            self.call(name, count, discard_result);
            return Ok(None);
        };
        let (count, binding) = self.user_arguments(&signature, &display, has_parens, 0)?;
        let index = self.emit(Op::Call {
            function: 0,
            arguments: count,
            discard_result,
            binding,
        });
        let result = match signature.types.get(&signature.name) {
            Some(VarType::Record(id)) => Some(*id),
            _ => None,
        };
        self.calls.push(PendingCall {
            index,
            name: signature.qualified,
            user: true,
            method: None,
            arguments: count,
            line: self.line,
            file: self.file,
        });
        Ok(result)
    }

    /// After a `.`: whether a method call follows (`name(`, or a bare
    /// `name` ending a `CALL` statement).
    fn method_step(&self) -> bool {
        matches!(self.peek(), Some(Token::Word(_)))
            && (self.tokens.get(self.position + 1) == Some(&Token::LeftParen)
                || (self.statement_call && self.position + 1 == self.tokens.len()))
    }

    /// Compiles a method call on the receiver already on the operand stack
    /// (the method name is next). Calls on a receiver of known class are
    /// resolved now; others by the receiver's runtime class. Returns the
    /// record type of the result when it is statically known.
    fn method_call(&mut self, record: Option<usize>) -> Result<Option<usize>, Error> {
        let Some(Token::Word(name)) = self.next() else {
            return Err(Error::new(self.line, "expected a method name after '.'"));
        };
        if name == "NEW" {
            return Err(Error::new(
                self.line,
                "a constructor only runs through NEW Class(...); it cannot be called directly",
            ));
        }
        let has_parens = self.peek() == Some(&Token::LeftParen);
        let index;
        let mut result = None;
        if let Some(id) = record {
            let types = self.types;
            let class = &types[id];
            let Some(signature) = self
                .methods
                .get(&id)
                .and_then(|methods| methods.get(&name))
                .cloned()
            else {
                return Err(Error::new(
                    self.line,
                    format!(
                        "{} {} has no method {name}",
                        if class.is_class {
                            "class"
                        } else {
                            "record type"
                        },
                        class.name
                    ),
                ));
            };
            let display = format!("{}.{name}", class.name);
            let (count, binding) = self.user_arguments(&signature, &display, has_parens, 1)?;
            index = self.emit(Op::Call {
                function: 0,
                arguments: count,
                discard_result: false,
                binding,
            });
            if let Some(VarType::Record(next)) = signature.types.get(&signature.name) {
                result = Some(*next);
            }
            self.calls.push(PendingCall {
                index,
                name: signature.qualified,
                user: true,
                method: Some((id, name)),
                arguments: count,
                line: self.line,
                file: self.file,
            });
        } else {
            let arguments = if has_parens { self.arguments()? } else { 0 };
            index = self.emit(Op::CallMethod {
                name,
                arguments,
                discard_result: false,
            });
        }
        self.last_method_call = Some(index);
        Ok(result)
    }

    /// Parses positional, named (`name := value`) and BYREF arguments of a
    /// user function and checks them against its signature. For a method
    /// (`skip == 1`) the receiver is already on the operand stack as the
    /// first argument, bound to the leading `SELF` parameter.
    fn user_arguments(
        &mut self,
        signature: &Signature,
        display: &str,
        has_parens: bool,
        skip: usize,
    ) -> Result<(usize, Option<Arc<CallBinding>>), Error> {
        let total = signature.parameters.len();
        let arity = |given: usize| {
            format!(
                "function {display} requires {} arguments, got {}",
                arity_text(signature.required() - skip, total - skip),
                given - skip
            )
        };
        let mut slots: Vec<usize> = (0..skip).collect();
        let mut refs: Vec<RefArg> = Vec::new();
        let mut named = false;
        if has_parens {
            self.expect(Token::LeftParen)?;
            if self.peek() != Some(&Token::RightParen) {
                loop {
                    let name = match (self.peek(), self.tokens.get(self.position + 1)) {
                        (Some(Token::Word(name)), Some(Token::ColonEqual)) => {
                            let name = name.clone();
                            self.position += 2;
                            Some(name)
                        }
                        _ => None,
                    };
                    let slot = match name {
                        Some(name) => {
                            named = true;
                            let slot = signature
                                .parameters
                                .iter()
                                .position(|parameter| *parameter == name)
                                .filter(|slot| *slot >= skip)
                                .ok_or_else(|| {
                                    Error::new(
                                        self.line,
                                        format!("function {display} has no parameter {name}"),
                                    )
                                })?;
                            if slots.contains(&slot) {
                                return Err(Error::new(
                                    self.line,
                                    format!("parameter {name} of {display} is given twice"),
                                ));
                            }
                            slot
                        }
                        None => {
                            if named {
                                return Err(Error::new(
                                    self.line,
                                    "a positional argument cannot follow a named argument",
                                ));
                            }
                            if slots.len() >= total {
                                // Count the remaining arguments for the message.
                                let mut given = slots.len() + 1;
                                let mut depth = 0usize;
                                for token in &self.tokens[self.position..] {
                                    match token {
                                        Token::LeftParen | Token::LeftBracket => depth += 1,
                                        Token::RightParen | Token::RightBracket => {
                                            if depth == 0 {
                                                break;
                                            }
                                            depth -= 1;
                                        }
                                        Token::Comma if depth == 0 => given += 1,
                                        _ => {}
                                    }
                                }
                                return Err(Error::new(self.line, arity(given)));
                            }
                            slots.len()
                        }
                    };
                    if signature.infos[slot].by_ref {
                        let place = self.place_argument()?;
                        refs.push(RefArg {
                            arg: slots.len(),
                            place,
                        });
                    } else {
                        self.expression(0)?;
                    }
                    slots.push(slot);
                    if self.peek() != Some(&Token::Comma) {
                        break;
                    }
                    self.next();
                }
            }
            self.expect(Token::RightParen)?;
        }
        if named {
            if let Some(missing) =
                (0..total).find(|slot| !signature.infos[*slot].optional && !slots.contains(slot))
            {
                return Err(Error::new(
                    self.line,
                    format!(
                        "function {display} is missing required parameter {}",
                        signature.parameters[missing]
                    ),
                ));
            }
        } else if slots.len() < signature.required() {
            return Err(Error::new(self.line, arity(slots.len())));
        }
        let identity = slots.iter().enumerate().all(|(index, slot)| index == *slot);
        let binding = (!identity || !refs.is_empty()).then(|| {
            Arc::new(CallBinding {
                slots: if identity { Vec::new() } else { slots.clone() },
                refs,
            })
        });
        Ok((slots.len(), binding))
    }

    /// Parses a BYREF argument: a variable, array element or record field.
    /// Pushes the place's operands, then the current value.
    fn place_argument(&mut self) -> Result<Place, Error> {
        let line = self.line;
        let invalid = || {
            Error::new(
                line,
                "a BYREF argument must be a variable, array element or record field",
            )
        };
        let Some(Token::Word(first)) = self
            .next()
            .filter(|t| matches!(t, Token::Word(w) if !is_keyword(w)))
        else {
            return Err(invalid());
        };
        let (name, qualifier) = match self.qualified_member(&first)? {
            Some((alias, exports, member)) => {
                if !exports.members.contains(&member) {
                    return Err(Error::new(
                        self.line,
                        format!("module {alias} has no variable {member}"),
                    ));
                }
                if exports.constants.contains(&member) {
                    return Err(Error::new(
                        self.line,
                        format!("cannot pass constant {member} BYREF"),
                    ));
                }
                (
                    format!("{}.{member}", exports.prefix),
                    Some((exports, member)),
                )
            }
            None => {
                self.check_assignable(&first)?;
                self.require_declared(&first)?;
                (first, None)
            }
        };
        let postfix = matches!(self.peek(), Some(Token::LeftBracket | Token::Dot));
        if postfix {
            self.emit(Op::Load(name.clone()));
        }
        let record = match &qualifier {
            Some((exports, member)) => match exports.var_types.get(member) {
                Some(VarType::Record(id)) => Some(*id),
                _ => None,
            },
            None => self.static_record_type(&name),
        };
        let target = if postfix {
            self.postfix_steps(record)?
        } else {
            Target::Variable
        };
        if matches!(target, Target::Call)
            || !matches!(self.peek(), Some(Token::Comma | Token::RightParen))
        {
            return Err(invalid());
        }
        Ok(match target {
            Target::Call => unreachable!("rejected above"),
            Target::Variable => {
                self.emit(Op::Load(name.clone()));
                Place::Variable(name)
            }
            Target::Index => {
                self.emit(Op::Dup2);
                self.emit(Op::IndexGet);
                Place::Element
            }
            Target::Field(field) => {
                self.emit(Op::Dup);
                self.emit(Op::GetField(field.clone()));
                Place::Field(field)
            }
        })
    }

    /// `DIM v AS Class` / `LOCAL v AS Class` without an initializer creates an
    /// instance; for a class with a constructor that means running it with no
    /// arguments. Returns true when the instance was emitted (the declaration
    /// then has an initializer).
    fn default_instance(&mut self, ty: VarType, has_init: bool) -> Result<bool, Error> {
        let VarType::Record(id) = ty else {
            return Ok(has_init);
        };
        if has_init {
            return Ok(true);
        }
        let Some(constructor) = self
            .methods
            .get(&id)
            .and_then(|methods| methods.get("NEW"))
            .cloned()
        else {
            return Ok(false);
        };
        let class = self.types[id].name.clone();
        if constructor.required() > 1 {
            return Err(Error::new(
                self.line,
                format!(
                    "the constructor of CLASS {class} requires arguments; \
                     declare with = NEW {class}(...)"
                ),
            ));
        }
        self.emit_construction(id, &constructor, false)?;
        Ok(true)
    }

    /// Emits the creation of an instance of class `ty` followed by its
    /// constructor call; the new object is left on the operand stack.
    fn emit_construction(
        &mut self,
        ty: usize,
        constructor: &Signature,
        has_parens: bool,
    ) -> Result<(), Error> {
        self.emit(Op::NewRecord {
            ty,
            fields: Vec::new(),
        });
        self.emit(Op::Dup);
        let display = format!("{}.NEW", self.types[ty].name);
        let (count, binding) = self.user_arguments(constructor, &display, has_parens, 1)?;
        let index = self.emit(Op::Call {
            function: 0,
            arguments: count,
            discard_result: true,
            binding,
        });
        self.calls.push(PendingCall {
            index,
            name: constructor.qualified.clone(),
            user: true,
            method: Some((ty, "NEW".to_string())),
            arguments: count,
            line: self.line,
            file: self.file,
        });
        Ok(())
    }

    /// The rest of `NEW Type [(field = value, ...)]` (type name consumed).
    /// For a class with a constructor the parenthesised list holds the
    /// constructor's arguments instead.
    fn new_record(&mut self, ty: usize) -> Result<(), Error> {
        if let Some(constructor) = self
            .methods
            .get(&ty)
            .and_then(|methods| methods.get("NEW"))
            .cloned()
        {
            let has_parens = self.peek() == Some(&Token::LeftParen);
            return self.emit_construction(ty, &constructor, has_parens);
        }
        let mut fields: Vec<usize> = Vec::new();
        if self.peek() == Some(&Token::LeftParen) {
            self.next();
            if self.peek() != Some(&Token::RightParen) {
                loop {
                    let Some(Token::Word(field)) = self.next() else {
                        return Err(Error::new(self.line, "expected a field name in NEW"));
                    };
                    let definition = &self.types[ty];
                    let Some(index) = definition.fields.iter().position(|f| f.name == field) else {
                        return Err(Error::new(
                            self.line,
                            format!("record type {} has no field {field}", definition.name),
                        ));
                    };
                    if fields.contains(&index) {
                        return Err(Error::new(
                            self.line,
                            format!("field {field} is initialised twice"),
                        ));
                    }
                    self.expect(Token::Equal)?;
                    self.expression(0)?;
                    fields.push(index);
                    if self.peek() != Some(&Token::Comma) {
                        break;
                    }
                    self.next();
                }
            }
            self.expect(Token::RightParen)?;
        }
        self.emit(Op::NewRecord { ty, fields });
        Ok(())
    }

    fn expression(&mut self, minimum: u8) -> Result<(), Error> {
        // The record type of the value just parsed, when statically known.
        let mut record: Option<usize> = None;
        match self.next() {
            Some(Token::Number(n)) => {
                self.emit(Op::Number(n));
            }
            Some(Token::Int(n)) => {
                self.emit(Op::Int(n));
            }
            Some(Token::MinInt) => {
                return Err(Error::new(
                    self.line,
                    "integer literal 9223372036854775808& is out of range (allowed only as -9223372036854775808&)",
                ));
            }
            Some(Token::String(text)) => {
                self.emit(Op::String(text));
            }
            Some(Token::Word(name))
                if matches!(name.as_str(), "ARRAY" | "LIST" | "MAP" | "LEN" | "MOD") =>
            {
                let count = self.arguments()?;
                let op = match name.as_str() {
                    "MOD" if count == 2 => Op::Binary(BinaryOp::Modulo),
                    "MOD" => return Err(Error::new(self.line, "MOD requires two arguments")),
                    "LEN" if count == 1 => Op::Length,
                    "LEN" => return Err(Error::new(self.line, "LEN requires one argument")),
                    "MAP" if count % 2 != 0 => {
                        return Err(Error::new(self.line, "MAP requires key/value pairs"));
                    }
                    "MAP" => Op::Collection(CollectionKind::Map, count),
                    "ARRAY" => Op::Collection(CollectionKind::Array, count),
                    "LIST" => Op::Collection(CollectionKind::List, count),
                    _ => unreachable!(),
                };
                self.emit(op);
            }
            Some(Token::Word(name)) if name == "TRUE" || name == "FALSE" => {
                self.emit(Op::Number(if name == "TRUE" { 1.0 } else { 0.0 }));
            }
            Some(Token::Word(name)) if name == "NOT" => {
                // NOT binds looser than comparisons and arithmetic, tighter than AND.
                self.expression(PRECEDENCE_COMPARISON)?;
                self.emit(Op::Not);
            }
            Some(Token::Word(name)) if name == "NOTHING" => {
                self.emit(Op::Nothing);
            }
            Some(Token::Word(name))
                if name == "NEW"
                    && qualified_name(&self.tokens[self.position..])
                        .is_some_and(|(key, _)| self.type_ids.contains_key(&key)) =>
            {
                let (type_name, used) = qualified_name(&self.tokens[self.position..])
                    .expect("guard checked the type name");
                self.position += used;
                let ty = self.type_ids[&type_name];
                self.new_record(ty)?;
                record = Some(ty);
            }
            Some(Token::Word(name)) if !is_keyword(&name) => {
                if let Some((alias, exports, member)) = self.qualified_member(&name)? {
                    if self.peek() == Some(&Token::LeftParen) {
                        record = self.call_site(member, Some((alias, exports)), false, true)?;
                    } else {
                        if !exports.members.contains(&member) {
                            return Err(Error::new(
                                self.line,
                                format!("module {alias} has no variable or constant {member}"),
                            ));
                        }
                        if let Some(VarType::Record(id)) = exports.var_types.get(&member) {
                            record = Some(*id);
                        }
                        self.emit(Op::Load(format!("{}.{member}", exports.prefix)));
                    }
                } else if self.peek() == Some(&Token::LeftParen) {
                    record = self.call_site(name, None, false, true)?;
                } else {
                    self.require_declared(&name)?;
                    record = self.static_record_type(&name);
                    self.emit(Op::Load(name));
                }
            }
            Some(Token::Minus)
                if self.peek() == Some(&Token::MinInt)
                    && self.tokens.get(self.position + 1) != Some(&Token::Caret) =>
            {
                self.next();
                self.emit(Op::Int(i64::MIN));
            }
            Some(Token::Minus) => {
                self.expression(PRECEDENCE_UNARY)?;
                self.emit(Op::Negate);
            }
            Some(Token::Plus) => self.expression(PRECEDENCE_UNARY)?,
            Some(Token::LeftParen) => {
                self.expression(0)?;
                self.expect(Token::RightParen)?;
            }
            _ => return Err(Error::new(self.line, "expected expression")),
        }
        loop {
            match self.peek() {
                Some(Token::LeftBracket) => {
                    self.next();
                    self.expression(0)?;
                    while self.peek() == Some(&Token::Comma) {
                        self.next();
                        self.emit(Op::IndexGet);
                        self.expression(0)?;
                    }
                    self.expect(Token::RightBracket)?;
                    self.emit(Op::IndexGet);
                    record = None;
                }
                Some(Token::Dot) => {
                    self.next();
                    if self.method_step() {
                        record = self.method_call(record)?;
                    } else {
                        let (field, next) = self.field_access(record)?;
                        self.emit(Op::GetField(field));
                        record = next;
                    }
                }
                _ => break,
            }
        }
        loop {
            if self.peek() == Some(&Token::Word("IS".into())) {
                let negated =
                    self.tokens.get(self.position + 1) == Some(&Token::Word("NOT".into()));
                let nothing = self.tokens.get(self.position + 1 + usize::from(negated))
                    == Some(&Token::Word("NOTHING".into()));
                if !nothing || PRECEDENCE_COMPARISON < minimum {
                    break;
                }
                self.position += 2 + usize::from(negated);
                self.emit(Op::Nothing);
                self.emit(Op::Binary(if negated {
                    BinaryOp::NotEqual
                } else {
                    BinaryOp::Equal
                }));
                continue;
            }
            let short_circuit = match self.peek() {
                Some(Token::Word(word)) if word == "ANDALSO" => Some((PRECEDENCE_AND, true)),
                Some(Token::Word(word)) if word == "ORELSE" => Some((PRECEDENCE_OR, false)),
                _ => None,
            };
            if let Some((precedence, is_and)) = short_circuit {
                if precedence < minimum {
                    break;
                }
                self.next();
                let jump = self.emit(if is_and {
                    Op::AndJump(0)
                } else {
                    Op::OrJump(0)
                });
                self.expression(precedence + 1)?;
                self.emit(Op::Truth);
                self.patch(jump, self.instructions.len());
                continue;
            }
            let Some((precedence, op)) = self.peek().and_then(binary) else {
                break;
            };
            if precedence < minimum {
                break;
            }
            self.next();
            // `^` is right-associative: 2 ^ 3 ^ 2 is 2 ^ (3 ^ 2).
            self.expression(if op == BinaryOp::Power {
                precedence
            } else {
                precedence + 1
            })?;
            self.emit(Op::Binary(op));
        }
        Ok(())
    }
}

const MAX_DIMENSIONS: usize = 8;
const PRECEDENCE_OR: u8 = 1;
const PRECEDENCE_AND: u8 = 2;
const PRECEDENCE_COMPARISON: u8 = 3;
const PRECEDENCE_UNARY: u8 = 6;
const PRECEDENCE_POWER: u8 = 7;

fn binary(token: &Token) -> Option<(u8, BinaryOp)> {
    Some(match token {
        Token::Equal => (3, BinaryOp::Equal),
        Token::NotEqual => (3, BinaryOp::NotEqual),
        Token::Less => (3, BinaryOp::Less),
        Token::LessEqual => (3, BinaryOp::LessEqual),
        Token::Greater => (3, BinaryOp::Greater),
        Token::GreaterEqual => (3, BinaryOp::GreaterEqual),
        Token::Plus => (4, BinaryOp::Add),
        Token::Minus => (4, BinaryOp::Subtract),
        Token::Star => (5, BinaryOp::Multiply),
        Token::Slash => (5, BinaryOp::Divide),
        Token::Backslash => (5, BinaryOp::IntDivide),
        Token::Caret => (PRECEDENCE_POWER, BinaryOp::Power),
        Token::Word(word) if word == "MOD" => (5, BinaryOp::Modulo),
        Token::Word(word) if word == "AND" => (PRECEDENCE_AND, BinaryOp::And),
        Token::Word(word) if word == "OR" => (PRECEDENCE_OR, BinaryOp::Or),
        _ => return None,
    })
}

fn is_keyword(name: &str) -> bool {
    matches!(
        name,
        "LET"
            | "PRINT"
            | "IF"
            | "THEN"
            | "ELSEIF"
            | "ELSE"
            | "GOTO"
            | "END"
            | "STOP"
            | "INPUT"
            | "GC"
            | "ARRAY"
            | "LIST"
            | "MAP"
            | "LEN"
            | "MOD"
            | "APPEND"
            | "REMOVE"
            | "FOR"
            | "EACH"
            | "IN"
            | "TO"
            | "STEP"
            | "NEXT"
            | "WHILE"
            | "WEND"
            | "DO"
            | "LOOP"
            | "UNTIL"
            | "EXIT"
            | "CONTINUE"
            | "SELECT"
            | "CASE"
            | "IS"
            | "CONST"
            | "TRUE"
            | "FALSE"
            | "AND"
            | "OR"
            | "NOT"
            | "ANDALSO"
            | "ORELSE"
            | "FUNCTION"
            | "FEND"
            | "LOCAL"
            | "PARAMETER"
            | "CALL"
            | "NOTHING"
    )
}
