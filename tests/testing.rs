//! Built-in unit testing: `TEST` blocks, `ASSERT`/`ASSERTEQ`/`ASSERTNEAR`,
//! the `run_tests` library API and the `--test` command line mode.

mod common;

use std::io::Cursor;
use std::path::Path;
use std::process::{Command, Output};

use basic_vm::{
    Error, MAX_CALL_DEPTH, MAX_CAPTURED_OUTPUT, TestOptions, TestReport, TestResult, TestStatus,
    Vm, compile, compile_file, run_tests,
};
use common::TempDir;

// ---------------------------------------------------------------- helpers

fn run_main(source: &str) -> (String, Result<(), Error>) {
    let program = match compile(source) {
        Ok(program) => program,
        Err(error) => return (String::new(), Err(error)),
    };
    let mut vm = Vm::new(program).unwrap();
    let mut output = Vec::new();
    let result = vm.run(&mut Cursor::new(""), &mut output);
    (String::from_utf8(output).unwrap(), result)
}

fn output_of(source: &str) -> String {
    let (output, result) = run_main(source);
    result.unwrap_or_else(|error| panic!("{error}"));
    output
}

fn compile_error(source: &str) -> String {
    match compile(source) {
        Ok(_) => panic!("expected a compile error for:\n{source}"),
        Err(error) => error.to_string(),
    }
}

/// Runs `statements` as a main program and returns the uncaught error.
fn failure(statements: &str) -> Error {
    let (_, result) = run_main(statements);
    result.expect_err("expected an error")
}

fn assertion_message(statements: &str) -> String {
    let error = failure(statements);
    assert!(error.assertion, "not an assertion failure: {error}");
    error.message
}

fn passes(statements: &str) {
    let (_, result) = run_main(statements);
    result.unwrap_or_else(|error| panic!("{statements}\n-> {error}"));
}

fn report_with(source: &str, options: &TestOptions) -> TestReport {
    let program = compile(source).unwrap_or_else(|error| panic!("{error}"));
    run_tests(&program, options)
}

fn report(source: &str) -> TestReport {
    report_with(source, &TestOptions::default())
}

fn lines(report: &TestReport) -> Vec<String> {
    report
        .results
        .iter()
        .map(|result| result.line_text(""))
        .collect()
}

fn result_names(report: &TestReport) -> Vec<String> {
    report.results.iter().map(|r| r.name.clone()).collect()
}

fn result_of<'a>(report: &'a TestReport, name: &str) -> &'a TestResult {
    report
        .results
        .iter()
        .find(|result| result.name == name)
        .unwrap_or_else(|| panic!("no test named {name}"))
}

fn write(dir: &TempDir, name: &str, text: &str) -> std::path::PathBuf {
    let path = dir.file(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, text).unwrap();
    path
}

struct Run {
    stdout: String,
    stderr: String,
    code: i32,
}

fn cli(args: &[&str]) -> Run {
    cli_in(args, None)
}

fn cli_in(args: &[&str], directory: Option<&Path>) -> Run {
    let mut command = Command::new(env!("CARGO_BIN_EXE_basic-vm"));
    command.args(args);
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    let Output {
        stdout,
        stderr,
        status,
    } = command.output().unwrap();
    Run {
        stdout: String::from_utf8(stdout).unwrap(),
        stderr: String::from_utf8(stderr).unwrap(),
        code: status.code().unwrap(),
    }
}

// ------------------------------------------------------ compile errors

#[test]
fn tests_nested_in_other_constructs_are_compile_errors() {
    let cases = [
        ("FUNCTION F\nTEST A\nEND TEST\nFEND\n", 2, "top level"),
        ("IF 1 THEN\nTEST A\nEND TEST\nEND IF\n", 2, "top level"),
        ("FOR I = 1 TO 2\nTEST A\nEND TEST\nNEXT I\n", 2, "top level"),
        ("WHILE 0\nTEST A\nEND TEST\nWEND\n", 2, "top level"),
        ("TRY\nTEST A\nEND TEST\nCATCH\nEND TRY\n", 2, "top level"),
        ("TEST A\nTEST B\nEND TEST\nEND TEST\n", 2, "nested"),
        (
            "CLASS C\n  x AS INTEGER\n  TEST A\n  END TEST\nEND CLASS\n",
            3,
            "top level",
        ),
        (
            "TYPE T\n  x AS INTEGER\n  TEST A\nEND TYPE\n",
            3,
            "top level",
        ),
    ];
    for (source, line, text) in cases {
        let message = compile_error(source);
        assert!(
            message.starts_with(&format!("line {line}:")) && message.contains(text),
            "{source:?} -> {message}"
        );
    }
}

#[test]
fn duplicate_missing_and_mismatched_blocks_are_compile_errors() {
    let message = compile_error("TEST A\nEND TEST\nTEST a\nEND TEST\n");
    assert!(
        message.starts_with("line 3:") && message.contains("duplicate"),
        "{message}"
    );
    let message = compile_error("PRINT 1\nTEST A\nPRINT 2\n");
    assert!(message.contains("unterminated TEST"), "{message}");
    let message = compile_error("END TEST\n");
    assert!(
        message.starts_with("line 1:") && message.contains("TEST"),
        "{message}"
    );
    let message = compile_error("FUNCTION F\nEND TEST\nFEND\n");
    assert!(message.starts_with("line 2:"), "{message}");
    let message = compile_error("TEST A\nFEND\n");
    assert!(
        message.starts_with("line 2:") && message.contains("END TEST"),
        "{message}"
    );
    let message = compile_error("TEST A\nIF 1 THEN\nEND TEST\n");
    assert!(message.starts_with("line 3:"), "{message}");
    let message = compile_error("TEST\nEND TEST\n");
    assert!(
        message.starts_with("line 1:") && message.contains("name"),
        "{message}"
    );
    let message = compile_error("TEST A$\nEND TEST\n");
    assert!(message.starts_with("line 1:"), "{message}");
}

#[test]
fn exit_test_is_only_legal_inside_a_test() {
    let message = compile_error("EXIT TEST\n");
    assert!(
        message.starts_with("line 1:") && message.contains("TEST"),
        "{message}"
    );
    let message = compile_error("FUNCTION F\nEXIT TEST\nFEND\n");
    assert!(message.starts_with("line 2:"), "{message}");
    let message = compile_error("TEST A\nEXIT FUNCTION\nEND TEST\n");
    assert!(
        message.starts_with("line 2:") && message.contains("EXIT TEST"),
        "{message}"
    );
}

#[test]
fn labels_cannot_cross_the_test_boundary() {
    // Out of the test into the main program.
    let message = compile_error("10 PRINT 1\nTEST A\nGOTO 10\nEND TEST\n");
    assert!(
        message.starts_with("line 3:") && message.contains("10"),
        "{message}"
    );
    // From the main program into a test.
    let message = compile_error("GOTO 20\nTEST A\n20 PRINT 1\nEND TEST\n");
    assert!(
        message.starts_with("line 1:") && message.contains("20"),
        "{message}"
    );
    // Between two tests.
    let message = compile_error("TEST A\n5 PRINT 1\nEND TEST\nTEST B\nGOTO 5\nEND TEST\n");
    assert!(message.starts_with("line 5:"), "{message}");
}

#[test]
fn the_same_label_may_be_used_in_every_test() {
    let source =
        "10 PRINT \"main\"\nTEST A\n10 PRINT \"a\"\nEND TEST\nTEST B\n10 PRINT \"b\"\nEND TEST\n";
    assert_eq!(output_of(source), "main\n");
    // Each test has its own labels and can loop through its own.
    let looping = "TEST A\n  n = 0\n10 n = n + 1\n  IF n < 3 THEN GOTO 10\n  ASSERTEQ 3, n\nEND TEST\n\
                   TEST B\n  n = 0\n10 n = n + 2\n  IF n < 6 THEN GOTO 10\n  ASSERTEQ 6, n\nEND TEST\n";
    assert_eq!(lines(&report(looping)), vec!["PASS A", "PASS B"]);
}

#[test]
fn compile_errors_in_modules_name_the_file_and_line() {
    let dir = TempDir::new("testing-module-error");
    write(&dir, "m.bas", "FUNCTION F\nTEST A\nEND TEST\nFEND\n");
    let main = write(&dir, "main.bas", "IMPORT \"m.bas\" AS M\nPRINT 1\n");
    let error = compile_file(&main).unwrap_err().to_string();
    assert!(
        error.contains("m.bas:2:") && error.contains("top level"),
        "{error}"
    );
}

#[test]
fn assert_statements_check_their_arity() {
    for source in [
        "ASSERT\n",
        "ASSERT 1, \"a\", 3\n",
        "ASSERTEQ 1\n",
        "ASSERTEQ 1, 2, \"m\", 4\n",
        "ASSERTNEAR 1, 2\n",
        "ASSERTNEAR 1, 2, 3, \"m\", 5\n",
    ] {
        let message = compile_error(source);
        assert!(
            message.starts_with("line 1:") && message.contains("requires"),
            "{source:?} -> {message}"
        );
    }
}

// -------------------------------------- skipped in normal runs, legacy names

