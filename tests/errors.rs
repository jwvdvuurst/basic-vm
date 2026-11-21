//! Structured error handling: TRY / CATCH / FINALLY / THROW.

use std::io::Cursor;

use basic_vm::{Error, Vm, compile};

fn run(source: &str) -> (String, Result<(), Error>, Vm) {
    let mut vm = Vm::new(compile(source).unwrap()).unwrap();
    let mut output = Vec::new();
    let result = vm.run(&mut Cursor::new(""), &mut output);
    (String::from_utf8(output).unwrap(), result, vm)
}

fn run_ok(source: &str) -> String {
    let (output, result, _vm) = run(source);
    result.unwrap_or_else(|error| panic!("{error}\n{source}"));
    output
}

#[test]
fn blocks_run_in_order_and_catch_receives_the_message() {
    let source = "TRY\nPRINT \"body\"\nCATCH E$\nPRINT \"catch\"\nFINALLY\nPRINT \"finally\"\nEND TRY\n\
                  TRY\nPRINT \"body2\"\nX = 1 / 0\nPRINT \"not reached\"\nCATCH E$\n\
                  PRINT \"caught: \"; E$\nFINALLY\nPRINT \"finally2\"\nEND TRY\nPRINT \"after\"";
    assert_eq!(
        run_ok(source),
        "body\nfinally\nbody2\ncaught: division by zero\nfinally2\nafter\n"
    );
}

#[test]
fn try_with_only_catch_or_only_finally_and_catch_without_a_variable() {
    let source = "TRY\nTHROW \"a\"\nCATCH\nPRINT \"swallowed explicitly\"\nEND TRY\n\
                  TRY\nPRINT \"only finally\"\nFINALLY\nPRINT \"f\"\nEND TRY\n\
                  TRY\nPRINT \"only catch, no error\"\nCATCH\nPRINT \"no\"\nEND TRY";
    assert_eq!(
        run_ok(source),
        "swallowed explicitly\nonly finally\nf\nonly catch, no error\n"
    );
}

#[test]
fn uncaught_error_runs_finally_then_keeps_its_message_and_line() {
    let (output, result, _vm) =
        run("TRY\nPRINT \"a\"\nTHROW \"boom\"\nFINALLY\nPRINT \"cleanup\"\nEND TRY\nPRINT \"no\"");
    assert_eq!(output, "a\ncleanup\n");
    let error = result.unwrap_err();
    assert_eq!((error.line, error.message.as_str()), (3, "boom"));
    assert_eq!(error.function, None);
}

#[test]
fn uncaught_error_inside_a_function_reports_the_function() {
    let (output, result, _vm) = run("FUNCTION F\nF = 1 / 0\nFEND\nTRY\nPRINT F()\nFINALLY\n\
         PRINT \"fin\"\nEND TRY");
    assert_eq!(output, "fin\n");
    let error = result.unwrap_err();
    assert_eq!(error.line, 2);
    assert_eq!(error.function.as_deref(), Some("F"));
    assert!(error.message.contains("division by zero"));
    assert_eq!(
        error.to_string(),
        "line 2: division by zero (in function F)"
    );
}

#[test]
fn nested_handlers_and_bare_throw_rethrow_the_original_error() {
    let source = "TRY\nTRY\nTHROW \"inner\"\nCATCH E$\nPRINT \"inner caught \"; E$\nTHROW\n\
                  FINALLY\nPRINT \"inner finally\"\nEND TRY\nPRINT \"skipped\"\n\
                  CATCH E$\nPRINT \"outer caught \"; E$; \" line \"; ERRLINE()\n\
                  FINALLY\nPRINT \"outer finally\"\nEND TRY";
    assert_eq!(
        run_ok(source),
        "inner caught inner\ninner finally\nouter caught inner line 3\nouter finally\n"
    );
}

