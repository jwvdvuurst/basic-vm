//! Phase 4A: exact integers, typed declarations, OPTION EXPLICIT, TYPEOF,
//! multi-dimensional arrays, REDIM and two-variable map iteration.

use std::io::Cursor;
use std::process::Command;

use basic_vm::{Error, Vm, compile};

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
    run(source).1.unwrap_err().message
}

fn compile_error(source: &str) -> (usize, String) {
    let Err(error) = compile(source) else {
        panic!("expected a compile error")
    };
    (error.line, error.message)
}

#[test]
fn integer_boundaries_round_trip_exactly() {
    let out = ok("A = 9223372036854775807&\nB = -9223372036854775808&\n\
         PRINT A\nPRINT B\nPRINT STR$(A)\nPRINT VAL(STR$(A)) = A\nPRINT VAL(STR$(B)) = B\n\
         PRINT VAL(\"9223372036854775807\") = A\nPRINT TYPEOF(VAL(\"9223372036854775807\"))");
    assert_eq!(
        out,
        "9223372036854775807\n-9223372036854775808\n9223372036854775807\n1\n1\n1\nINTEGER\n"
    );
}

#[test]
fn overflow_is_an_explicit_error() {
    let max = "9223372036854775807&";
    let min = "-9223372036854775808&";
    assert!(runtime_error(&format!("PRINT {max} + 1&")).contains("integer overflow in +"));
    assert!(runtime_error(&format!("PRINT {min} - 1&")).contains("integer overflow in -"));
    assert!(runtime_error(&format!("PRINT {max} * 2&")).contains("integer overflow in *"));
    assert!(runtime_error(&format!("A = {min}\nPRINT -A")).contains("overflow"));
    assert!(runtime_error(&format!("PRINT ABS({min})")).contains("ABS"));
    assert!(runtime_error(&format!("PRINT {min} \\ -1&")).contains("overflow"));
    assert_eq!(ok(&format!("PRINT {min} MOD -1&")), "0\n");
}

#[test]
fn integer_division_and_mod_follow_truncation() {
    assert_eq!(
        ok(
            "PRINT 7& \\ 2&; \" \"; -7& \\ 2&; \" \"; 7& \\ -2&; \" \"; -7& \\ -2&\n\
            PRINT 7& MOD 3&; \" \"; -7& MOD 3&; \" \"; 7& MOD -3&; \" \"; -7& MOD -3&\n\
            PRINT 7 \\ 2"
        ),
        "3 -3 -3 3\n1 -1 1 -1\n3\n"
    );
    assert!(runtime_error("PRINT 1& \\ 0&").contains("integer division by zero"));
    assert!(runtime_error("PRINT 1& MOD 0&").contains("MOD divisor must not be zero"));
    assert!(runtime_error("PRINT 7.5 \\ 2").contains("whole-number"));
}

#[test]
fn comparisons_are_exact_beyond_two_to_the_53() {
    let out = ok(
        "A = 9007199254740993&\nB = 9007199254740992&\nC = 9007199254740992\n\
         PRINT A > B\nPRINT A = B\nPRINT A > C\nPRINT B = C\nPRINT A <> C\nPRINT 1& = 1\n\
         PRINT 9223372036854775807& < 9223372036854775808",
    );
    assert_eq!(out, "1\n0\n1\n1\n1\n1\n1\n");
}

#[test]
fn mixing_and_promotion() {
    assert_eq!(
        ok(
            "PRINT TYPEOF(1& + 1)\nPRINT 1& + 0.5\nPRINT TYPEOF(1& + 1&)\nPRINT 7& / 2&\n\
            PRINT TYPEOF(2& ^ 3&)\nPRINT 2& ^ 3&\nPRINT TYPEOF(1 + 1)"
        ),
        "DOUBLE\n1.5\nINTEGER\n3.5\nDOUBLE\n8\nDOUBLE\n"
    );
}

#[test]
fn hex_and_literal_syntax() {
    assert_eq!(
        ok("PRINT &HFF\nPRINT &HFFFFFFFFFFFFFFFF\nPRINT &H7FFFFFFFFFFFFFFF\nPRINT TYPEOF(&H10)"),
        "255\n-1\n9223372036854775807\nINTEGER\n"
    );
    assert!(compile("PRINT 9223372036854775808&").is_err());
    assert!(compile("PRINT 1.5&").is_err());
    assert!(compile("PRINT &H").is_err());
    assert!(compile("PRINT &H12345678901234567").is_err());
}

