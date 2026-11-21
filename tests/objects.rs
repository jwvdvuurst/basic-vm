//! Phase 5: objects (CLASS = record type + methods), SELF, constructors.

use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use basic_vm::{Error, Vm, compile, compile_file};

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

fn runtime_error(source: &str) -> String {
    run(source)
        .1
        .expect_err("expected a runtime error")
        .to_string()
}

fn compile_error(source: &str) -> String {
    match compile(source) {
        Err(error) => error.to_string(),
        Ok(_) => panic!("expected a compile error\n{source}"),
    }
}

/// Runs `source` and returns the number of live heap allocations afterwards.
fn live_after(source: &str) -> usize {
    let mut vm = Vm::new(compile(source).unwrap()).unwrap();
    let mut out = Vec::new();
    vm.run(&mut Cursor::new(""), &mut out)
        .unwrap_or_else(|error| panic!("{error}"));
    vm.heap_stats().live_allocations
}

const COUNTER: &str = "CLASS Counter\n n AS INTEGER\n step AS INTEGER = 1\n \
    FUNCTION Bump() AS INTEGER\n  SELF.n = SELF.n + SELF.step\n  Bump = SELF.n\n FEND\n \
    FUNCTION Reset()\n  SELF.n = 0\n FEND\nEND CLASS\n";

fn with_counter(body: &str) -> String {
    format!("{COUNTER}{body}")
}

#[test]
fn instances_are_isolated() {
    let out = ok(&with_counter(
        "DIM a AS Counter\nDIM b AS Counter\nb.step = 10\n\
         x = a.Bump()\nx = a.Bump()\ny = b.Bump()\nPRINT a.n; \",\"; b.n; \",\"; y",
    ));
    assert_eq!(out, "2,10,10\n");
}

#[test]
fn methods_resolve_statically_and_dynamically() {
    let out = ok(&with_counter(
        "DIM a AS Counter\nDIM l AS LIST\nAPPEND l, a\nAPPEND l, NEW Counter(step = 5)\n\
         x = l[1].Bump()\nx = l[2].Bump()\nx = l[2].Bump()\n\
         v = l[2]\nz = v.Bump()\nPRINT a.n; \",\"; l[2].n; \",\"; z\n\
         CALL l[1].Reset()\nCALL a.Bump()\nCALL a.Bump\nPRINT a.n\nCALL a.Reset\nPRINT a.n",
    ));
    assert_eq!(out, "1,15,15\n2\n0\n");
}

#[test]
fn constructors_positional_named_optional() {
    let src = "CLASS P\n name$ AS STRING\n age AS INTEGER\n \
        FUNCTION NEW(PARAMETER n$ AS STRING, PARAMETER a AS INTEGER = 7)\n  \
        SELF.name$ = n$\n  SELF.age = a\n FEND\nEND CLASS\n\
        DIM p AS P = NEW P(\"ann\")\nDIM q AS P = NEW P(a := 3, n$ := \"bob\")\n\
        PRINT p.name$; \",\"; p.age; \",\"; q.name$; \",\"; q.age";
    assert_eq!(ok(src), "ann,7,bob,3\n");
}

#[test]
fn constructor_error_is_catchable_and_does_not_leak() {
    let src = "CLASS P\n x AS INTEGER\n FUNCTION NEW(PARAMETER v)\n  \
        IF v < 0 THEN THROW \"neg\"\n  SELF.x = v\n FEND\nEND CLASS\n\
        c = 0\nFOR i = 1 TO 500\n TRY\n  p = NEW P(-1)\n CATCH\n  c = c + 1\n END TRY\nNEXT\n\
                GC\nIF c <> 500 THEN THROW \"count\"\n";
    assert_eq!(live_after(src), 0);
}

#[test]
fn new_inside_method_and_returned_instances() {
    let src = "CLASS Person\n name$ AS STRING\n FUNCTION NEW(PARAMETER n$)\n  SELF.name$ = n$\n FEND\n \
        FUNCTION Clone() AS Person\n  Clone = NEW Person(SELF.name$ + \"'\")\n FEND\nEND CLASS\n\
        DIM a AS Person = NEW Person(\"x\")\nDIM b AS Person = a.Clone().Clone()\nPRINT b.name$; \"|\"; a.name$";
    assert_eq!(ok(src), "x''|x\n");
}