#[test]
fn bare_throw_in_a_nested_try_inside_a_catch_targets_the_enclosing_catch() {
    let source = "TRY\nTHROW \"outer\"\nCATCH E$\nTRY\nPRINT \"nested body\"\nTHROW\n\
                  FINALLY\nPRINT \"nested finally\"\nEND TRY\nEND TRY";
    let (output, result, _vm) = run(source);
    assert_eq!(output, "nested body\nnested finally\n");
    let error = result.unwrap_err();
    assert_eq!((error.line, error.message.as_str()), (2, "outer"));
}

#[test]
fn errors_in_catch_and_finally_replace_the_current_error() {
    let source = "TRY\nTRY\nTHROW \"a\"\nCATCH E$\nPRINT \"c1 \"; E$\nTHROW E$ + \"b\"\nFINALLY\n\
                  PRINT \"f1\"\nEND TRY\nCATCH E$\nPRINT \"c2 \"; E$\nEND TRY\n\
                  TRY\nTRY\nTHROW \"first\"\nFINALLY\nTHROW \"second\"\nEND TRY\nCATCH E$\n\
                  PRINT E$\nEND TRY";
    assert_eq!(run_ok(source), "c1 a\nf1\nc2 ab\nsecond\n");
}

#[test]
fn unwinding_through_calls_loops_and_snapshots_keeps_the_vm_consistent() {
    let source = "FUNCTION DEEP(PARAMETER N)\nLOCAL ITEM\nFOR EACH ITEM IN LIST(1, 2, 3)\n\
                  IF N = 0 THEN THROW \"bottom \" + STR$(ITEM)\nDEEP = DEEP(N - 1)\nNEXT\nFEND\n\
                  FUNCTION HALF(PARAMETER X)\nHALF = 1 / X\nFEND\n\
                  FOR I = 1 TO 3\nTRY\nR = DEEP(3)\nCATCH E$\n\
                  PRINT I; \" \"; E$; \" \"; ERRFUNC$()\nEND TRY\nNEXT\n\
                  TRY\nPRINT 10 + 5 * HALF(0)\nCATCH E$\nPRINT \"caught\"\nEND TRY\n\
                  PRINT 7 * 6; \" \"; HALF(2)\n\
                  GC\nL = LIST(\"a\", \"b\")\nFOR EACH S$ IN L\nPRINT S$\nNEXT\n\
                  FOR EACH S IN LIST(1, 2)\nTRY\nGC\nTHROW \"in loop \" + STR$(S)\nCATCH E$\nPRINT E$\n\
                  END TRY\nNEXT";
    let (output, result, vm) = run(source);
    result.unwrap();
    assert_eq!(
        output,
        "1 bottom 1 DEEP\n2 bottom 1 DEEP\n3 bottom 1 DEEP\ncaught\n42 0.5\na\nb\n\
         in loop 1\nin loop 2\n"
    );
    assert!(vm.heap_stats().collections >= 3);
}

#[test]
fn collection_survives_gc_after_the_error_unwound_the_frame_that_held_it() {
    let source = "KEEP = LIST(\"x\", \"y\")\nFUNCTION BAD\nLOCAL T$\nT$ = \"temporary\" + \"text\"\n\
                  APPEND KEEP, T$\nBAD = 1 / 0\nFEND\nTRY\nCALL BAD\nCATCH E$\nGC\nEND TRY\n\
                  PRINT KEEP[3]; \" \"; LEN(KEEP)";
    assert_eq!(run_ok(source), "temporarytext 3\n");
}

#[test]
fn exit_and_continue_run_finally_blocks() {
    let source = "FOR I = 1 TO 5\nTRY\nIF I = 2 THEN CONTINUE FOR\nIF I = 4 THEN EXIT FOR\n\
                  PRINT \"body \"; I\nFINALLY\nPRINT \"finally \"; I\nEND TRY\nNEXT\nPRINT \"done \"; I\n\
                  J = 0\nDO\nJ = J + 1\nTRY\nIF J = 3 THEN EXIT DO\nFINALLY\nPRINT \"do \"; J\nEND TRY\nLOOP\n\
                  FUNCTION F(PARAMETER X)\nTRY\nTRY\nF = 1\nIF X > 0 THEN EXIT FUNCTION\nF = 2\n\
                  FINALLY\nPRINT \"inner finally\"\nEND TRY\nF = 3\nFINALLY\nPRINT \"outer finally\"\n\
                  END TRY\nFEND\nPRINT F(1); \" \"; F(0)";
    assert_eq!(
        run_ok(source),
        "body 1\nfinally 1\nfinally 2\nbody 3\nfinally 3\nfinally 4\ndone 4\n\
         do 1\ndo 2\ndo 3\n\
         inner finally\nouter finally\n1 inner finally\nouter finally\n3\n"
    );
}