#[test]
fn test_blocks_are_skipped_in_normal_runs() {
    let source = "PRINT \"before\"\nTEST A\nPRINT \"inside\"\nASSERT 0\nEND TEST\n\
                  PRINT \"between\"\nFUNCTION F\nF = 1\nFEND\nTEST B\nPRINT F()\nTHROW \"x\"\nEND TEST\n\
                  PRINT \"after\"\n";
    assert_eq!(output_of(source), "before\nbetween\nafter\n");
}

#[test]
fn test_and_assert_remain_ordinary_variable_names() {
    let source = "TEST = 5\nPRINT TEST\nTEST = TEST + 1\nPRINT TEST * 2\n\
                  ASSERT = 3\nASSERTEQ = 4\nASSERTNEAR = 5\nPRINT ASSERT + ASSERTEQ + ASSERTNEAR\n\
                  DIM a(3)\nTEST = 1\nPRINT TEST\n";
    assert_eq!(output_of(source), "5\n12\n12\n1\n");
}

#[test]
fn test_and_assert_work_as_array_and_member_names() {
    let source = "DIM TEST(2)\nTEST[1] = 7\nPRINT TEST[1]\n\
                  CLASS C\n  ASSERT AS INTEGER\nEND CLASS\nDIM c AS C = NEW C()\nc.ASSERT = 4\nPRINT c.ASSERT\n";
    assert_eq!(output_of(source), "7\n4\n");
}

// ------------------------------------------------------ assertion semantics

#[test]
fn assert_uses_numeric_truth() {
    passes("ASSERT 1\nASSERT -2\nASSERT 0.5\nASSERT 1 = 1\nASSERT TRUE, \"ok\"\nASSERT NOT 0\n");
    assert_eq!(assertion_message("ASSERT 0\n"), "ASSERT failed");
    assert_eq!(assertion_message("ASSERT 1 = 2\n"), "ASSERT failed");
    assert_eq!(
        assertion_message("ASSERT FALSE, \"must hold\"\n"),
        "ASSERT failed - must hold"
    );
}

#[test]
fn assert_with_wrong_argument_types_is_a_plain_error() {
    for source in [
        "ASSERT \"yes\"\n",
        "ASSERT 1, 5\n",
        "ASSERTEQ 1, 1, 5\n",
        "ASSERTNEAR 1, 1, 1, 5\n",
        "ASSERTNEAR \"a\", 1, 1\n",
        "ASSERTNEAR 1, \"a\", 1\n",
        "ASSERTNEAR 1, 1, \"a\"\n",
        "ASSERTNEAR 1, 1, -0.5\n",
        "ASSERTNEAR LIST(1), LIST(1), 0\n",
    ] {
        let error = failure(source);
        assert!(!error.assertion, "{source:?} -> {error}");
    }
    let message = failure("ASSERTNEAR 1, 1, -1\n").message;
    assert!(message.contains("epsilon"), "{message}");
}

#[test]
fn asserteq_compares_numbers_exactly_like_the_equals_operator() {
    passes("ASSERTEQ 5, 5\nASSERTEQ 3, 3.0\nASSERTEQ 0.5, 1 / 2\nASSERTEQ -0.0, 0\n");
    assert_eq!(
        assertion_message("ASSERTEQ 5, 6\n"),
        "ASSERTEQ failed: expected 5, got 6"
    );
    assert_eq!(
        assertion_message("ASSERTEQ 5, 6, \"sum\"\n"),
        "ASSERTEQ failed: expected 5, got 6 - sum"
    );
    // 2^53 + 1 is not representable as a float: the comparison must stay exact.
    passes("ASSERTEQ 9007199254740992&, 9007199254740992.0\n");
    passes("ASSERTEQ 9007199254740993&, 9007199254740993&\n");
    let message = assertion_message("ASSERTEQ 9007199254740993&, 9007199254740992.0\n");
    assert!(
        message.starts_with("ASSERTEQ failed: expected 9007199254740993, got"),
        "{message}"
    );
    assert!(failure("ASSERTEQ 9007199254740992.0, 9007199254740993&\n").assertion);
    // The language's own `=` agrees.
    assert_eq!(
        output_of("PRINT 9007199254740993& = 9007199254740992.0\n"),
        "0\n"
    );
}

#[test]
fn asserteq_compares_strings_booleans_and_nothing() {
    passes(
        "ASSERTEQ \"abc\", \"ab\" + \"c\"\nASSERTEQ \"\", \"\"\nASSERTEQ TRUE, 1\nASSERTEQ FALSE, 0\n",
    );
    passes("ASSERTEQ NOTHING, NOTHING\n");
    assert_eq!(
        assertion_message("ASSERTEQ \"abc\", \"abd\"\n"),
        "ASSERTEQ failed: expected \"abc\", got \"abd\""
    );
    // Case matters and so does trailing whitespace.
    assert!(failure("ASSERTEQ \"a\", \"A\"\n").assertion);
    assert!(failure("ASSERTEQ \"a\", \"a \"\n").assertion);
    assert_eq!(
        assertion_message("ASSERTEQ NOTHING, 0\n"),
        "ASSERTEQ failed: expected NOTHING, got 0"
    );
    // Kinds never compare equal, and a mismatch is a failure, not an error.
    assert_eq!(
        assertion_message("ASSERTEQ 1, \"1\"\n"),
        "ASSERTEQ failed: expected 1, got \"1\""
    );
    assert!(failure("ASSERTEQ \"\", 0\n").assertion);
    assert!(failure("ASSERTEQ LIST(), \"\"\n").assertion);
    assert!(failure("ASSERTEQ LIST(1), 1\n").assertion);
}

#[test]
fn asserteq_quotes_and_escapes_strings_in_messages() {
    assert_eq!(
        assertion_message("ASSERTEQ \"say \"\"hi\"\"\", \"x\"\n"),
        "ASSERTEQ failed: expected \"say \"\"hi\"\"\", got \"x\""
    );
    // A long string is cut, so the message stays bounded.
    let message = assertion_message(
        "S$ = \"xxxxxxxxxx\"\nS$ = S$ + S$\nS$ = S$ + S$\nS$ = S$ + S$\nS$ = S$ + S$\nASSERTEQ S$, \"\"\n",
    );
    assert!(message.len() < 200 && message.contains("..."), "{message}");
}

#[test]
fn asserteq_compares_lists_arrays_and_maps_deeply() {
    passes("ASSERTEQ LIST(1, 2, 3), LIST(1, 2, 3)\nASSERTEQ LIST(), LIST()\n");
    passes("ASSERTEQ ARRAY(1, 2), ARRAY(1, 2)\n");
    passes(
        "ASSERTEQ LIST(1, LIST(2, \"x\"), MAP(\"k\", LIST(3))), LIST(1, LIST(2, \"x\"), MAP(\"k\", LIST(3)))\n",
    );
    // Order matters for lists and arrays.
    assert_eq!(
        assertion_message("ASSERTEQ LIST(1, 2), LIST(2, 1)\n"),
        "ASSERTEQ failed: expected LIST(1, 2), got LIST(2, 1)"
    );
    assert!(failure("ASSERTEQ LIST(1, 2), LIST(1, 2, 3)\n").assertion);
    assert!(failure("ASSERTEQ LIST(1, LIST(2)), LIST(1, LIST(3))\n").assertion);
    // A list is not an array.
    assert!(failure("ASSERTEQ LIST(1, 2), ARRAY(1, 2)\n").assertion);
    // Maps: insertion order is irrelevant.
    passes("ASSERTEQ MAP(\"a\", 1, \"b\", 2), MAP(\"b\", 2, \"a\", 1)\n");
    assert_eq!(
        assertion_message("ASSERTEQ MAP(\"a\", 1), MAP(\"a\", 2)\n"),
        "ASSERTEQ failed: expected MAP(\"a\": 1), got MAP(\"a\": 2)"
    );
    assert!(failure("ASSERTEQ MAP(\"a\", 1), MAP(\"b\", 1)\n").assertion);
    assert!(failure("ASSERTEQ MAP(\"a\", 1), MAP(\"a\", 1, \"b\", 2)\n").assertion);
    assert!(failure("ASSERTEQ MAP(\"a\", 1), LIST(\"a\", 1)\n").assertion);
    // The same collection compares equal to itself.
    passes("L = LIST(1, 2)\nASSERTEQ L, L\n");
}

#[test]
fn asserteq_renders_long_collections_compactly() {
    let long = (1..=40)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let message = assertion_message(&format!("ASSERTEQ LIST({long}), LIST()\n"));
    assert!(message.contains("more"), "{message}");
    assert!(message.len() < 300, "{} chars: {message}", message.len());
    let message = assertion_message("ASSERTEQ LIST(LIST(LIST(LIST(LIST(1))))), 0\n");
    assert!(message.contains("LIST(...)"), "{message}");
}

