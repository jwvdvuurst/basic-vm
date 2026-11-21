//! Phase 4B: records (TYPE ... END TYPE), NEW, fields, NOTHING, COPY, GC.

use std::io::Cursor;
use std::process::Command;

use basic_vm::{Error, Vm, compile};

const PERSON: &str = "TYPE Person\n name$ AS STRING = \"anon\"\n age AS INTEGER\n \
     score AS DOUBLE = 1.5\n ok AS BOOLEAN = TRUE\n boss AS Person\nEND TYPE\n";

fn run(source: &str) -> (String, Result<(), Error>) {
    let mut vm = Vm::new(compile(source).unwrap()).unwrap();
    let mut output = Vec::new();
    let result = vm.run(&mut Cursor::new(""), &mut output);
    (String::from_utf8(output).unwrap(), result)
}

fn ok(source: &str) -> String {
    let (output, result) = run(source);
    result.unwrap_or_else(|error| panic!("{error}\n{source}"));
    output
}

fn with_person(body: &str) -> String {
    format!("{PERSON}{body}")
}

fn runtime_error(source: &str) -> String {
    run(source).1.unwrap_err().message
}

fn compile_error(source: &str) -> String {
    match compile(source) {
        Err(error) => error.message,
        Ok(_) => panic!("expected a compile error\n{source}"),
    }
}

#[test]
fn defaults_and_field_read_write() {
    let out = ok(&with_person(
        "DIM p AS Person\nPRINT p.name$; \"|\"; p.age; \"|\"; p.score; \"|\"; p.ok\n\
         p.name$ = \"Zed\"\np.age = p.age + 7\nPRINT p.name$; p.age\n\
         PRINT p.boss IS NOTHING\nPRINT p.boss = NOTHING",
    ));
    assert_eq!(out, "anon|0|1.5|1\nZed7\n1\n1\n");
}

#[test]
fn type_declared_after_use_and_new_with_initialisers() {
    let out = ok(
        "DIM p AS Pt = NEW Pt(y = 4, x = 2)\nPRINT p.x; p.y\nTYPE Pt\n x AS INTEGER\n y AS INTEGER\nEND TYPE",
    );
    assert_eq!(out, "24\n");
}

#[test]
fn field_type_enforcement() {
    let msg = runtime_error(&with_person("DIM p AS Person\np.age = 1.5"));
    assert!(
        msg.contains("PERSON.AGE") && msg.contains("INTEGER"),
        "{msg}"
    );
    let msg = runtime_error(&with_person("DIM p AS Person\np.name$ = 3"));
    assert!(msg.contains("PERSON.NAME$"), "{msg}");
    let msg = runtime_error(&with_person("DIM p AS Person\np.boss = 3"));
    assert!(msg.contains("PERSON.BOSS"), "{msg}");
    assert!(
        msg.contains("declared AS PERSON") && msg.contains("cannot assign the number 3"),
        "{msg}"
    );
    assert!(!msg.contains("outside"), "{msg}");
    let msg = runtime_error(&with_person("DIM p AS Person\np.boss = 5.5"));
    assert!(msg.contains("cannot assign the number 5.5"), "{msg}");
}

#[test]
fn stray_colon_equals_has_a_friendly_message() {
    assert!(compile_error("X := 5").contains("only valid in named arguments"));
    assert!(compile_error(":= 5").contains("only valid in named arguments"));
}

#[test]
fn unknown_fields_are_errors() {
    let msg = compile_error(&with_person("DIM p AS Person\nPRINT p.height"));
    assert!(msg.contains("no field"), "{msg}");
    let msg = compile_error(&with_person("DIM p AS Person\np.height = 1"));
    assert!(msg.contains("no field"), "{msg}");
    let msg = runtime_error(&with_person(
        "DIM l AS LIST\nAPPEND l, NEW Person\nPRINT l[1].height",
    ));
    assert!(msg.contains("no field"), "{msg}");
    let msg = compile_error("DIM p AS Nope");
    assert!(!msg.is_empty());
}

#[test]
fn nested_records_and_chains() {
    let out = ok(&with_person(
        "DIM a AS Person\nDIM b AS Person\nDIM c AS Person\na.boss = b\nb.boss = c\n\
         a.boss.boss.age = 5\nPRINT c.age\nPRINT a.boss.boss.name$",
    ));
    assert_eq!(out, "5\nanon\n");
}

#[test]
fn aliasing_versus_copy() {
    let out = ok(&with_person(
        "DIM a AS Person\nDIM b AS Person\nb = a\nb.age = 9\nPRINT a.age\n\
         DIM c AS Person\nc = COPY(a)\nc.age = 1\nPRINT a.age; c.age\n\
         PRINT a = b; a = c; a <> c\n\
         DIM x AS Person\nDIM y AS Person\nx.age = 2\nPRINT y.age",
    ));
    assert_eq!(out, "9\n91\n101\n0\n");
}

#[test]
fn copy_is_shallow_for_nested_records() {
    let out = ok(&with_person(
        "DIM a AS Person\na.boss = NEW Person\nDIM c AS Person\nc = COPY(a)\nc.boss.age = 4\nPRINT a.boss.age",
    ));
    assert_eq!(out, "4\n");
}