#[test]
fn exit_from_catch_and_nested_loops_through_try_blocks() {
    let source = "FOR I = 1 TO 2\nFOR EACH V IN LIST(10, 20, 30)\nTRY\nTHROW \"x\"\nCATCH E$\n\
                  IF V = 20 THEN EXIT FOR\nFINALLY\nPRINT \"f \"; I; \" \"; V\nEND TRY\nNEXT\nNEXT\n\
                  PRINT \"end \"; I";
    assert_eq!(run_ok(source), "f 1 10\nf 1 20\nf 2 10\nf 2 20\nend 3\n");
}

#[test]
fn exit_inside_finally_abandons_a_pending_error() {
    let source =
        "FOR I = 1 TO 2\nTRY\nTHROW \"lost\"\nFINALLY\nEXIT FOR\nEND TRY\nNEXT\nPRINT \"ok \"; I";
    assert_eq!(run_ok(source), "ok 1\n");
}

#[test]
fn err_functions_describe_the_error_being_handled() {
    let source = "FUNCTION BOOM\nTHROW \"from boom\"\nFEND\n\
                  TRY\nCALL BOOM\nCATCH\nPRINT ERR$(); \"|\"; ERRLINE(); \"|\"; ERRFUNC$()\nEND TRY\n\
                  TRY\nTHROW \"main\"\nCATCH\nPRINT ERR$(); \"|\"; ERRLINE(); \"|\"; LEN(ERRFUNC$())\nEND TRY\n\
                  TRY\nTHROW \"p\"\nFINALLY\nPRINT ERR$()\nEND TRY";
    let (output, result, _vm) = run(source);
    assert_eq!(output, "from boom|2|BOOM\nmain|10|0\np\n");
    assert_eq!(result.unwrap_err().message, "p");
    let error = run("PRINT ERR$()").1.unwrap_err();
    assert!(
        error.message.contains("no error is being handled"),
        "{error}"
    );
}

#[test]
fn runtime_errors_of_every_kind_are_catchable() {
    for (statement, message) in [
        ("X = 1 / 0", "division by zero"),
        ("X = 5 MOD 0", "MOD divisor must not be zero"),
        ("X = \"a\" + 1", "expected a number"),
        ("X = LIST(1)[5]", "index out of bounds"),
        ("X = UNDEFINED_NAME", "undefined variable"),
        ("X$ = 5", "type mismatch"),
        ("X = SQRT(-1)", "SQRT: argument must not be negative"),
        (
            "OPEN \"no-such-dir-basic-vm/x.txt\" FOR INPUT AS #1",
            "file not found",
        ),
        ("THROW 5", "THROW requires a string message"),
        ("INPUT X", "unexpected end of input"),
    ] {
        let source = format!("TRY\n{statement}\nCATCH E$\nPRINT E$\nEND TRY\nPRINT \"next\"");
        let output = run_ok(&source);
        assert!(
            output.contains(message) && output.ends_with("next\n"),
            "{statement}: {output}"
        );
    }
}

#[test]
fn end_inside_try_halts_without_running_finally() {
    assert_eq!(
        run_ok("TRY\nPRINT \"in\"\nEND\nFINALLY\nPRINT \"never\"\nEND TRY"),
        "in\n"
    );
}

#[test]
fn many_sequential_and_recursive_handlers_do_not_leak_handler_state() {
    let source = "FUNCTION R(PARAMETER N)\nTRY\nIF N = 0 THEN THROW \"base\"\nR = R(N - 1)\n\
                  FINALLY\nCOUNT = COUNT + 1\nEND TRY\nFEND\nCOUNT = 0\nFOR I = 1 TO 200\nTRY\n\
                  X = R(5)\nCATCH E$\nOKAY = OKAY + 1\nEND TRY\nNEXT\nPRINT COUNT; \" \"; OKAY";
    assert_eq!(run_ok(&format!("OKAY = 0\n{source}")), "1200 200\n");
}