#[test]
fn asserteq_compares_records_by_type_and_fields() {
    let types = "TYPE Point\n  x AS INTEGER\n  y AS INTEGER\nEND TYPE\n\
                 TYPE Other\n  x AS INTEGER\n  y AS INTEGER\nEND TYPE\n\
                 TYPE Named\n  name$ AS STRING\n  tags AS LIST\nEND TYPE\n";
    passes(&format!(
        "{types}DIM a AS Point = NEW Point(x = 1, y = 2)\nDIM b AS Point = NEW Point(x = 1, y = 2)\n\
         ASSERTEQ a, b\nASSERTEQ a, a\n\
         DIM n AS Named = NEW Named(name$ = \"n\", tags = LIST(\"a\"))\n\
         DIM m AS Named = NEW Named(name$ = \"n\", tags = LIST(\"a\"))\nASSERTEQ n, m\n"
    ));
    let message = assertion_message(&format!(
        "{types}DIM a AS Point = NEW Point(x = 1, y = 2)\nDIM b AS Point = NEW Point(x = 1, y = 3)\nASSERTEQ a, b\n"
    ));
    assert_eq!(
        message,
        "ASSERTEQ failed: expected POINT(X = 1, Y = 2), got POINT(X = 1, Y = 3)"
    );
    // Same fields, different type.
    assert!(
        failure(&format!(
            "{types}DIM a AS Point = NEW Point(x = 1, y = 2)\nDIM b AS Other = NEW Other(x = 1, y = 2)\nASSERTEQ a, b\n"
        ))
        .assertion
    );
    // A nested difference is found.
    assert!(
        failure(&format!(
            "{types}DIM n AS Named = NEW Named(name$ = \"n\", tags = LIST(\"a\"))\n\
             DIM m AS Named = NEW Named(name$ = \"n\", tags = LIST(\"b\"))\nASSERTEQ n, m\n"
        ))
        .assertion
    );
    // A record is not a number, a list or NOTHING.
    assert!(
        failure(&format!(
            "{types}DIM a AS Point = NEW Point(x = 1, y = 2)\nASSERTEQ a, NOTHING\n"
        ))
        .assertion
    );
}

#[test]
fn asserteq_terminates_on_cyclic_structures() {
    // Two lists that contain themselves.
    passes("A = LIST(1)\nAPPEND A, A\nB = LIST(1)\nAPPEND B, B\nASSERTEQ A, B\n");
    // Cycles of different shape are still found different (and do not hang).
    let message =
        assertion_message("A = LIST(1)\nAPPEND A, A\nB = LIST(2)\nAPPEND B, B\nASSERTEQ A, B\n");
    assert!(message.starts_with("ASSERTEQ failed"), "{message}");
    assert!(message.len() < 300, "{message}");
    // Records that point at themselves.
    let node = "TYPE Node\n  value AS INTEGER\n  next AS Node\nEND TYPE\n\
                DIM a AS Node = NEW Node(value = 1)\nDIM b AS Node = NEW Node(value = 1)\n\
                a.next = a\nb.next = b\n";
    passes(&format!("{node}ASSERTEQ a, b\n"));
    let message = assertion_message(&format!("{node}b.value = 2\nASSERTEQ a, b\n"));
    assert!(message.starts_with("ASSERTEQ failed"), "{message}");
    // A mutual cycle through two maps.
    passes(
        "M = MAP(\"v\", 1)\nN = MAP(\"v\", 1)\nM[\"self\"] = M\nN[\"self\"] = N\nASSERTEQ M, N\n",
    );
}

#[test]
fn assertnear_checks_the_absolute_difference() {
    passes(
        "ASSERTNEAR 1, 1, 0\nASSERTNEAR 1, 1.5, 0.5\nASSERTNEAR 1.5, 1, 0.5\nASSERTNEAR 0.1 + 0.2, 0.3, 0.000000001\n",
    );
    assert_eq!(
        assertion_message("ASSERTNEAR 1, 2, 0.5\n"),
        "ASSERTNEAR failed: expected 1, got 2 (difference 1, allowed 0.5)"
    );
    assert_eq!(
        assertion_message("ASSERTNEAR 1, 2, 0.5, \"close\"\n"),
        "ASSERTNEAR failed: expected 1, got 2 (difference 1, allowed 0.5) - close"
    );
    // The boundary is inclusive; just beyond fails.
    passes("ASSERTNEAR 10, 12, 2\n");
    assert!(failure("ASSERTNEAR 10, 12.0001, 2\n").assertion);
    // Epsilon 0 demands exact equality.
    assert!(failure("ASSERTNEAR 0.1 + 0.2, 0.3, 0\n").assertion);
    // Integers beyond 2^53 are compared exactly, not through floats.
    assert!(failure("ASSERTNEAR 9007199254740993&, 9007199254740992&, 0\n").assertion);
    passes("ASSERTNEAR 9007199254740993&, 9007199254740992&, 1\n");
    // Booleans are numbers.
    passes("ASSERTNEAR TRUE, 1, 0\n");
}

// ------------------------------------------- catchability and error kinds

#[test]
fn assertion_failures_are_catchable_and_catch_receives_the_message() {
    let source = "TRY\n  ASSERTEQ 1, 2, \"m\"\nCATCH e$\n  PRINT e$\nEND TRY\n\
                  TRY\n  ASSERT 0\nCATCH e$\n  PRINT e$\nEND TRY\n\
                  TRY\n  ASSERTNEAR 1, 3, 1\nCATCH e$\n  PRINT e$\nEND TRY\n\
                  PRINT \"done\"\n";
    assert_eq!(
        output_of(source),
        "ASSERTEQ failed: expected 1, got 2 - m\nASSERT failed\n\
         ASSERTNEAR failed: expected 1, got 3 (difference 2, allowed 1)\ndone\n"
    );
}

#[test]
fn assertion_inside_a_function_is_catchable_by_the_caller() {
    let source = "FUNCTION Check(PARAMETER n)\n  ASSERT n > 0, \"positive\"\n  Check = n\nFEND\n\
                  TRY\n  PRINT Check(-1)\nCATCH e$\n  PRINT \"caught: \"; e$\nEND TRY\nPRINT Check(2)\n";
    assert_eq!(output_of(source), "caught: ASSERT failed - positive\n2\n");
}

#[test]
fn error_kind_distinguishes_assertions_from_other_errors() {
    let error = failure("ASSERT 0\n");
    assert!(error.assertion);
    assert_eq!(error.line, 1);
    assert!(!failure("THROW \"x\"\n").assertion);
    assert!(!failure("X = 1 / 0\n").assertion);
    assert!(!failure("ASSERT \"text\"\n").assertion);
    // A caught assertion does not taint a later, different error.
    let error = failure("TRY\n  ASSERT 0\nCATCH\n  PRINT 1\nEND TRY\nTHROW \"later\"\n");
    assert!(!error.assertion, "{error}");
    let error = failure("TRY\n  ASSERT 0\nCATCH\n  PRINT 1\nEND TRY\nX = 1 / 0\n");
    assert!(!error.assertion, "{error}");
    // ... nor does a caught ordinary error make a later assertion an error.
    let error = failure("TRY\n  THROW \"x\"\nCATCH\n  PRINT 1\nEND TRY\nASSERT 0\n");
    assert!(error.assertion, "{error}");
}

#[test]
fn assertion_kind_survives_rethrow_and_finally() {
    let error = failure("TRY\n  ASSERT 0, \"a\"\nCATCH e$\n  THROW\nEND TRY\n");
    assert!(error.assertion, "{error}");
    assert!(error.message.contains("a"), "{error}");
    let error = failure("TRY\n  ASSERT 0\nFINALLY\n  PRINT \"cleanup\"\nEND TRY\n");
    assert!(error.assertion, "{error}");
    // An error thrown inside CATCH replaces the assertion.
    let error = failure("TRY\n  ASSERT 0\nCATCH\n  THROW \"other\"\nEND TRY\n");
    assert!(!error.assertion, "{error}");
    assert_eq!(error.message, "other");
}

#[test]
fn assertion_errors_report_line_and_function() {
    let source = "FUNCTION Deep\n  ASSERT 0, \"deep\"\nFEND\nPRINT 1\nCALL Deep\n";
    let (output, result) = run_main(source);
    let error = result.unwrap_err();
    assert_eq!(output, "1\n");
    assert!(error.assertion);
    assert_eq!(error.line, 2);
    assert_eq!(error.function.as_deref(), Some("DEEP"));
    assert_eq!(
        error.to_string(),
        "line 2: ASSERT failed - deep (in function DEEP)"
    );
}

// ------------------------------------------------------- classification

#[test]
fn tests_pass_fail_or_error() {
    let source = "FUNCTION Add(PARAMETER a, PARAMETER b)\n  Add = a + b\nFEND\n\
TEST Passes\n  ASSERTEQ 5, Add(2, 3)\nEND TEST\n\
TEST Fails\n  ASSERTEQ 6, Add(2, 3)\nEND TEST\n\
TEST Throws\n  THROW \"boom\"\nEND TEST\n\
TEST Divides\n  LOCAL x\n  x = 1 / 0\nEND TEST\n\
TEST Empty\nEND TEST\n";
    let report = report(source);
    assert_eq!(
        lines(&report),
        vec![
            "PASS Passes",
            "FAIL Fails: ASSERTEQ failed: expected 6, got 5 (line 8)",
            "ERROR Throws: boom (line 11)",
            "ERROR Divides: division by zero (line 15)",
            "PASS Empty",
        ]
    );
    assert_eq!(report.total(), 5);
    assert_eq!(
        (report.passed(), report.failed(), report.errors()),
        (2, 1, 2)
    );
    assert!(!report.success());
    assert_eq!(result_of(&report, "Fails").status, TestStatus::Failed);
    assert_eq!(result_of(&report, "Throws").status, TestStatus::Errored);
    assert_eq!(result_of(&report, "Fails").line, 8);
}