#[test]
fn conversion_functions() {
    assert_eq!(
        ok(
            "PRINT CINT(2.5)\nPRINT CINT(-2.5)\nPRINT TYPEOF(CINT(1.2))\nPRINT INT64(4.0)\n\
            PRINT INT64(\"-12\")\nPRINT CDBL(3&) / 2\nPRINT TYPEOF(CDBL(3&))\n\
            PRINT CBOOL(5)\nPRINT CBOOL(\"false\")\nPRINT CSTR(5&) + CSTR(1.5)"
        ),
        "3\n-3\nINTEGER\n4\n-12\n1.5\nDOUBLE\n1\n0\n51.5\n"
    );
    assert!(runtime_error("PRINT INT64(1.5)").contains("INT64"));
    assert!(runtime_error("PRINT INT64(\"x\")").contains("INT64"));
    assert!(runtime_error("PRINT CINT(1E30)").contains("CINT"));
    assert!(runtime_error("PRINT CBOOL(\"maybe\")").contains("CBOOL"));
}

#[test]
fn typed_scalars_check_and_convert_assignments() {
    assert_eq!(
        ok(
            "DIM N AS INTEGER\nDIM D AS DOUBLE\nDIM B AS BOOLEAN\nDIM S$ AS STRING\n\
            PRINT N; \" \"; D; \" \"; B; \" [\"; S$; \"]\"\n\
            N = 4.0\nPRINT TYPEOF(N)\nD = 3&\nPRINT TYPEOF(D)\nB = 7\nPRINT B"
        ),
        "0 0 0 []\nINTEGER\nDOUBLE\n1\n"
    );
    assert!(runtime_error("DIM N AS INTEGER\nN = 1.5").contains("N"));
    assert!(runtime_error("DIM N AS INTEGER\nN = 1E30").contains("N"));
    assert!(runtime_error("DIM N AS INTEGER\nN = LIST()").contains("N"));
    assert!(runtime_error("DIM D AS DOUBLE\nD = \"x\"").contains("D"));
    assert!(runtime_error("DIM L AS LIST\nL = MAP()").contains("L"));
    assert!(runtime_error("DIM N AS INTEGER = 2.5").contains("N"));
    assert_eq!(ok("DIM L AS LIST\nAPPEND L, 1\nPRINT LEN(L)"), "1\n");
}

#[test]
fn variant_variables_hold_anything() {
    assert_eq!(
        ok(
            "DIM V AS VARIANT\nV = 1\nPRINT TYPEOF(V)\nV = \"s\"\nPRINT TYPEOF(V)\nV = LIST()\nPRINT TYPEOF(V)"
        ),
        "DOUBLE\nSTRING\nLIST\n"
    );
}

#[test]
fn legacy_string_name_rule_still_holds() {
    assert!(runtime_error("A = 1\nA = \"x\"").contains("type mismatch"));
    assert!(compile("DIM N$ AS INTEGER").is_err());
    assert!(compile("DIM N AS STRING").is_err());
    assert!(compile("DIM N AS BOGUS").is_err());
}

#[test]
fn integer_for_loops_are_exact_and_stop_at_the_range_end() {
    assert_eq!(
        ok(
            "FOR I = 9223372036854775805& TO 9223372036854775807&\nPRINT I\nNEXT\nPRINT \"done\"\n\
            FOR I = 3& TO 1& STEP -1&\nPRINT I\nNEXT"
        ),
        "9223372036854775805\n9223372036854775806\n9223372036854775807\ndone\n3\n2\n1\n"
    );
}

#[test]
fn input_into_integer_variable() {
    let mut vm = Vm::new(compile("DIM N AS INTEGER\nINPUT N\nPRINT N + 1&").unwrap()).unwrap();
    let mut out = Vec::new();
    vm.run(&mut Cursor::new("9223372036854775806\n"), &mut out)
        .unwrap();
    assert_eq!(String::from_utf8(out).unwrap(), "9223372036854775807\n");
}

#[test]
fn min_max_abs_sort_work_on_integers() {
    assert_eq!(
        ok(
            "PRINT MAX(9007199254740993&, 9007199254740992&)\nPRINT MIN(1&, 0.5)\nPRINT ABS(-5&)\n\
            L = LIST(3&, 1, 2&)\nSORT L\nPRINT L[1]; L[2]; L[3]\nPRINT CONTAINS(L, 2)"
        ),
        "9007199254740993\n0.5\n5\n123\n1\n"
    );
}