#[test]
fn compile_errors_for_malformed_blocks() {
    for (source, line, message) in [
        ("TRY\nPRINT 1\nEND TRY", 3, "CATCH block, a FINALLY block"),
        ("CATCH E$", 1, "CATCH without a matching TRY"),
        ("FINALLY", 1, "FINALLY without a matching TRY"),
        ("END TRY", 1, "END TRY without a matching TRY"),
        ("THROW", 1, "only valid inside a CATCH block"),
        (
            "TRY\nTHROW\nCATCH\nEND TRY",
            2,
            "only valid inside a CATCH block",
        ),
        (
            "TRY\nTHROW\nFINALLY\nEND TRY",
            2,
            "only valid inside a CATCH block",
        ),
        (
            "TRY\nTHROW \"x\"\nCATCH\nEND TRY\nTHROW",
            5,
            "only valid inside a CATCH block",
        ),
        ("TRY\nCATCH\nCATCH\nEND TRY", 3, "may appear only once"),
        ("TRY\nFINALLY\nCATCH\nEND TRY", 3, "may appear only once"),
        ("TRY\nFINALLY\nFINALLY\nEND TRY", 3, "only one FINALLY"),
        ("TRY\nPRINT 1", 1, "unterminated TRY"),
        ("TRY\nCATCH X\nEND TRY", 2, "must be a string variable"),
        ("IF 1 THEN TRY", 1, "own lines"),
        (
            "TRY\nGOTO 10\nCATCH\nEND TRY\n10 PRINT 1",
            2,
            "GOTO is not allowed",
        ),
        (
            "TRY\nIF 1 THEN 10\nCATCH\nEND TRY\n10 PRINT 1",
            2,
            "GOTO is not allowed",
        ),
        (
            "GOTO 10\nTRY\n10 PRINT 1\nCATCH\nEND TRY",
            1,
            "inside a TRY block",
        ),
        ("WHILE 1\nTRY\nWEND", 3, "WEND must match"),
        ("FUNCTION F\nTRY\nFEND", 3, "FEND cannot close"),
        ("TRY\nFUNCTION F\nFEND\nCATCH\nEND TRY", 2, "global scope"),
    ] {
        let error = compile(source).unwrap_err();
        assert_eq!(error.line, line, "{source}");
        assert!(error.message.contains(message), "{source}: {error}");
    }
}

#[test]
fn goto_and_labels_outside_try_blocks_still_work_around_them() {
    let source = "GOTO 20\n10 PRINT \"ten\"\nGOTO 30\n20 PRINT \"twenty\"\nTRY\nPRINT \"try\"\n\
                  FINALLY\nPRINT \"fin\"\nEND TRY\nGOTO 10\n30 PRINT \"end\"";
    assert_eq!(run_ok(source), "twenty\ntry\nfin\nten\nend\n");
}

#[test]
fn variables_named_like_new_file_statements_still_work() {
    let source = "OPEN = 1\nCLOSE = 2\nPUT = 3\nSEEK = 4\nLINE = 5\nGET = 6\nEOF = 7\n\
                  PRINT OPEN + CLOSE + PUT + SEEK + LINE + GET + EOF";
    assert_eq!(run_ok(source), "28\n");
}

#[test]
fn variables_named_like_try_statements_still_work() {
    let source = "TRY = 1\nCATCH = 2\nFINALLY = 3\nTHROW = 4\nPRINT TRY + CATCH + FINALLY + THROW";
    assert_eq!(run_ok(source), "10\n");
    assert_eq!(
        run_ok(
            "TRY = ARRAY(0, 0)\nTHROW = ARRAY(0, 0)\nTRY[1] = 5\nTHROW[2] = 6\nPRINT TRY[1] + THROW[2]"
        ),
        "11\n"
    );
}