#[test]
fn failures_inside_called_functions_name_the_function() {
    let source = "FUNCTION Check\n  ASSERT 0, \"inside\"\nFEND\nFUNCTION Boom\n  THROW \"bang\"\nFEND\n\
TEST A\n  CALL Check\nEND TEST\nTEST B\n  CALL Boom\nEND TEST\n";
    let report = report(source);
    assert_eq!(
        lines(&report),
        vec![
            "FAIL A: ASSERT failed - inside (line 2, in function CHECK)",
            "ERROR B: bang (line 5, in function BOOM)",
        ]
    );
}

#[test]
fn a_caught_assertion_inside_a_test_passes() {
    let source = "TEST Caught\n  DIM ok = 0\n  TRY\n    ASSERTEQ 1, 2\n  CATCH e$\n    ok = 1\n    ASSERT LEFT$(e$, 14) = \"ASSERTEQ faile\"\n  END TRY\n  ASSERT ok\nEND TEST\n";
    assert_eq!(lines(&report(source)), vec!["PASS Caught"]);
}

#[test]
fn an_error_is_not_masked_by_finally_and_later_tests_still_run() {
    let source = "TEST A\n  TRY\n    THROW \"x\"\n  FINALLY\n    PRINT \"cleanup\"\n  END TRY\nEND TEST\nTEST B\nEND TEST\n";
    let report = report(source);
    assert_eq!(lines(&report), vec!["ERROR A: x (line 3)", "PASS B"]);
    assert_eq!(report.results[0].output, "cleanup\n");
}

#[test]
fn exit_test_ends_a_test_as_a_pass() {
    let source = "TEST Early\n  PRINT \"one\"\n  EXIT TEST\n  ASSERT 0, \"not reached\"\nEND TEST\n\
TEST InsideLoopsAndTry\n  FOR I = 1 TO 10\n    TRY\n      IF I = 3 THEN\n        EXIT TEST\n      END IF\n    FINALLY\n      PRINT I\n    END TRY\n  NEXT I\n  ASSERT 0, \"not reached\"\nEND TEST\n\
TEST AfterwardsStillWorks\n  ASSERT 1\nEND TEST\n";
    let report = report(source);
    assert_eq!(
        lines(&report),
        vec![
            "PASS Early",
            "PASS InsideLoopsAndTry",
            "PASS AfterwardsStillWorks"
        ]
    );
}

#[test]
fn tests_use_their_own_local_scope_and_control_flow() {
    let source = "x = 100\nTEST Locals\n  LOCAL x\n  x = 5\n  FOR i = 1 TO 3\n    x = x + i\n  NEXT i\n  ASSERTEQ 11, x\nEND TEST\n\
TEST Labels\n  n = 0\n10 n = n + 1\n  IF n < 3 THEN GOTO 10\n  ASSERTEQ 3, n\nEND TEST\n\
TEST Loops\n  total = 0\n  FOR EACH v IN LIST(1, 2, 3)\n    total = total + v\n  NEXT\n  DO WHILE total < 100\n    total = total * 2\n  LOOP\n  ASSERTEQ 192, total\nEND TEST\n";
    let report = report(source);
    assert_eq!(
        lines(&report),
        vec!["PASS Locals", "PASS Labels", "PASS Loops"],
        "{:?}",
        report
    );
}

#[test]
fn tests_can_use_classes_constants_and_functions_of_their_file() {
    let source = "CONST LIMIT = 10\nTYPE Point\n  x AS INTEGER\nEND TYPE\n\
CLASS Counter\n  n AS INTEGER\n  FUNCTION NEW()\n    SELF.n = 0\n  FEND\n  FUNCTION Bump() AS INTEGER\n    SELF.n = SELF.n + 1\n    Bump = SELF.n\n  FEND\nEND CLASS\n\
TEST Everything\n  DIM c AS Counter = NEW Counter()\n  ASSERTEQ 1, c.Bump()\n  ASSERTEQ 2, c.Bump()\n  DIM p AS Point = NEW Point(x = LIMIT)\n  ASSERTEQ 10, p.x\nEND TEST\n";
    assert_eq!(lines(&report(source)), vec!["PASS Everything"]);
}

// ----------------------------------------------- main program not run

#[test]
fn the_main_program_body_is_not_run_in_test_mode() {
    let source = "PRINT \"main starts\"\nINPUT x\nDO\nLOOP\nTEST Shown\n  PRINT \"in test\"\n  ASSERT 0\nEND TEST\n";
    let report = report(source);
    assert_eq!(report.results.len(), 1);
    // Only what the test itself printed is captured.
    assert_eq!(report.results[0].output, "in test\n");
}

#[test]
fn global_constants_and_declarations_of_the_main_file_are_available() {
    let source = "CONST LIMIT = 4\nDIM items = LIST(1, 2)\nDIM name$ AS STRING = \"abc\"\n\
x = 5\nPRINT \"not run\"\n\
TEST Sees\n  ASSERTEQ 4, LIMIT\n  ASSERTEQ 2, LEN(items)\n  ASSERTEQ \"abc\", name$\nEND TEST\n";
    assert_eq!(lines(&report(source)), vec!["PASS Sees"]);
}

#[test]
fn tests_read_and_write_globals_but_see_no_main_body_assignments() {
    let source = "DIM counter AS INTEGER = 0\nTEST Writes\n  counter = counter + 1\n  ASSERTEQ 1, counter\nEND TEST\n";
    assert_eq!(lines(&report(source)), vec!["PASS Writes"]);
}

// ------------------------------------------------------------ isolation

#[test]
fn every_test_runs_on_a_fresh_vm() {
    let source = "DIM counter AS INTEGER = 0\nDIM items = LIST()\nTEST First\n  counter = counter + 1\n  APPEND items, counter\n  g = 10\n  ASSERTEQ 1, counter\n  ASSERTEQ 1, LEN(items)\nEND TEST\n\
TEST Second\n  counter = counter + 1\n  APPEND items, counter\n  ASSERTEQ 1, counter\n  ASSERTEQ 1, LEN(items)\n  caught = 0\n  TRY\n    x = g\n  CATCH\n    caught = 1\n  END TRY\n  ASSERT caught, \"g leaked from the first test\"\nEND TEST\n";
    let report = report(source);
    assert_eq!(
        lines(&report),
        vec!["PASS First", "PASS Second"],
        "{report:?}"
    );
}

#[test]
fn the_random_generator_starts_from_the_default_seed_in_every_test() {
    // Each test reports the first number it draws as its (failing) message.
    let source = "TEST A\n  R = RND()\n  R = RND()\n  R = RND()\n  RANDOMIZE 7\n  R = RND()\n  ASSERT 0\nEND TEST\n\
TEST B\n  ASSERTEQ \"\", STR$(RND())\nEND TEST\nTEST C\n  ASSERTEQ \"\", STR$(RND())\nEND TEST\n\
TEST D\n  RANDOMIZE 7\n  ASSERTEQ \"\", STR$(RND())\nEND TEST\nTEST E\n  RANDOMIZE 7\n  ASSERTEQ \"\", STR$(RND())\nEND TEST\n";
    let report = report(source);
    let message = |name: &str| result_of(&report, name).message.clone();
    assert_eq!(message("B"), message("C"));
    assert_eq!(message("D"), message("E"));
    assert_ne!(message("B"), message("D"));
    // The first draw equals the first draw of a normal run.
    let first = output_of("PRINT STR$(RND())\n");
    assert_eq!(
        message("B"),
        format!(
            "ASSERTEQ failed: expected \"\", got \"{}\"",
            first.trim_end()
        )
    );
}

#[test]
fn files_are_not_shared_between_tests() {
    let dir = TempDir::new("testing-files");
    let path = dir.text("data.txt");
    let source = format!(
        "TEST Writer\n  OPEN \"{path}\" FOR OUTPUT AS #1\n  PRINT #1, \"hello\"\n  CLOSE #1\nEND TEST\n\
         TEST Reader\n  OPEN \"{path}\" FOR INPUT AS #1\n  LINE INPUT #1, L$\n  CLOSE #1\n  ASSERTEQ \"hello\", L$\nEND TEST\n\
         TEST SameHandleAgain\n  OPEN \"{path}\" FOR INPUT AS #1\n  CLOSE #1\nEND TEST\n"
    );
    assert_eq!(
        lines(&report(&source)),
        vec!["PASS Writer", "PASS Reader", "PASS SameHandleAgain"]
    );
}

// ---------------------------------------------------------- file leaks

