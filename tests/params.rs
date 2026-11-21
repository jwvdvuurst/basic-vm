//! Function parameter enhancements: BYVAL/BYREF, optional parameters, named arguments.

use std::io::Cursor;

use basic_vm::{Error, Vm, compile};

fn run(source: &str) -> (String, Result<(), Error>) {
    let mut vm = Vm::new(compile(source).unwrap_or_else(|e| panic!("{e}\n{source}"))).unwrap();
    let mut output = Vec::new();
    let result = vm.run(&mut Cursor::new(""), &mut output);
    (String::from_utf8(output).unwrap(), result)
}

fn ok(source: &str) -> String {
    let (output, result) = run(source);
    result.unwrap_or_else(|error| panic!("{error}\n{source}"));
    output
}

fn compile_error(source: &str) -> String {
    match compile(source) {
        Err(error) => error.message,
        Ok(_) => panic!("expected a compile error\n{source}"),
    }
}

fn runtime_error(source: &str) -> String {
    run(source).1.unwrap_err().message
}

#[test]
fn byval_is_the_default_and_does_not_write_back() {
    let out = ok("X = 1\nCALL BUMP(X)\nPRINT X\nCALL BUMP2(X)\nPRINT X\n\
         FUNCTION BUMP(PARAMETER N)\nN = N + 10\nFEND\n\
         FUNCTION BUMP2(PARAMETER BYVAL N)\nN = N + 10\nFEND");
    assert_eq!(out, "1\n1\n");
}

#[test]
fn byref_writes_back_globals_locals_and_parameters() {
    let out = ok(
        "DIM X AS INTEGER = 1\nCALL BUMP(X)\nPRINT X\nCALL OUTER(5)\n\
         FUNCTION BUMP(PARAMETER BYREF N AS INTEGER)\nN = N + 10\nFEND\n\
         FUNCTION OUTER(PARAMETER P AS INTEGER)\nLOCAL L AS INTEGER\nL = 100\nCALL BUMP(L)\nPRINT L\n\
         CALL BUMP(P)\nPRINT P\nFEND",
    );
    assert_eq!(out, "11\n110\n15\n");
}

#[test]
fn byref_a_function_can_swap_two_variables() {
    let out = ok("A = 1\nB = 2\nCALL SWAP(A, B)\nPRINT A; \",\"; B\n\
         FUNCTION SWAP(PARAMETER BYREF X, PARAMETER BYREF Y)\nLOCAL T\nT = X\nX = Y\nY = T\nFEND");
    assert_eq!(out, "2,1\n");
}

#[test]
fn byref_elements_and_fields_write_back() {
    let out = ok("TYPE Pt\n x AS INTEGER\n y AS INTEGER\nEND TYPE\n\
         DIM A(2)\nA[1] = 5\nCALL INC(A[1])\nPRINT A[1]\n\
         L = LIST(1, 2, 3)\nCALL INC(L[2])\nPRINT L[2]\n\
         M = MAP(\"k\", 1)\nCALL INC(M[\"k\"])\nPRINT M[\"k\"]\n\
         P = NEW Pt(x = 1, y = 2)\nCALL INC(P.y)\nPRINT P.x; P.y\n\
         FUNCTION INC(PARAMETER BYREF N)\nN = N + 1\nFEND");
    assert_eq!(out, "6\n3\n2\n13\n");
}

#[test]
fn byref_with_recursion() {
    let out = ok("T = 0\nCALL SUMTO(4, T)\nPRINT T\n\
         FUNCTION SUMTO(PARAMETER N, PARAMETER BYREF ACC)\n\
         IF N = 0 THEN EXIT FUNCTION\nACC = ACC + N\nCALL SUMTO(N - 1, ACC)\nFEND");
    assert_eq!(out, "10\n");
}

#[test]
fn byref_writes_back_on_exit_function() {
    let out = ok("X = 1\nCALL F(X)\nPRINT X\n\
         FUNCTION F(PARAMETER BYREF N)\nN = 42\nEXIT FUNCTION\nN = 99\nFEND");
    assert_eq!(out, "42\n");
}