#[test]
fn methods_call_methods_and_recurse() {
    let src = "CLASS M\n k AS INTEGER\n \
        FUNCTION Fact(PARAMETER n) AS INTEGER\n  IF n <= 1 THEN\n   Fact = 1\n  ELSE\n   Fact = n * SELF.Fact(n - 1)\n  END IF\n FEND\n \
        FUNCTION Twice(PARAMETER n) AS INTEGER\n  Twice = SELF.Fact(n) * 2\n FEND\nEND CLASS\n\
        DIM m AS M\nPRINT m.Twice(5)";
    assert_eq!(ok(src), "240\n");
}

#[test]
fn composition_and_list_elements() {
    let src = "CLASS Engine\n hp AS INTEGER = 50\n FUNCTION Boost()\n  SELF.hp = SELF.hp + 10\n FEND\nEND CLASS\n\
        CLASS Car\n e AS Engine\n FUNCTION NEW()\n  SELF.e = NEW Engine()\n FEND\n \
        FUNCTION Go()\n  CALL SELF.e.Boost()\n FEND\nEND CLASS\n\
        DIM c AS Car = NEW Car()\nCALL c.Go()\nCALL c.Go()\n\
        DIM l AS LIST\nAPPEND l, c\nCALL l[1].Go()\nPRINT c.e.hp";
    assert_eq!(ok(src), "80\n");
}

#[test]
fn cyclic_parents_and_children_are_reclaimed() {
    let src = "CLASS Node\n other AS Node\n FUNCTION Link(PARAMETER o AS Node)\n  SELF.other = o\n FEND\nEND CLASS\n\
        FOR i = 1 TO 2000\n DIM a AS Node\n DIM b AS Node\n CALL a.Link(b)\n CALL b.Link(a)\nNEXT\n\
        a = NOTHING\nb = NOTHING\nGC\n";
    assert_eq!(live_after(src), 0);
}

#[test]
fn locals_are_isolated_across_nested_calls_on_different_instances() {
    let src = "CLASS A\n id AS INTEGER\n peer AS A\n \
        FUNCTION Run(PARAMETER d) AS INTEGER\n  LOCAL t AS INTEGER = SELF.id * 100\n  \
        IF d > 0 THEN\n   x = SELF.peer.Run(d - 1)\n  END IF\n  Run = t + SELF.id\n FEND\nEND CLASS\n\
        DIM a AS A\nDIM b AS A\na.id = 1\nb.id = 2\na.peer = b\nb.peer = a\nPRINT a.Run(1)";
    assert_eq!(ok(src), "101\n");
}

#[test]
fn receiver_stays_alive_when_caller_drops_it() {
    let src = "CLASS T\n v AS INTEGER = 9\n FUNCTION Probe() AS INTEGER\n  g = NOTHING\n  \
          LOCAL s AS INTEGER\n  FOR i = 1 TO 20000\n   s = s + 1\n  NEXT\n  GC\n  Probe = SELF.v\n FEND\nEND CLASS\n\
        g = NEW T()\nPRINT g.Probe()";
    assert_eq!(ok(src), "9\n");
}

#[test]
fn runtime_error_cases() {
    let e = runtime_error(&with_counter(
        "DIM l AS LIST\nAPPEND l, NEW Counter()\nx = l[1].Nope()",
    ));
    assert!(e.contains("COUNTER") && e.contains("NOPE"), "{e}");
    let e = runtime_error(&with_counter("a = NOTHING\nx = a.Bump()"));
    assert!(e.contains("NOTHING"), "{e}");
    let e = runtime_error(&with_counter("DIM a AS Counter\na = NOTHING\nx = a.Bump()"));
    assert!(e.contains("NOTHING"), "{e}");
}