#[test]
fn files_left_open_fail_the_test() {
    let dir = TempDir::new("testing-leak");
    let path = dir.text("leak.txt");
    let source = format!(
        "TEST Leaks\n  OPEN \"{path}\" FOR OUTPUT AS #3\n  PRINT #3, \"x\"\nEND TEST\n\
         TEST Closes\n  OPEN \"{path}\" FOR OUTPUT AS #1\n  CLOSE #1\nEND TEST\n\
         TEST AfterLeak\n  OPEN \"{path}\" FOR OUTPUT AS #3\n  CLOSE #3\nEND TEST\n"
    );
    let report = report(&source);
    let leak = result_of(&report, "Leaks");
    assert_eq!(leak.status, TestStatus::Failed);
    assert!(
        leak.message.contains("resource leak") && leak.message.contains("#3"),
        "{}",
        leak.message
    );
    assert_eq!(leak.line, 1);
    assert_eq!(result_of(&report, "Closes").status, TestStatus::Passed);
    // The leaked file was closed by the runner, so the next test can use it.
    assert_eq!(result_of(&report, "AfterLeak").status, TestStatus::Passed);
    // ... and its data was flushed.
    assert_eq!(std::fs::read_to_string(dir.file("leak.txt")).unwrap(), "");
}

#[test]
fn a_failing_test_that_left_a_file_open_reports_the_failure_not_the_leak() {
    let dir = TempDir::new("testing-leak-fail");
    let path = dir.text("leak.txt");
    let source =
        format!("TEST A\n  OPEN \"{path}\" FOR OUTPUT AS #1\n  ASSERT 0, \"first\"\nEND TEST\n");
    let report = report(&source);
    assert_eq!(report.results[0].status, TestStatus::Failed);
    assert_eq!(report.results[0].message, "ASSERT failed - first");
}

// ------------------------------------------------------- output capture

#[test]
fn output_is_captured_and_shown_only_for_non_passing_tests() {
    let source = "TEST Quiet\n  PRINT \"hidden\"\nEND TEST\n\
TEST Loud\n  PRINT \"line one\"\n  PRINT \"line two\"\n  ASSERT 0, \"x\"\nEND TEST\n\
TEST Broken\n  PRINT \"before\"\n  THROW \"y\"\nEND TEST\n";
    let report = report(source);
    assert_eq!(result_of(&report, "Quiet").output, "hidden\n");
    assert_eq!(
        report.render("main.bas"),
        "PASS Quiet\n\
         FAIL Loud: ASSERT failed - x (main.bas:7)\n    line one\n    line two\n\
         ERROR Broken: y (main.bas:11)\n    before\n\
         3 tests: 1 passed, 1 failed, 1 errors\n"
    );
}

#[test]
fn stdin_is_empty_in_tests() {
    let source = "TEST Reads\n  INPUT x\nEND TEST\n";
    let report = report(source);
    assert_eq!(report.results[0].status, TestStatus::Errored);
}

#[test]
fn graphics_use_the_headless_backend_and_screenshots_still_work() {
    let dir = TempDir::new("testing-graphics");
    let image = dir.text("shot.ppm");
    let source = format!(
        "TEST Draws\n  SCREEN 16, 12, \"t\"\n  GCLS 4\n  PSET 1, 1, 15\n  ASSERTEQ 16777215, POINT(1, 1)\n  SCREENSHOT \"{image}\"\nEND TEST\n\
         TEST Interactive\n  SCREEN 8, 8\n  FLIP\n  DOEVENTS\n  SLEEP 5000\nEND TEST\n"
    );
    let report = report(&source);
    assert_eq!(
        lines(&report),
        vec!["PASS Draws", "PASS Interactive"],
        "{report:?}"
    );
    let decoded = common::decode_file(&dir.file("shot.ppm"));
    assert_eq!((decoded.width, decoded.height), (16, 12));
    assert_eq!(decoded.at(1, 1), 0xFF_FF_FF);
}

// ----------------------------------------------------- filter / options

#[test]
fn the_filter_matches_names_case_insensitively() {
    let source =
        "TEST Add_basic\nEND TEST\nTEST Add_negatives\nEND TEST\nTEST Sub_basic\nEND TEST\n";
    let filtered = |text: &str| {
        let options = TestOptions {
            filter: Some(text.into()),
            ..TestOptions::default()
        };
        lines(&report_with(source, &options))
    };
    assert_eq!(
        filtered("add"),
        vec!["PASS Add_basic", "PASS Add_negatives"]
    );
    assert_eq!(filtered("BASIC"), vec!["PASS Add_basic", "PASS Sub_basic"]);
    assert_eq!(filtered("sub_b"), vec!["PASS Sub_basic"]);
    assert!(filtered("nothing").is_empty());
    assert_eq!(filtered("").len(), 3);
}

#[test]
fn a_program_without_tests_gives_an_empty_report() {
    let report = report("PRINT 1\n");
    assert!(report.results.is_empty());
    assert!(!report.success());
}

#[test]
fn test_names_are_case_insensitive_and_reported_as_written() {
    let report = report("TEST mixedCase_Name\nEND TEST\n");
    assert_eq!(lines(&report), vec!["PASS mixedCase_Name"]);
}

// -------------------------------------------------------------- modules

const MODULE: &str = "CONST BASE = 100\ncount = 1\nPRINT \"module init\"\n\
FUNCTION Twice(PARAMETER n)\n  Twice = n * 2\nFEND\n\
TEST Twice_works\n  ASSERTEQ 8, Twice(4)\n  ASSERTEQ 100, BASE\nEND TEST\n\
TEST Init_ran\n  ASSERTEQ 1, count\n  PRINT \"in module test\"\n  ASSERT 0, \"module failure\"\nEND TEST\n";

#[test]
fn module_tests_run_only_with_include_imports() {
    let dir = TempDir::new("testing-modules");
    write(&dir, "m.bas", MODULE);
    let main = write(
        &dir,
        "main.bas",
        "IMPORT \"m.bas\" AS Lib\nTEST Main_test\n  ASSERTEQ 8, Lib.Twice(4)\nEND TEST\n",
    );
    let program = compile_file(&main).unwrap();
    let without = run_tests(&program, &TestOptions::default());
    assert_eq!(lines(&without), vec!["PASS Main_test"]);

    let with = run_tests(
        &program,
        &TestOptions {
            include_imports: true,
            ..TestOptions::default()
        },
    );
    let text = lines(&with);
    assert_eq!(text.len(), 3);
    assert_eq!(text[0], "PASS Main_test");
    assert_eq!(text[1], "PASS Lib.Twice_works");
    assert!(
        text[2].starts_with("FAIL Lib.Init_ran: ASSERT failed - module failure ("),
        "{}",
        text[2]
    );
    assert!(text[2].contains("m.bas:14"), "{}", text[2]);
    // The module's top-level initialisation ran for the test; the output
    // is captured.
    assert_eq!(with.results[2].output, "module init\nin module test\n");
    assert_eq!(
        with.results[2]
            .file
            .as_deref()
            .map(|f| f.ends_with("m.bas")),
        Some(true)
    );
}

#[test]
fn the_filter_applies_to_the_qualified_module_name() {
    let dir = TempDir::new("testing-module-filter");
    write(&dir, "m.bas", MODULE);
    let main = write(
        &dir,
        "main.bas",
        "IMPORT \"m.bas\" AS Lib\nTEST Twice_main\nEND TEST\n",
    );
    let program = compile_file(&main).unwrap();
    let options = |filter: &str| TestOptions {
        filter: Some(filter.into()),
        include_imports: true,
    };
    assert_eq!(
        result_names(&run_tests(&program, &options("lib."))),
        vec!["Lib.Twice_works", "Lib.Init_ran"]
    );
    assert_eq!(
        result_names(&run_tests(&program, &options("lib.init"))),
        vec!["Lib.Init_ran"]
    );
    assert_eq!(
        lines(&run_tests(&program, &options("twice"))),
        vec!["PASS Twice_main", "PASS Lib.Twice_works"]
    );
    // Without --test-imports the module's tests are not even considered.
    let only_main = TestOptions {
        filter: Some("lib.".into()),
        include_imports: false,
    };
    assert!(run_tests(&program, &only_main).results.is_empty());
}

#[test]
fn a_module_run_directly_runs_its_own_tests() {
    let dir = TempDir::new("testing-module-direct");
    let module = write(
        &dir,
        "m.bas",
        "CONST BASE = 100\nFUNCTION Twice(PARAMETER n)\n  Twice = n * 2\nFEND\n\
         TEST Twice_works\n  ASSERTEQ 8, Twice(4)\n  ASSERTEQ 100, BASE\nEND TEST\n",
    );
    let program = compile_file(&module).unwrap();
    assert_eq!(
        lines(&run_tests(&program, &TestOptions::default())),
        vec!["PASS Twice_works"]
    );
}