#[test]
fn byref_is_copy_in_copy_out_and_skipped_when_an_error_unwinds() {
    let out = ok(
        "X = 1\nTRY\nCALL F(X)\nCATCH E$\nPRINT \"caught \"; E$\nEND TRY\nPRINT X\n\
         CALL G(X)\nPRINT X\n\
         FUNCTION F(PARAMETER BYREF N)\nN = 50\nTHROW \"boom\"\nFEND\n\
         FUNCTION G(PARAMETER BYREF N)\nTRY\nN = 7\nTHROW \"inner\"\nCATCH E$\nN = N + 1\nEND TRY\nFEND",
    );
    assert_eq!(out, "caught boom\n1\n8\n");
}

#[test]
fn byref_checks_the_callers_declared_type_at_write_back() {
    let message = runtime_error(
        "DIM X AS INTEGER = 1\nCALL F(X)\n\
         FUNCTION F(PARAMETER BYREF N)\nN = 1.5\nFEND",
    );
    assert!(message.contains("type mismatch"), "{message}");
}

#[test]
fn byref_requires_an_assignable_argument() {
    for argument in ["X + 1", "5", "\"s\"", "F(X)", "(X)"] {
        let message = compile_error(&format!(
            "X = 1\nCALL F({argument})\nFUNCTION F(PARAMETER BYREF N)\nFEND"
        ));
        assert!(
            message.contains("BYREF argument must be"),
            "{argument}: {message}"
        );
    }
    let message = compile_error("CONST C = 1\nCALL F(C)\nFUNCTION F(PARAMETER BYREF N)\nFEND");
    assert!(message.contains("constant"), "{message}");
}

#[test]
fn collections_and_records_are_shared_by_default() {
    let out = ok(
        "TYPE Pt\n x AS INTEGER\nEND TYPE\nL = LIST(1)\nP = NEW Pt(x = 1)\n\
         CALL F(L, P)\nPRINT LEN(L); P.x\n\
         FUNCTION F(PARAMETER BYVAL A, PARAMETER B)\nAPPEND A, 2\nB.x = 9\nA = LIST()\nFEND",
    );
    assert_eq!(out, "29\n");
}

#[test]
fn byref_of_a_collection_rebinds_the_callers_variable() {
    let out = ok("L = LIST(1)\nCALL F(L)\nPRINT LEN(L)\n\
         FUNCTION F(PARAMETER BYREF A)\nA = LIST(1, 2, 3)\nFEND");
    assert_eq!(out, "3\n");
}

#[test]
fn modifier_words_are_contextual_parameter_names() {
    let out = ok("PRINT F(3)\nFUNCTION F(PARAMETER BYREF)\nF = BYREF * 2\nFEND");
    assert_eq!(out, "6\n");
}

#[test]
fn byref_parameter_with_default_or_optional_is_an_error() {
    assert!(
        compile_error("FUNCTION F(PARAMETER BYREF N = 1)\nFEND").contains("cannot be OPTIONAL")
    );
    assert!(
        compile_error("FUNCTION F(PARAMETER BYREF OPTIONAL N)\nFEND")
            .contains("cannot be OPTIONAL")
    );
    assert!(compile_error("FUNCTION F(PARAMETER BYVAL BYREF N)\nFEND").contains("only one of"));
}

#[test]
fn optional_parameters_with_defaults() {
    let out = ok(
        "PRINT ADD(1)\nPRINT ADD(1, 2)\nPRINT GREET$()\nPRINT GREET$(\"Bo\")\nPRINT OPT()\n\
         FUNCTION ADD(PARAMETER A, PARAMETER N = 10)\nADD = A + N\nFEND\n\
         FUNCTION GREET$(PARAMETER NAME$ = \"World\")\nGREET$ = \"Hi \" + NAME$\nFEND\n\
         FUNCTION OPT(PARAMETER OPTIONAL X AS INTEGER, PARAMETER OPTIONAL S$)\n\
         OPT = X + LEN(S$)\nFEND",
    );
    assert_eq!(out, "11\n3\nHi World\nHi Bo\n0\n");
}

#[test]
fn defaults_are_typed_and_checked() {
    let out = ok(
        "PRINT F()\nFUNCTION F(PARAMETER D AS DOUBLE = 2, PARAMETER B AS BOOLEAN = TRUE, \
         PARAMETER N = -3)\nF = D + B + N\nFEND",
    );
    assert_eq!(out, "0\n");
    assert!(compile_error("FUNCTION F(PARAMETER N = \"x\")\nFEND").contains("does not fit"));
    assert!(
        compile_error("FUNCTION F(PARAMETER N AS INTEGER = 1.5)\nFEND").contains("does not fit")
    );
    assert!(compile_error("FUNCTION F(PARAMETER N = 1 + 2)\nFEND").contains("must be a literal"));
}