#[test]
fn option_explicit_requires_declarations() {
    let (line, message) = compile_error("OPTION EXPLICIT\nPRINT 1\nX = 2");
    assert_eq!(line, 3);
    assert!(message.contains("undeclared variable X"), "{message}");
    let (line, _) = compile_error("OPTION EXPLICIT\nDIM A AS INTEGER\nPRINT A + B");
    assert_eq!(line, 3);
    assert!(compile("PRINT 1\nOPTION EXPLICIT").is_err());
    assert!(compile("OPTION EXPLICIT\nDIM A\nDIM A").is_err());
    // Declared names, constants, loop and catch variables and parameters are fine.
    assert_eq!(
        ok(
            "OPTION EXPLICIT\nCONST K = 2\nDIM A AS INTEGER = 1&\nFOR I = 1 TO 2\nA = A + K\nNEXT\n\
            TRY\nTHROW \"x\"\nCATCH E$\nPRINT E$\nEND TRY\nPRINT A; \" \"; F(3)\nEND\n\
            FUNCTION F(PARAMETER P)\nLOCAL Q AS INTEGER = 1&\nF = P + Q\nFEND"
        ),
        "x\n5 4\n"
    );
    let (line, _) =
        compile_error("OPTION EXPLICIT\nPRINT F(1)\nEND\nFUNCTION F(PARAMETER P)\nF = P + Z\nFEND");
    assert_eq!(line, 5);
}

#[test]
fn legacy_names_dim_and_option_remain_usable() {
    assert_eq!(ok("DIM = 3\nREDIM = DIM + 1\nPRINT REDIM"), "4\n");
}

#[test]
fn typeof_iterates_a_mixed_list() {
    let out = ok(
        "DIM X AS VARIANT\nL = LIST(1, 2&, \"s\", ARRAY(1), LIST(), MAP())\n\
        FOR EACH X IN L\nPRINT TYPENAME$(X); \" \";\nNEXT\nPRINT\n\
        PRINT ISNUMERIC(1); ISNUMERIC(1&); ISNUMERIC(\"1\"); ISINTEGER(1&); ISINTEGER(1)\n\
        PRINT ISSTRING(\"a\"); ISARRAY(ARRAY()); ISLIST(LIST()); ISMAP(MAP()); ISLIST(MAP())",
    );
    assert_eq!(out, "DOUBLE INTEGER STRING ARRAY LIST MAP \n11010\n11110\n");
}

#[test]
fn typed_parameters_locals_and_results() {
    let program = "PRINT TYPEOF(F(4)); \" \"; F(4)\nPRINT G$(\"a\")\nEND\n\
        FUNCTION F(PARAMETER N AS INTEGER) AS INTEGER\nLOCAL T AS INTEGER = N\nF = T * 2\nFEND\n\
        FUNCTION G$(PARAMETER S$ AS STRING) AS STRING\nG$ = S$ + \"!\"\nFEND";
    assert_eq!(ok(program), "INTEGER 8\na!\n");
    // Whole floats convert; fractions and wrong kinds do not.
    assert!(
        runtime_error("PRINT F(1.5)\nFUNCTION F(PARAMETER N AS INTEGER)\nF = N\nFEND")
            .contains("N")
    );
    assert!(
        runtime_error("PRINT F(LIST())\nFUNCTION F(PARAMETER N AS INTEGER)\nF = N\nFEND")
            .contains("N")
    );
    assert!(
        runtime_error("PRINT F(1)\nFUNCTION F(PARAMETER N) AS INTEGER\nF = 1.5\nFEND")
            .contains("F")
    );
    assert!(
        runtime_error("PRINT F(1)\nFUNCTION F(PARAMETER N)\nLOCAL T AS INTEGER = 1.5\nFEND")
            .contains("T")
    );
    // Types are per call: an earlier call's types do not leak.
    assert_eq!(
        ok("PRINT F(1)\nPRINT F(2)\nFUNCTION F(PARAMETER N AS DOUBLE) AS DOUBLE\nF = N\nFEND"),
        "1\n2\n"
    );
    assert_eq!(
        ok("PRINT TYPEOF(F(2))\nFUNCTION F(PARAMETER N AS DOUBLE) AS DOUBLE\nF = N\nFEND"),
        "DOUBLE\n"
    );
}

#[test]
fn multi_dimensional_arrays() {
    assert_eq!(
        ok(
            "DIM A(2,3)\nA[1,1] = 5\nA[2,3] = 7\nPRINT A[1,1]; A[1,2]; A[2,3]; A[2][3]\n\
            PRINT UBOUND(A); UBOUND(A, 2); LBOUND(A, 2); LEN(A); LEN(A[1])\n\
            DIM V(3) AS STRING\nPRINT \"[\"; V[2]; \"]\"\nDIM W(2) AS INTEGER\nPRINT TYPEOF(W[1])"
        ),
        "5077\n23123\n[]\nINTEGER\n"
    );
}