#[test]
fn imported_modules_tests_see_the_modules_own_functions_and_state() {
    let dir = TempDir::new("testing-module-scope");
    write(
        &dir,
        "m.bas",
        "CONST BASE = 100\nTYPE Box\n  v AS INTEGER\nEND TYPE\nCLASS Acc\n  total AS INTEGER\n  FUNCTION NEW()\n    SELF.total = BASE\n  FEND\n  FUNCTION Add(PARAMETER n AS INTEGER)\n    SELF.total = SELF.total + n\n  FEND\nEND CLASS\n\
         FUNCTION Helper(PARAMETER n)\n  Helper = n + BASE\nFEND\nglobal = 5\n\
         TEST Scope\n  DIM a AS Acc = NEW Acc()\n  CALL a.Add(Helper(1))\n  ASSERTEQ 201, a.total\n  DIM b AS Box = NEW Box(v = global)\n  ASSERTEQ 5, b.v\n  global = 6\nEND TEST\n\
         TEST ScopeAgain\n  ASSERTEQ 5, global\nEND TEST\n",
    );
    let main = write(
        &dir,
        "main.bas",
        "IMPORT \"m.bas\" AS M\nTEST Own\n  ASSERTEQ 105, M.Helper(5)\n  ASSERTEQ 5, M.global\nEND TEST\n",
    );
    let program = compile_file(&main).unwrap();
    let options = TestOptions {
        include_imports: true,
        ..TestOptions::default()
    };
    assert_eq!(
        lines(&run_tests(&program, &options)),
        vec!["PASS Own", "PASS M.Scope", "PASS M.ScopeAgain"]
    );
}

#[test]
fn nested_imports_are_reported_in_import_order() {
    let dir = TempDir::new("testing-module-order");
    write(&dir, "b.bas", "TEST In_b\nEND TEST\n");
    write(
        &dir,
        "a.bas",
        "IMPORT \"b.bas\" AS B\nTEST In_a\nEND TEST\n",
    );
    write(&dir, "c.bas", "TEST In_c\nEND TEST\n");
    let main = write(
        &dir,
        "main.bas",
        "IMPORT \"a.bas\" AS A\nIMPORT \"c.bas\" AS C\nTEST In_main\nEND TEST\n",
    );
    let program = compile_file(&main).unwrap();
    let names: Vec<String> = run_tests(
        &program,
        &TestOptions {
            include_imports: true,
            ..TestOptions::default()
        },
    )
    .results
    .into_iter()
    .map(|result| result.name)
    .collect();
    assert_eq!(names.len(), 4);
    assert_eq!(names[0], "In_main");
    assert!(names[1].ends_with("In_a"), "{names:?}");
    assert!(names[2].ends_with("In_b"), "{names:?}");
    assert_eq!(names[3], "C.In_c");
    let tests: Vec<String> = program.tests().iter().map(|t| t.full_name()).collect();
    assert_eq!(tests.len(), 4);
}

#[test]
fn the_same_test_name_may_exist_in_different_files() {
    let dir = TempDir::new("testing-module-names");
    write(&dir, "m.bas", "TEST Same\nEND TEST\n");
    let main = write(
        &dir,
        "main.bas",
        "IMPORT \"m.bas\" AS M\nTEST Same\nEND TEST\n",
    );
    let program = compile_file(&main).unwrap();
    let report = run_tests(
        &program,
        &TestOptions {
            include_imports: true,
            ..TestOptions::default()
        },
    );
    assert_eq!(lines(&report), vec!["PASS Same", "PASS M.Same"]);
}

#[test]
fn test_blocks_in_imported_modules_do_nothing_in_normal_runs() {
    let dir = TempDir::new("testing-module-normal");
    write(
        &dir,
        "m.bas",
        "FUNCTION One\n  One = 1\nFEND\nPRINT \"init\"\nTEST T\n  PRINT \"never\"\n  ASSERT 0\nEND TEST\n",
    );
    let main = write(&dir, "main.bas", "IMPORT \"m.bas\" AS M\nPRINT M.One()\n");
    let program = compile_file(&main).unwrap();
    let mut vm = Vm::new(program).unwrap();
    let mut output = Vec::new();
    vm.run(&mut Cursor::new(""), &mut output).unwrap();
    assert_eq!(String::from_utf8(output).unwrap(), "init\n1\n");
}

// ----------------------------------------------------------------- CLI

#[test]
fn cli_reports_each_test_and_the_summary() {
    let dir = TempDir::new("testing-cli-report");
    let path = write(
        &dir,
        "t.bas",
        "FUNCTION Add(PARAMETER a, PARAMETER b)\n  Add = a + b\nFEND\n\
TEST Add_ok\n  ASSERTEQ 5, Add(2, 3)\nEND TEST\n\
TEST Add_wrong\n  PRINT \"debug output\"\n  ASSERTEQ 6, Add(2, 3), \"sum\"\nEND TEST\n\
TEST Add_error\n  THROW \"bad\"\nEND TEST\n",
    );
    let shown = path.display().to_string();
    let run = cli(&["--test", &shown]);
    assert_eq!(
        run.stdout,
        format!(
            "PASS Add_ok\n\
             FAIL Add_wrong: ASSERTEQ failed: expected 6, got 5 - sum ({shown}:9)\n    debug output\n\
             ERROR Add_error: bad ({shown}:12)\n\
             3 tests: 1 passed, 1 failed, 1 errors\n"
        )
    );
    assert_eq!(run.stderr, "");
    assert_eq!(run.code, 1);
}

#[test]
fn cli_exits_zero_when_every_test_passes() {
    let dir = TempDir::new("testing-cli-pass");
    let path = write(
        &dir,
        "t.bas",
        "PRINT \"main body must not run\"\nTEST A\n  PRINT \"quiet\"\nEND TEST\nTEST B\n  EXIT TEST\nEND TEST\n",
    );
    let shown = path.display().to_string();
    for flag in ["--test", "--run-tests"] {
        let run = cli(&[flag, &shown]);
        assert_eq!(
            run.stdout,
            "PASS A\nPASS B\n2 tests: 2 passed, 0 failed, 0 errors\n"
        );
        assert_eq!(run.stderr, "");
        assert_eq!(run.code, 0);
    }
}

#[test]
fn cli_normal_runs_skip_the_tests() {
    let dir = TempDir::new("testing-cli-normal");
    let path = write(
        &dir,
        "t.bas",
        "PRINT \"hello\"\nTEST A\n  PRINT \"test\"\n  ASSERT 0\nEND TEST\nPRINT \"bye\"\n",
    );
    let run = cli(&[&path.display().to_string()]);
    assert_eq!(run.stdout, "hello\nbye\n");
    assert_eq!(run.code, 0);
}

#[test]
fn cli_filter_and_no_tests_found() {
    let dir = TempDir::new("testing-cli-filter");
    let path = write(&dir, "t.bas", "TEST Alpha\nEND TEST\nTEST Beta\nEND TEST\n");
    let shown = path.display().to_string();
    let run = cli(&["--test", "--test-filter", "ALP", &shown]);
    assert_eq!(
        run.stdout,
        "PASS Alpha\n1 tests: 1 passed, 0 failed, 0 errors\n"
    );
    assert_eq!(run.code, 0);

    let run = cli(&["--test", "--test-filter", "zzz", &shown]);
    assert_eq!(run.stdout, "");
    assert_eq!(run.stderr, "basic-vm: no tests found\n");
    assert_eq!(run.code, 1);

    let empty = write(&dir, "empty.bas", "PRINT 1\n");
    let run = cli(&["--test", &empty.display().to_string()]);
    assert_eq!(run.stdout, "");
    assert_eq!(run.stderr, "basic-vm: no tests found\n");
    assert_eq!(run.code, 1);
}

#[test]
fn cli_imports_flag_includes_module_tests() {
    let dir = TempDir::new("testing-cli-imports");
    write(
        &dir,
        "lib/m.bas",
        "FUNCTION One\n  One = 1\nFEND\nTEST One_is_one\n  ASSERTEQ 1, One()\nEND TEST\n",
    );
    let main = write(
        &dir,
        "main.bas",
        "IMPORT \"lib/m.bas\" AS M\nTEST Main_calls\n  ASSERTEQ 1, M.One()\nEND TEST\n",
    );
    let shown = main.display().to_string();
    let without = cli(&["--test", &shown]);
    assert_eq!(
        without.stdout,
        "PASS Main_calls\n1 tests: 1 passed, 0 failed, 0 errors\n"
    );
    let with = cli(&["--test", "--test-imports", &shown]);
    assert_eq!(
        with.stdout,
        "PASS Main_calls\nPASS M.One_is_one\n2 tests: 2 passed, 0 failed, 0 errors\n"
    );
    assert_eq!(with.code, 0);
    // A module run directly runs its own tests, unqualified.
    let module = dir.file("lib/m.bas").display().to_string();
    let direct = cli(&["--test", &module]);
    assert_eq!(
        direct.stdout,
        "PASS One_is_one\n1 tests: 1 passed, 0 failed, 0 errors\n"
    );
}

#[test]
fn cli_module_failures_show_the_module_file_and_line() {
    let dir = TempDir::new("testing-cli-module-fail");
    let module = write(&dir, "m.bas", "TEST Bad\n  ASSERTEQ 1, 2\nEND TEST\n");
    let main = write(&dir, "main.bas", "IMPORT \"m.bas\" AS M\n");
    let run = cli(&["--test", "--test-imports", &main.display().to_string()]);
    assert!(
        run.stdout
            .starts_with("FAIL M.Bad: ASSERTEQ failed: expected 1, got 2 ("),
        "{}",
        run.stdout
    );
    assert!(
        run.stdout
            .ends_with("m.bas:2)\n1 tests: 0 passed, 1 failed, 0 errors\n"),
        "{}",
        run.stdout
    );
    assert!(module.exists());
    assert_eq!(run.code, 1);
}