#[test]
fn compile_error_cases() {
    let e = compile_error(&with_counter("DIM a AS Counter\nx = a.Nope()"));
    assert!(e.to_uppercase().contains("NOPE"), "{e}");
    let e = compile_error("CLASS C\n x AS INTEGER\n FUNCTION X()\n FEND\nEND CLASS\n");
    assert!(e.contains("X"), "{e}");
    let e = compile_error(
        "CLASS C\n FUNCTION NEW()\n FEND\n FUNCTION NEW(PARAMETER a)\n FEND\nEND CLASS\n",
    );
    assert!(e.to_uppercase().contains("NEW"), "{e}");
    let e = compile_error("CLASS C\n FUNCTION M()\n  SELF = 1\n FEND\nEND CLASS\n");
    assert!(e.contains("SELF"), "{e}");
    let e = compile_error("CLASS C\n FUNCTION NEW(PARAMETER a)\n FEND\nEND CLASS\nDIM c AS C\n");
    assert!(e.contains("NEW"), "{e}");
    let e = compile_error("CLASS C\n x AS INTEGER\n");
    assert!(e.to_uppercase().contains("CLASS"), "{e}");
}

#[test]
fn self_outside_method_is_an_error_but_variable_named_self_works() {
    let e = runtime_error("PRINT SELF.x");
    assert!(e.contains("SELF"), "{e}");
    assert_eq!(ok("SELF = 4\nclass = 5\nPRINT SELF + class"), "9\n");
}

#[test]
fn diagnostics_name_the_method() {
    let e = runtime_error(
        "CLASS Person\n FUNCTION Greet()\n  PRINT 1 / 0\n FEND\nEND CLASS\nDIM p AS Person\nCALL p.Greet()",
    );
    assert!(e.contains("PERSON.GREET"), "{e}");
}

#[test]
fn try_catch_unwinds_inside_methods() {
    let src = "CLASS Z\n FUNCTION Go() AS INTEGER\n  TRY\n   THROW \"boom\"\n  CATCH\n   Go = 3\n  END TRY\n FEND\nEND CLASS\n\
         DIM z AS Z\nPRINT z.Go()";
    assert_eq!(ok(src), "3\n");
}

#[test]
fn byref_method_parameters_work_statically() {
    let src = "CLASS B\n FUNCTION Inc(PARAMETER BYREF n)\n  n = n + 1\n FEND\nEND CLASS\n\
        DIM b AS B\nk = 1\nCALL b.Inc(k)\nPRINT k";
    assert_eq!(ok(src), "2\n");
}

#[test]
fn records_copy_and_type_inspection() {
    let out = ok(&with_counter(
        "DIM a AS Counter\nb = COPY(a)\nb.n = 5\nPRINT a.n; \",\"; b.n; \",\"; TYPEOF(a); \",\"; TYPENAME$(a)",
    ));
    assert_eq!(out, "0,5,RECORD,COUNTER\n");
    assert_eq!(
        ok("TYPE T\n a AS INTEGER = 2\nEND TYPE\nDIM t AS T\nPRINT t.a"),
        "2\n"
    );
}

static SEQ: AtomicUsize = AtomicUsize::new(0);

#[test]
fn classes_work_across_modules() {
    let dir = std::env::temp_dir().join(format!(
        "basic-vm-objects-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("m.bas"),
        "base = 100\nCLASS Acc\n v AS INTEGER\n FUNCTION Add(PARAMETER n) AS INTEGER\n  SELF.v = SELF.v + n + base\n  Add = SELF.v\n FEND\nEND CLASS\n",
    )
    .unwrap();
    let main = dir.join("main.bas");
    fs::write(
        &main,
        "IMPORT \"m.bas\" AS M\nDIM a AS M.Acc\nbase = 1\nx = a.Add(5)\nPRINT a.Add(1)\n",
    )
    .unwrap();
    let program = compile_file(&main).unwrap();
    let mut vm = Vm::new(program).unwrap();
    let mut out = Vec::new();
    vm.run(&mut Cursor::new(""), &mut out).unwrap();
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(String::from_utf8(out).unwrap(), "206\n");
}

#[test]
fn objects_example_runs_from_the_cli() {
    let path: PathBuf = ["examples", "objects.bas"].iter().collect();
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Mini with 75hp") && text.contains("CAR RECORD"));
}