#[test]
fn array_bounds_errors_are_explicit() {
    assert!(runtime_error("DIM A(2,3)\nPRINT A[3,1]").contains("out of bounds"));
    assert!(runtime_error("DIM A(2,3)\nPRINT A[1,4]").contains("out of bounds"));
    assert!(runtime_error("DIM A(2,3)\nA[0,1] = 1").contains("out of bounds"));
    assert!(runtime_error("DIM A(2)\nPRINT UBOUND(A, 2)").contains("dimension"));
    assert!(runtime_error("N = 0 - 1\nDIM A(N)").contains("non-negative"));
    assert!(runtime_error("N = 1.5\nDIM A(N)").contains("non-negative"));
    assert!(compile("DIM A()").is_err());
    assert!(compile("DIM A(1,2,3,4,5,6,7,8,9)").is_err());
    assert!(runtime_error("DIM A(100000000)").contains("too large"));
}

#[test]
fn multi_dimensional_allocation_is_bounded_and_fast() {
    let start = std::time::Instant::now();
    assert!(runtime_error("DIM B(2000000000,0,0)").contains("too large"));
    assert!(runtime_error("DIM B(0,2000000000)").contains("too large"));
    assert!(runtime_error("DIM B(10000000,0,0)").contains("too large"));
    assert!(runtime_error("DIM A(1000,1000,17)").contains("too large"));
    assert_eq!(
        ok("DIM A(1000000,0)\nPRINT LEN(A); \"/\"; UBOUND(A, 2)"),
        "1000000/0\n"
    );
    assert_eq!(
        ok("DIM A(200000,2)\nA[200000,2] = 7\nPRINT A[200000,2]"),
        "7\n"
    );
    assert_eq!(ok("DIM A(1000,1000,16)\nPRINT UBOUND(A, 3)"), "16\n");
    assert!(start.elapsed().as_secs() < 20, "allocation too slow");
}

#[test]
fn redim_resizes_in_place_and_aliases_see_it() {
    assert_eq!(
        ok(
            "DIM A(2,2) AS INTEGER\nA[1,1] = 9\nB = A\nREDIM A(3,2)\nPRINT UBOUND(B); UBOUND(B, 2); B[1,1]\n\
            A[3,2] = 4\nPRINT B[3,2]\nREDIM PRESERVE A(2,2)\nPRINT A[1,1]; UBOUND(A)"
        ),
        "320\n4\n02\n"
    );
    // PRESERVE keeps the existing elements.
    assert_eq!(
        ok(
            "DIM A(2,2) AS INTEGER\nA[1,1] = 9\nREDIM PRESERVE A(3,3)\nPRINT A[1,1]; UBOUND(A); UBOUND(A, 2)"
        ),
        "933\n"
    );
    assert_eq!(
        ok("DIM A(2)\nA[1] = 5\nREDIM PRESERVE A(4)\nPRINT A[1]; A[4]"),
        "50\n"
    );
    assert!(runtime_error("X = 1\nREDIM X(3)").contains("REDIM"));
}

#[test]
fn arrays_survive_garbage_collection() {
    let source = "DIM A(40,40)\nFOR I = 1 TO 40\nFOR J = 1 TO 40\nA[I,J] = I * 100 + J\nNEXT\nNEXT\n\
        S$ = \"\"\nFOR K = 1 TO 3000\nS$ = STR$(K) + \"x\"\nL = LIST(S$, S$)\nNEXT\n\
        ROW = A[7]\nREDIM PRESERVE A(40,40)\nFOR K = 1 TO 3000\nS$ = STR$(K) + \"y\"\nNEXT\n\
        PRINT A[40,40]; \" \"; ROW[3]; \" \"; A[7,3]";
    assert_eq!(ok(source), "4040 703 703\n");
}

#[test]
fn two_variable_for_each_over_maps() {
    assert_eq!(
        ok(
            "DIM K$ AS STRING\nM = MAP(\"b\", 2, \"a\", 1)\nFOR EACH K$, V IN M\nPRINT K$; V\nM[\"c\"] = 3\nNEXT\n\
            PRINT LEN(M)\nFOR EACH V IN M\nPRINT V;\nNEXT\nPRINT"
        ),
        "b2\na1\n3\n213\n"
    );
    assert_eq!(
        ok("M = MAP()\nFOR EACH K$, V IN M\nPRINT \"no\"\nNEXT\nPRINT \"empty\""),
        "empty\n"
    );
    assert!(runtime_error("FOR EACH K, V IN LIST(1)\nNEXT").contains("map"));
    assert!(compile("FOR EACH A, A IN MAP()\nNEXT").is_err());
}

#[test]
fn types_example_runs_with_bounded_output() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/types.bas");
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with("max: 9223372036854775807\n"));
    assert!(text.contains("caught: integer overflow in +\n"));
    assert!(text.contains("GRID[2,3] = 23 rows=2 cols=3\n"));
    assert!(text.ends_with("double: 42\n"));
    assert!(text.lines().count() < 30);
}