#[test]
fn cli_compile_errors_keep_the_usual_format_and_exit_code() {
    let dir = TempDir::new("testing-cli-compile");
    let path = write(&dir, "t.bas", "PRINT 1\nTEST A\nPRINT 2\n");
    let run = cli(&["--test", &path.display().to_string()]);
    assert_eq!(run.stdout, "");
    assert!(
        run.stderr
            .starts_with("basic-vm: line 2: unterminated TEST"),
        "{}",
        run.stderr
    );
    assert_eq!(run.code, 1);

    let nested = write(&dir, "n.bas", "FUNCTION F\nTEST A\nEND TEST\nFEND\n");
    let run = cli(&[&nested.display().to_string()]);
    assert!(
        run.stderr.starts_with("basic-vm: line 2:"),
        "{}",
        run.stderr
    );
    assert_eq!(run.code, 1);

    let run = cli(&["--test", &dir.text("missing.bas")]);
    assert_eq!(run.code, 1);
    assert_eq!(run.stdout, "");
}

#[test]
fn cli_misuse_of_the_test_mode_exits_with_two() {
    let dir = TempDir::new("testing-cli-misuse");
    let path = write(&dir, "t.bas", "TEST A\nEND TEST\n");
    let shown = path.display().to_string();
    let image = dir.text("x.png");
    let cases: Vec<Vec<&str>> = vec![
        vec!["--test", "--dump-bytecode", &shown],
        vec!["--test", "--headless-graphics", &shown],
        vec!["--test", "--screenshot", &image, &shown],
        vec!["--test", "--auto-close-ms", "5", &shown],
        vec!["--test", "--max-frames", "5", &shown],
        vec!["--test", &shown, "extra"],
        vec!["--test"],
        vec!["--test", "--test-filter"],
        vec!["--test-imports", &shown],
        vec!["--test-filter", "A", &shown],
    ];
    for args in cases {
        let run = cli(&args);
        assert_eq!(run.code, 2, "{args:?}: {}", run.stderr);
        assert_eq!(run.stdout, "", "{args:?}");
        assert!(
            run.stderr.starts_with("basic-vm: "),
            "{args:?}: {}",
            run.stderr
        );
    }
    // Nothing was run or written.
    assert!(!dir.file("x.png").exists());
}