#[test]
fn records_in_collections_sort_pass_and_return() {
    let out = ok(&with_person(
        "FUNCTION Make(PARAMETER n$ AS STRING, PARAMETER a AS INTEGER) AS Person\n\
           LOCAL p AS Person\n p.name$ = n$\n p.age = a\n Make = p\nFEND\n\
         FUNCTION Birthday(PARAMETER p AS Person)\n p.age = p.age + 1\nFEND\n\
         DIM m AS MAP\nm[\"a\"] = Make(\"A\", 30)\nm[\"b\"] = Make(\"B\", 20)\n\
         CALL Birthday(m[\"b\"])\nPRINT m[\"b\"].age; m[\"a\"].name$\n\
         DIM l AS LIST\nAPPEND l, Make(\"X\", 3)\nAPPEND l, Make(\"Y\", 1)\nAPPEND l, Make(\"Z\", 2)\n\
         FOR i = 1 TO LEN(l) - 1\n FOR j = 1 TO LEN(l) - i\n \
           IF l[j].age > l[j + 1].age THEN\n  t = l[j]\n  l[j] = l[j + 1]\n  l[j + 1] = t\n END IF\n NEXT j\nNEXT i\n\
         FOR i = 1 TO 3\n PRINT l[i].name$;\nNEXT i\nPRINT",
    ));
    assert_eq!(out, "21A\nYZX\n");
}

#[test]
fn typed_parameter_rejects_wrong_values() {
    let msg = runtime_error(&with_person(
        "FUNCTION F(PARAMETER p AS Person)\nFEND\nCALL F(3)",
    ));
    assert!(msg.contains("PERSON") || msg.contains("RECORD"), "{msg}");
}

#[test]
fn nothing_dereference_and_misuse() {
    let msg = runtime_error(&with_person("DIM p AS Person\nPRINT p.boss.age"));
    assert!(msg.to_uppercase().contains("NOTHING"), "{msg}");
    let msg = runtime_error(&with_person("DIM p AS Person\np.boss.age = 1"));
    assert!(msg.to_uppercase().contains("NOTHING"), "{msg}");
    let msg = runtime_error(&with_person("DIM p AS Person\nPRINT p"));
    assert!(msg.contains("PRINT"), "{msg}");
    let msg = runtime_error(&with_person("DIM p AS Person\nPRINT p + 1"));
    assert!(!msg.is_empty());
    let out = ok(&with_person(
        "DIM p AS Person\nPRINT p IS NOTHING; p IS NOT NOTHING\np = NOTHING\nPRINT p IS NOTHING",
    ));
    assert_eq!(out, "01\n1\n");
}

#[test]
fn typeof_typename_fields_and_reflection() {
    let out = ok(&with_person(
        "DIM p AS Person\nPRINT TYPEOF(p); TYPENAME$(p); ISRECORD(p); ISRECORD(3)\n\
         PRINT TYPEOF(p.boss); TYPEOF(NOTHING)\n\
         DIM f AS LIST = FIELDS(p)\nPRINT LEN(f); f[1]; f[5]\n\
         SETFIELD p, \"age\", 12\nPRINT GETFIELD(p, \"AGE\"); p.age",
    ));
    assert_eq!(out, "RECORDPERSON10\nNOTHINGNOTHING\n5NAME$BOSS\n1212\n");
}

#[test]
fn getfield_unknown_name_fails() {
    let msg = runtime_error(&with_person("DIM p AS Person\nPRINT GETFIELD(p, \"zip\")"));
    assert!(msg.contains("no field"), "{msg}");
}

#[test]
fn cyclic_records_are_reclaimed() {
    let source = "TYPE N\n next AS N\n v AS INTEGER\nEND TYPE\n\
        FOR i = 1 TO 2000\n a = NEW N(v = i)\n b = NEW N\n a.next = b\n b.next = a\nNEXT i\n\
        a = NOTHING\nb = NOTHING\nGC\n";
    let mut vm = Vm::new(compile(source).unwrap()).unwrap();
    let mut out = Vec::new();
    vm.run(&mut Cursor::new(""), &mut out).unwrap();
    let stats = vm.heap_stats();
    assert!(stats.collections > 0);
    assert_eq!(stats.live_allocations, 0, "{stats:?}");
}

#[test]
fn live_records_survive_collection() {
    let out = ok(&with_person(
        "DIM l AS LIST\nFOR i = 1 TO 500\n DIM p AS Person\n p.age = i\n p.name$ = \"n\" + STR$(i)\n APPEND l, p\nNEXT i\n\
         GC\nPRINT l[1].age; l[500].age; l[250].name$",
    ));
    assert_eq!(out, "1500n250\n");
}

#[test]
fn legacy_programs_unaffected() {
    let out = ok("TYPE = 3\nPRINT TYPE\nNEW = 2\nPRINT NEW + .5\nX = .25\nPRINT X + 1.5");
    assert_eq!(out, "3\n2.5\n1.75\n");
}

#[test]
fn declaration_errors() {
    assert!(compile("TYPE A\n x AS INTEGER").is_err());
    assert!(compile("TYPE A\n x AS INTEGER\n X AS INTEGER\nEND TYPE").is_err());
    assert!(compile("TYPE A\nEND TYPE\nTYPE A\nEND TYPE").is_err());
    assert!(compile("SUB F()\nTYPE A\nEND TYPE\nEND SUB").is_err());
    assert!(compile("TYPE A\n s AS STRING\nEND TYPE").is_err());
}

#[test]
fn const_defaults_and_option_explicit() {
    let out = ok(
        "OPTION EXPLICIT\nCONST LIMIT = 10\nTYPE T\n n AS INTEGER = LIMIT\nEND TYPE\n\
         DIM t AS T\nPRINT t.n",
    );
    assert_eq!(out, "10\n");
    let msg = compile_error("OPTION EXPLICIT\nTYPE T\n n AS INTEGER\nEND TYPE\nt = NEW T");
    assert!(!msg.is_empty());
}

#[test]
fn records_example_runs_with_bounded_output() {
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .arg("examples/records.bas")
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with("Bob is 29, boss: Ann\n"), "{text}");
    assert!(text.lines().count() < 40);
}