#[test]
fn required_parameters_cannot_follow_optional_ones() {
    let message = compile_error("FUNCTION F(PARAMETER A = 1, PARAMETER B)\nFEND");
    assert!(message.contains("cannot follow an optional"), "{message}");
}

#[test]
fn named_arguments_any_order_and_mixed_with_positional() {
    let out = ok(
        "PRINT SUB(10, 3)\nPRINT SUB(B := 3, A := 10)\nPRINT SUB(10, B := 3)\n\
         PRINT SCALE(2, K := 5)\nPRINT SCALE(2)\nCALL SHOW(Y := 2, X := 1)\n\
         FUNCTION SUB(PARAMETER A, PARAMETER B)\nSUB = A - B\nFEND\n\
         FUNCTION SCALE(PARAMETER V, PARAMETER K = 3)\nSCALE = V * K\nFEND\n\
         FUNCTION SHOW(PARAMETER X, PARAMETER Y)\nPRINT X; \"-\"; Y\nFEND",
    );
    assert_eq!(out, "7\n7\n7\n10\n6\n1-2\n");
}

#[test]
fn named_arguments_evaluate_left_to_right() {
    let out = ok("PRINT F(B := T(\"b\"), A := T(\"a\"))\n\
         FUNCTION T(PARAMETER S$)\nPRINT S$\nT = 1\nFEND\n\
         FUNCTION F(PARAMETER A, PARAMETER B)\nF = A + B\nFEND");
    assert_eq!(out, "b\na\n2\n");
}

#[test]
fn named_arguments_with_byref() {
    let out = ok("X = 1\nY = 2\nCALL F(B := Y, A := X)\nPRINT X; Y\n\
         FUNCTION F(PARAMETER BYREF A, PARAMETER BYREF B)\nA = A + 10\nB = B + 20\nFEND");
    assert_eq!(out, "1122\n");
}

#[test]
fn named_argument_errors_are_compile_errors_even_for_later_functions() {
    let header = "FUNCTION F(PARAMETER A, PARAMETER B = 2)\nFEND\n";
    let cases = [
        ("CALL F(A := 1, C := 2)", "has no parameter C"),
        ("CALL F(A := 1, A := 2)", "given twice"),
        ("CALL F(1, A := 2)", "given twice"),
        ("CALL F(B := 2)", "missing required parameter A"),
        ("CALL F(A := 1, 2)", "positional argument cannot follow"),
        ("CALL F()", "requires 1 to 2 arguments, got 0"),
        ("CALL F(1, 2, 3)", "requires 1 to 2 arguments, got 3"),
        ("X = F(1, 2, 3)", "got 3"),
    ];
    for (call, expected) in cases {
        for source in [format!("{call}\n{header}"), format!("{header}{call}")] {
            let message = compile_error(&source);
            assert!(message.contains(expected), "{call}: {message}");
        }
    }
}

#[test]
fn builtins_reject_named_arguments() {
    let message = compile_error("PRINT ABS(X := 1)");
    assert!(message.contains("named arguments"), "{message}");
    let message = compile_error("L = LIST(A := 1)");
    assert!(message.contains("named arguments"), "{message}");
}

#[test]
fn legacy_arity_errors_are_unchanged() {
    let message = compile_error("PRINT F(1)\nFUNCTION F\nFEND");
    assert!(
        message.contains("function F requires 0 arguments, got 1"),
        "{message}"
    );
    let message = compile_error("PRINT F()\nFUNCTION F(PARAMETER A, PARAMETER B)\nFEND");
    assert!(
        message.contains("function F requires 2 arguments, got 0"),
        "{message}"
    );
}

#[test]
fn named_argument_marker_is_not_a_comparison() {
    // `=` keeps comparing; `:=` only names an argument.
    let out = ok("A = 1\nPRINT F(A = 1)\nFUNCTION F(PARAMETER V)\nF = V\nFEND");
    assert_eq!(out, "1\n");
}