#[test]
fn cli_tests_the_complex_number_library() {
    let library = common::examples("lib/complexutil.bas");
    let run = cli(&["--test", &library.display().to_string()]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    assert_eq!(run.stderr, "");
    let report: Vec<&str> = run.stdout.lines().collect();
    let (summary, tests) = report.split_last().unwrap();
    assert_eq!(tests.len(), 49, "{tests:?}");
    assert!(
        tests.iter().all(|line| line.starts_with("PASS ")),
        "{tests:?}"
    );
    assert_eq!(
        *summary,
        format!("{0} tests: {0} passed, 0 failed, 0 errors", tests.len())
    );
    assert!(tests.contains(&"PASS Divide_byZeroThrows"));

    // Through the program that imports it: only with --test-imports.
    let program = common::examples("complex.bas").display().to_string();
    let run = cli(&["--test", &program]);
    assert_eq!(run.code, 1);
    assert_eq!(run.stderr, "basic-vm: no tests found\n");
    let run = cli(&["--test", "--test-imports", &program]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    assert!(
        run.stdout.contains("PASS Cx.Divide_byZeroThrows\n"),
        "{}",
        run.stdout
    );
    assert_eq!(run.stdout.lines().count(), tests.len() + 1);
    let run = cli(&[
        "--test",
        "--test-imports",
        "--test-filter",
        "fromstring",
        &program,
    ]);
    assert_eq!(
        run.stdout,
        "PASS Cx.FromString_acceptedForms\nPASS Cx.FromString_exponentSignsAreNotSeparators\n\
         PASS Cx.FromString_signsAndSpacing\nPASS Cx.FromString_rejectsGarbage\n\
         PASS Cx.ToString_roundTripsThroughFromString\n\
         5 tests: 5 passed, 0 failed, 0 errors\n"
    );
}

#[test]
fn cli_runs_the_testing_example_and_its_normal_run_is_unchanged() {
    let example = common::examples("testing.bas").display().to_string();
    let run = cli(&["--test", &example]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    assert!(
        run.stdout
            .ends_with("8 tests: 8 passed, 0 failed, 0 errors\n"),
        "{}",
        run.stdout
    );
    let run = cli(&[&example]);
    assert_eq!(run.code, 0);
    assert_eq!(
        run.stdout,
        "Clamp(15, 0, 10) = 10\nReverse$(\"stressed\") = desserts\n"
    );
    // The library and its importing program behave as before in normal runs.
    let run = cli(&[&common::examples("complex.bas").display().to_string()]);
    assert_eq!(run.code, 0);
    assert!(run.stdout.starts_with("a = 3 + 4i, b = 1 + 2i\n"));
    assert!(
        run.stdout
            .ends_with("caught: invalid complex number: \"1 + 2x\"\n")
    );
    assert!(!run.stdout.contains("PASS"));
}

#[test]
fn cli_run_from_another_directory_uses_relative_imports() {
    let dir = TempDir::new("testing-cli-cwd");
    write(&dir, "sub/m.bas", "TEST T\n  ASSERT 1\nEND TEST\n");
    write(&dir, "sub/main.bas", "IMPORT \"m.bas\" AS M\n");
    let run = cli_in(&["--test", "--test-imports", "sub/main.bas"], Some(&dir.0));
    assert_eq!(
        run.stdout,
        "PASS M.T\n1 tests: 1 passed, 0 failed, 0 errors\n"
    );
    assert_eq!(run.code, 0);
}

// ----------------------------------------------- QA round 2 regressions

#[test]
fn runaway_recursion_in_a_test_is_an_error_and_later_tests_still_run() {
    let source = "FUNCTION Forever(PARAMETER n)\n  Forever = Forever(n + 1) + 1\nFEND\n\
TEST Runaway\n  PRINT Forever(0)\nEND TEST\nTEST Fine\n  ASSERT 1\nEND TEST\n";
    let started = std::time::Instant::now();
    let report = report(source);
    assert_eq!(
        lines(&report),
        vec![
            format!(
                "ERROR Runaway: call depth limit exceeded ({MAX_CALL_DEPTH} frames) calling FOREVER; \
                 is a function calling itself without end? (line 2, in function FOREVER)"
            ),
            "PASS Fine".to_string()
        ]
    );
    assert!(started.elapsed().as_secs() < 30, "{:?}", started.elapsed());
}

#[test]
fn cli_prints_each_result_as_soon_as_its_test_finishes() {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    use std::sync::mpsc;
    use std::time::Duration;

    let dir = TempDir::new("testing-cli-stream");
    let path = write(
        &dir,
        "t.bas",
        "TEST Quick\n  PRINT \"x\"\nEND TEST\nTEST Bad\n  PRINT \"shown\"\n  ASSERT 0\nEND TEST\n\
         TEST Hangs\n  DO\n  LOOP\nEND TEST\n",
    );
    let shown = path.display().to_string();
    let mut child = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .args(["--test", &shown])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    // The third test never ends; the first results must still arrive.
    let mut seen = Vec::new();
    while seen.len() < 3 {
        match receiver.recv_timeout(Duration::from_secs(30)) {
            Ok(line) => seen.push(line),
            Err(_) => break,
        }
    }
    child.kill().unwrap();
    child.wait().unwrap();
    reader.join().unwrap();
    assert_eq!(
        seen,
        vec![
            "PASS Quick".to_string(),
            format!("FAIL Bad: ASSERT failed ({shown}:6)"),
            "    shown".to_string()
        ]
    );
}

#[test]
fn sleep_returns_immediately_in_tests_but_timer_still_advances() {
    let source = "TEST Slow\n  t = TIMER()\n  SLEEP 3600000\n  ASSERT TIMER() - t >= 3600, \"timer\"\nEND TEST\n\
TEST Polling\n  t = TIMER()\n  DO WHILE TIMER() < t + 100\n    SLEEP 50\n  LOOP\nEND TEST\n";
    let started = std::time::Instant::now();
    let report = report(source);
    assert_eq!(lines(&report), vec!["PASS Slow", "PASS Polling"]);
    assert!(
        started.elapsed().as_secs() < 20,
        "SLEEP really slept: {:?}",
        started.elapsed()
    );
}

#[test]
fn messages_in_the_report_escape_control_characters() {
    let source = "TEST Fails\n  ASSERT 0, \"tab\there\rcr \u{1b}esc \u{2028}sep \u{e9}\u{2713}\"\nEND TEST\n\
TEST Throws\n  THROW \"a\tb\u{7}c\"\nEND TEST\n\
TEST Equal\n  ASSERTEQ \"x\ty\", \"z\", \"m\tn\"\nEND TEST\n";
    let report = report(source);
    assert_eq!(
        lines(&report),
        vec![
            "FAIL Fails: ASSERT failed - tab\\there\\rcr \\u{1b}esc \\u{2028}sep \u{e9}\u{2713} (line 2)",
            "ERROR Throws: a\\tb\\u{7}c (line 5)",
            "FAIL Equal: ASSERTEQ failed: expected \"x\\ty\", got \"z\" - m\\tn (line 8)",
        ]
    );
    // The result keeps the raw message; only the report text is escaped.
    assert!(result_of(&report, "Throws").message.contains('\t'));
    let text = report.render("t.bas");
    assert!(
        text.chars().all(|c| !c.is_control() || c == '\n'),
        "{text:?}"
    );
}

#[test]
fn cli_report_stays_one_line_per_test_for_multi_line_messages() {
    let dir = TempDir::new("testing-cli-newlines");
    let path = write(
        &dir,
        "t.bas",
        "TEST Multi\n  ASSERT 0, ENV$(\"BASIC_TEST_MESSAGE\")\nEND TEST\n\
         TEST Thrown\n  THROW ENV$(\"BASIC_TEST_MESSAGE\")\nEND TEST\n",
    );
    let shown = path.display().to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .args(["--test", &shown])
        .env("BASIC_TEST_MESSAGE", "first\nsecond\r\nthird")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout,
        format!(
            "FAIL Multi: ASSERT failed - first\\nsecond\\r\\nthird ({shown}:2)\n\
             ERROR Thrown: first\\nsecond\\r\\nthird ({shown}:5)\n\
             2 tests: 0 passed, 1 failed, 1 errors\n"
        )
    );
}

#[test]
fn captured_output_is_capped_but_the_test_runs_on() {
    let pad = "x".repeat(60);
    let source = format!(
        "TEST Chatty\n  FOR i = 1 TO 3000\n    PRINT \"line \" + STR$(i) + \" {pad}\"\n  NEXT i\n  ASSERT 0, \"after the noise\"\nEND TEST\n\
         TEST Quiet\n  PRINT \"small\"\n  ASSERT 0\nEND TEST\n\
         TEST ChattyButFine\n  FOR i = 1 TO 3000\n    PRINT \"line \" + STR$(i) + \" {pad}\"\n  NEXT i\nEND TEST\n"
    );
    let total: usize = (1..=3000).map(|i| format!("line {i} {pad}\n").len()).sum();
    assert!(total > MAX_CAPTURED_OUTPUT);
    let report = report(&source);

    let chatty = result_of(&report, "Chatty");
    assert_eq!(chatty.status, TestStatus::Failed);
    assert_eq!(chatty.message, "ASSERT failed - after the noise");
    assert_eq!(chatty.output.len(), MAX_CAPTURED_OUTPUT);
    assert_eq!(chatty.output_omitted, total - MAX_CAPTURED_OUTPUT);
    assert!(chatty.output.starts_with("line 1 xxx"));
    let text = chatty.render("t.bas");
    assert!(
        text.ends_with(&format!(
            "    ... output truncated ({} bytes omitted)\n",
            total - MAX_CAPTURED_OUTPUT
        )),
        "{}",
        &text[text.len().saturating_sub(120)..]
    );

    // Small output has no note; a passing test keeps the cap too.
    let quiet = result_of(&report, "Quiet");
    assert_eq!(
        (quiet.output.as_str(), quiet.output_omitted),
        ("small\n", 0)
    );
    assert!(!quiet.render("t.bas").contains("truncated"));
    let fine = result_of(&report, "ChattyButFine");
    assert_eq!(fine.status, TestStatus::Passed);
    assert_eq!(fine.output_omitted, total - MAX_CAPTURED_OUTPUT);
    assert_eq!(fine.render("t.bas"), "PASS ChattyButFine\n");
}

#[test]
fn the_output_cap_never_splits_a_character() {
    let source = "TEST Wide\n  FOR i = 1 TO 20000\n    PRINT \"\u{e9}\u{e9}\u{e9}\u{e9}\"\n  NEXT i\n  ASSERT 0\nEND TEST\n";
    let report = report(source);
    let wide = &report.results[0];
    // Four two-byte characters and a newline per line.
    assert_eq!(wide.output.len() + wide.output_omitted, 20000 * 9);
    assert!(wide.output.len() <= MAX_CAPTURED_OUTPUT);
    // Every complete line is intact; only the final one may be cut short.
    let complete = wide.output.rsplit_once('\n').unwrap().0;
    assert!(
        complete
            .lines()
            .all(|line| line == "\u{e9}\u{e9}\u{e9}\u{e9}")
    );
}

#[test]
fn end_and_stop_inside_a_test_are_errors_not_passes() {
    let source = "TEST E\n  PRINT \"a\"\n  END\n  ASSERT 0, \"not reached\"\nEND TEST\n\
TEST S\n  STOP\nEND TEST\n\
FUNCTION F\n  END\nFEND\n\
TEST InFn\n  CALL F\nEND TEST\n\
TEST InTry\n  TRY\n    STOP\n  CATCH e$\n    PRINT \"caught\"\n  END TRY\n  ASSERT 0, \"after\"\nEND TEST\n\
TEST After\n  ASSERT 1\nEND TEST\n\
TEST Early\n  EXIT TEST\nEND TEST\n";
    let report = report(source);
    assert_eq!(
        lines(&report),
        vec![
            "ERROR E: test ended by END (line 3)",
            "ERROR S: test ended by STOP (line 7)",
            "ERROR InFn: test ended by END (line 10, in function F)",
            "ERROR InTry: test ended by STOP (line 17)",
            "PASS After",
            "PASS Early",
        ]
    );
    assert_eq!(result_of(&report, "E").status, TestStatus::Errored);
    assert_eq!(result_of(&report, "E").output, "a\n");
    // The program cannot catch it.
    assert_eq!(result_of(&report, "InTry").output, "");
}

#[test]
fn end_and_stop_still_halt_normal_runs() {
    assert_eq!(output_of("PRINT 1\nEND\nPRINT 2\n"), "1\n");
    assert_eq!(output_of("PRINT 1\nSTOP\nPRINT 2\n"), "1\n");
    assert_eq!(
        output_of("FUNCTION F\n  PRINT \"in\"\n  END\nFEND\nCALL F\nPRINT \"no\"\n"),
        "in\n"
    );
}

#[test]
fn the_same_module_file_under_two_aliases_runs_its_tests_once_under_the_first() {
    let dir = TempDir::new("testing-alias-same-file");
    write(&dir, "m.bas", "TEST T1\nEND TEST\n");
    let main = write(
        &dir,
        "main.bas",
        "IMPORT \"m.bas\" AS One\nIMPORT \"m.bas\" AS Two\nTEST M\nEND TEST\n",
    );
    let program = compile_file(&main).unwrap();
    let options = TestOptions {
        include_imports: true,
        ..TestOptions::default()
    };
    assert_eq!(
        lines(&run_tests(&program, &options)),
        vec!["PASS M", "PASS One.T1"]
    );
}

#[test]
fn a_different_module_file_reusing_a_taken_alias_gets_a_numbered_suffix() {
    let dir = TempDir::new("testing-alias-clash");
    write(&dir, "m.bas", "TEST T1\nEND TEST\n");
    write(&dir, "n.bas", "TEST T2\nEND TEST\n");
    write(
        &dir,
        "a.bas",
        "IMPORT \"n.bas\" AS Inner\nTEST TA\nEND TEST\n",
    );
    let main = write(
        &dir,
        "main.bas",
        "IMPORT \"a.bas\" AS A\nIMPORT \"m.bas\" AS inner\nTEST M\nEND TEST\n",
    );
    let program = compile_file(&main).unwrap();
    let options = TestOptions {
        include_imports: true,
        ..TestOptions::default()
    };
    assert_eq!(
        lines(&run_tests(&program, &options)),
        vec!["PASS M", "PASS A.TA", "PASS Inner.T2", "PASS inner#2.T1"]
    );
}

#[test]
fn parenthesised_assert_arguments_get_a_helpful_compile_error() {
    for source in [
        "ASSERTEQ (1, 1)\n",
        "ASSERTEQ (1, 1), \"m\"\n",
        "ASSERT (1, 2)\n",
        "ASSERTNEAR (1, 2, 3)\n",
    ] {
        let message = compile_error(source);
        assert!(
            message.starts_with("line 1:") && message.contains("without parentheses"),
            "{source:?} -> {message}"
        );
    }
    assert!(compile_error("ASSERTEQ (1, 1)\n").contains("ASSERTEQ expected, actual"));
    // Parentheses around a single operand or a sub-expression stay valid.
    passes("ASSERT (1 > 0) AND 1\n");
    passes("ASSERT (1)\n");
    passes("ASSERT (1), \"message\"\n");
    passes("ASSERTEQ (1 + 2) * 3, 9\n");
    passes("ASSERTEQ (1), (1), \"m\"\n");
    passes("ASSERTNEAR (1), (1.05), (0.1)\n");
    // Other compile errors do not get the hint.
    let message = compile_error("ASSERT (1 +)\n");
    assert!(!message.contains("without parentheses"), "{message}");
}

#[test]
fn asserteq_of_the_same_collection_is_equal() {
    passes("a = LIST(1, LIST(2, 3), MAP(\"k\", ARRAY(4)))\nASSERTEQ a, a\n");
    passes("a = LIST()\nAPPEND a, a\nASSERTEQ a, a\n");
}
