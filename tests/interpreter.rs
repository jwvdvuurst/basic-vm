use std::io::{self, Cursor, Write};

use basic_vm::{INITIAL_HEAP_BYTES, Op, Vm, compile};

fn execute(source: &str, input: &str) -> (String, Vm) {
    let mut vm = Vm::new(compile(source).unwrap()).unwrap();
    let mut output = Vec::new();
    vm.run(&mut Cursor::new(input), &mut output).unwrap();
    (String::from_utf8(output).unwrap(), vm)
}

#[test]
fn mixed_labels_loop_in_source_order_and_halt() {
    let source = "N = 1\n200 PRINT N\nN = N + 1\nIF N <= 3 THEN 200\nEND\nPRINT 99";
    assert_eq!(execute(source, "").0, "1\n2\n3\n");
    assert_eq!(execute("20 PRINT 20\n10 PRINT 10", "").0, "20\n10\n");
    assert_eq!(
        execute("GOTO 10\nPRINT 0\n10 REM target\nPRINT 1", "").0,
        "1\n"
    );
}

#[test]
fn primes_example_identifies_primes_and_first_divisors_through_100() {
    let source = include_str!("../examples/primes.bas");
    let expected: String = (3..=100)
        .map(|n| match (2..n).find(|divisor| n % divisor == 0) {
            Some(divisor) => format!("{n} is not prime, divisible by {divisor}\n"),
            None => format!("{n} is prime\n"),
        })
        .collect();
    assert_eq!(execute(source, "").0, expected);
}

#[test]
fn arithmetic_comparisons_inline_condition_and_print_format() {
    let source = "let x = -(2 + 3) * 4 + 30 / 2\n\
                  IF X = -5 THEN PRINT \"yes\"; X, 1 < 2\n\
                  IF X > 0 THEN PRINT \"no\"\n\
                  PRINT 10 - 3 - 2; \"!\";\nPRINT";
    assert_eq!(execute(source, "").0, "yes-5\t1\n5!\n");
}

#[test]
fn strings_comments_and_input() {
    let source = "INPUT NAME$\nINPUT N\n\
                  PRINT \"Hello \" + NAME$; \": \"; N * 2 ' comment\n\
                  PRINT \"He said \"\"Hi\"\"\" REM comment";
    assert_eq!(
        execute(source, "Ada\r\n2.5\r\n").0,
        "Hello Ada: 5\nHe said \"Hi\"\n"
    );
}

#[test]
fn mod_function_and_operator_handle_numeric_remainders_and_precedence() {
    let source = "PRINT MOD(10, 3), 10 mod 3\n\
                  PRINT MOD(-10, 3), 10 MOD -3, -10 MOD -3\n\
                  PRINT MOD(5.5, 2), MOD(10, MOD(7, 4))\n\
                  PRINT 2 + 10 MOD 3 * 4, 20 MOD 6 MOD 3, 20 / 2 MOD 3\n\
                  A = ARRAY(10, 11)\nPRINT MOD(A[2], LEN(A))\n\
                  FOR I = 1 TO 4\nIF I MOD 2 = 0 THEN PRINT I\nNEXT";
    assert_eq!(
        execute(source, "").0,
        "1\t1\n-1\t1\t-1\n1.5\t1\n6\t2\t1\n1\n2\n4\n"
    );
}

#[test]
fn mod_rejects_zero_divisors_and_non_numeric_operands() {
    for (source, message) in [
        ("PRINT MOD(10, 0)", "MOD divisor must not be zero"),
        ("PRINT 10 MOD -0", "MOD divisor must not be zero"),
        ("PRINT MOD(\"text\", 3)", "expected a number"),
        ("PRINT 10 MOD \"text\"", "expected a number"),
        ("PRINT MOD(\"a\", \"b\")", "numeric operands"),
        ("PRINT MOD(LIST(1), 3)", "expected a number"),
    ] {
        let mut vm = Vm::new(compile(source).unwrap()).unwrap();
        let error = vm.run(&mut Cursor::new(""), &mut Vec::new()).unwrap_err();
        assert_eq!(error.line, 1);
        assert!(error.message.contains(message), "{source}: {error}");
    }
}

#[test]
fn mod_requires_exactly_two_function_arguments() {
    for source in ["PRINT MOD()", "PRINT MOD(10)", "PRINT MOD(10, 3, 2)"] {
        let error = compile(source).unwrap_err();
        assert_eq!(error.line, 1);
        assert_eq!(error.message, "MOD requires two arguments");
    }
}

#[test]
fn functions_support_forward_calls_optional_parameters_and_traditional_results() {
    let source = "GLOBAL = 10\nX = 99\nDOUBLE = 100\n\
                  PRINT double(21)\nCALL GREET\nCALL GREET()\nPRINT GLOBAL, X, DOUBLE\nEND\n\
                  FUNCTION DOUBLE(PARAMETER X)\nLOCAL TMP\nTMP = X * 2\n\
                  X = 0\nGLOBAL = GLOBAL + 1\nDOUBLE = TMP\nFEND\n\
                  FUNCTION GREET\nPRINT \"hello\"\nFEND";
    assert_eq!(execute(source, "").0, "42\nhello\nhello\n11\t99\t100\n");
    assert_eq!(
        execute("FUNCTION VALUE()\nVALUE = 7\nFEND\nPRINT VALUE()", "").0,
        "7\n"
    );
}

#[test]
fn function_parameters_are_values_or_shared_collection_references() {
    let source = "L = LIST(1)\nS$ = \"original\"\nR = CHANGE(L, S$)\n\
                  PRINT L[1], L[2], S$, R[1], R[2]\nPRINT GREETING$(S$)\n\
                  FUNCTION CHANGE(PARAMETER A, PARAMETER P$)\n\
                  APPEND A, P$\nA = LIST(999)\nP$ = \"changed\"\n\
                  CHANGE = ARRAY(A[1], P$)\nFEND\n\
                  FUNCTION GREETING$(PARAMETER NAME$)\n\
                  GREETING$ = \"Hello \" + NAME$\nFEND";
    assert_eq!(
        execute(source, "").0,
        "1\toriginal\toriginal\t999\tchanged\nHello original\n"
    );
}

#[test]
fn recursive_functions_have_independent_parameters_results_and_locals() {
    let source = "PRINT FACT(6), EVEN(6), ODD(5)\n\
                  FUNCTION FACT(PARAMETER N)\nLOCAL TEMP\nFACT = 1\n\
                  IF N <= 1 THEN 100\nTEMP = FACT(N - 1)\nFACT = N * TEMP\n100 FEND\n\
                  FUNCTION EVEN(PARAMETER N)\nEVEN = 1\nIF N = 0 THEN 100\n\
                  EVEN = ODD(N - 1)\n100 FEND\n\
                  FUNCTION ODD(PARAMETER N)\nODD = 0\nIF N = 0 THEN 100\n\
                  ODD = EVEN(N - 1)\n100 FEND";
    assert_eq!(execute(source, "").0, "720\t1\t1\n");
}

#[test]
fn recursive_calls_preserve_caller_loops_and_use_local_loop_variables() {
    let source = "I = 99\nFOR EACH N IN LIST(2, 3)\nPRINT SUM(N)\nNEXT\nPRINT I\n\
                  FUNCTION SUM(PARAMETER LIMIT)\nLOCAL I\nSUM = 0\n\
                  FOR I = 1 TO LIMIT\nSUM = SUM + I + SUM(0)\nNEXT I\nFEND";
    assert_eq!(execute(source, "").0, "3\n6\n99\n");
    let source = "FOR I = 1 TO 2\nPRINT COUNT(2)\nNEXT\n\
                  FUNCTION COUNT(PARAMETER N)\nCOUNT = 0\n\
                  WHILE N > 0\nCOUNT = COUNT + 1\nN = N - 1\nWEND\nFEND";
    assert_eq!(execute(source, "").0, "2\n2\n");
}

#[test]
fn gc_traces_suspended_function_locals_parameters_and_operand_values() {
    let source = "PRINT HOLD(3)\nPRINT \"prefix\" + TAIL$()\n\
                  G$ = \"argument\"\nPRINT ECHO$(G$)\nG$ = \"\"\nGC\n\
                  FUNCTION HOLD(PARAMETER N)\nLOCAL DATA\n\
                  DATA = MAP(\"name\", LIST(\"local\"))\nHOLD = 0\n\
                  IF N = 0 THEN 100\nHOLD = HOLD(N - 1) + LEN(DATA[\"name\"][1])\n\
                  100 GC\nFEND\n\
                  FUNCTION TAIL$\nGC\nTAIL$ = \"tail\"\nFEND\n\
                  FUNCTION ECHO$(PARAMETER P$)\nG$ = \"\"\nGC\nECHO$ = P$\nFEND";
    let (output, vm) = execute(source, "");
    assert_eq!(output, "15\nprefixtail\nargument\n");
    assert_eq!(vm.heap_stats().used_bytes, 1);
    assert_eq!(vm.heap_stats().live_allocations, 1);
}

#[test]
fn gc_traces_suspended_iteration_snapshots_and_reclaims_returned_cycles() {
    let source = "SOURCE = LIST(\"a\", \"b\")\nFOR EACH S$ IN SOURCE\n\
                  CALL COLLECT\nPRINT S$\nNEXT\nS$ = \"\"\nGC\n\
                  FUNCTION COLLECT\nSOURCE = 0\nGC\nFEND";
    let (output, vm) = execute(source, "");
    assert_eq!(output, "a\nb\n");
    assert_eq!(vm.heap_stats().used_bytes, 1);
    let source = "A = MAKE()\nGC\nPRINT LEN(A)\nA = 0\nGC\n\
                  FUNCTION MAKE\nLOCAL C\nC = LIST()\nAPPEND C, C\nMAKE = C\nFEND";
    let (output, vm) = execute(source, "");
    assert_eq!(output, "1\n");
    assert_eq!(vm.heap_stats().used_bytes, 0);
    assert_eq!(vm.heap_stats().live_allocations, 0);
}

#[test]
fn function_fend_discards_callee_loops_and_end_still_halts_the_program() {
    let source = "FOR I = 1 TO 2\nPRINT FIRST()\nNEXT\nGC\n\
                  FUNCTION FIRST\nLOCAL N\nFOR EACH N IN LIST(5, 6)\n\
                  FIRST = N\nGOTO 100\nNEXT\n100 FEND";
    let (output, vm) = execute(source, "");
    assert_eq!(output, "5\n5\n");
    assert_eq!(vm.heap_stats().used_bytes, 0);
    assert_eq!(
        execute(
            "CALL HALT_NOW\nPRINT \"bad\"\nFUNCTION HALT_NOW\nEND\nFEND",
            ""
        )
        .0,
        ""
    );
}

#[test]
fn function_locals_are_lexical_and_input_uses_local_scope() {
    let source = "X = 99\nX$ = \"global\"\nPRINT READ_VALUE(), X, X$\n\
                  FUNCTION READ_VALUE\nLOCAL X\nLOCAL X$\nINPUT X\nINPUT X$\n\
                  PRINT X$\nREAD_VALUE = X\nFEND";
    assert_eq!(execute(source, "21\nlocal\n").0, "local\n21\t99\tglobal\n");
    let source = "X = 99\nPRINT BAD()\nFUNCTION BAD\nBAD = X\nLOCAL X\nFEND";
    let mut vm = Vm::new(compile(source).unwrap()).unwrap();
    let error = vm.run(&mut Cursor::new(""), &mut Vec::new()).unwrap_err();
    assert_eq!(error.line, 4);
    assert!(error.message.contains("uninitialized local variable X"));
}

#[test]
fn function_runtime_errors_report_missing_results_and_type_mismatches() {
    for (source, line, message) in [
        (
            "PRINT F()\nFUNCTION F\nFEND",
            3,
            "did not assign its result",
        ),
        (
            "CALL F(1)\nFUNCTION F(PARAMETER X$)\nFEND",
            1,
            "type mismatch",
        ),
        (
            "CALL F(\"x\")\nFUNCTION F(PARAMETER X)\nFEND",
            1,
            "type mismatch",
        ),
        ("PRINT F$()\nFUNCTION F$\nF$ = 1\nFEND", 3, "type mismatch"),
        ("PRINT F()\nFUNCTION F\nF = \"x\"\nFEND", 3, "type mismatch"),
        (
            "CALL F\nFUNCTION F\nLOCAL X\nPRINT X\nFEND",
            4,
            "uninitialized local variable",
        ),
    ] {
        let mut vm = Vm::new(compile(source).unwrap()).unwrap();
        let error = vm.run(&mut Cursor::new(""), &mut Vec::new()).unwrap_err();
        assert_eq!(error.line, line, "{source}");
        assert!(error.message.contains(message), "{source}: {error}");
    }
}

#[test]
fn function_compiler_validates_declarations_signatures_and_jump_scope() {
    for (source, line, message) in [
        ("FUNCTION F", 1, "unterminated FUNCTION"),
        ("FEND", 1, "FEND without FUNCTION"),
        ("LOCAL X", 1, "only valid inside"),
        ("FUNCTION F\nFUNCTION G\nFEND\nFEND", 2, "global scope"),
        ("FOR I = 1 TO 2\nFUNCTION F\nFEND\nNEXT", 2, "outside loops"),
        ("FUNCTION F\nWHILE 1\nFEND", 3, "unfinished loop"),
        (
            "FUNCTION F\nFEND\nFUNCTION f\nFEND",
            3,
            "duplicate function",
        ),
        (
            "FUNCTION F(PARAMETER X, PARAMETER X)\nFEND",
            1,
            "duplicate parameter",
        ),
        ("FUNCTION F(PARAMETER F)\nFEND", 1, "result name"),
        ("FUNCTION F(PARAMER X)\nFEND", 1, "PARAMETER"),
        ("FUNCTION F\nLOCAL X\nLOCAL X\nFEND", 3, "duplicate local"),
        (
            "FUNCTION F(PARAMETER X)\nLOCAL X\nFEND",
            2,
            "duplicate local",
        ),
        ("FUNCTION F\nLOCAL F\nFEND", 2, "duplicate local"),
        ("IF 1 THEN LOCAL X", 1, "own lines"),
        ("PRINT MISSING()", 1, "unknown function"),
        ("CALL F(1)\nFUNCTION F\nFEND", 1, "requires 0 arguments"),
        (
            "PRINT F()\nFUNCTION F(PARAMETER X)\nFEND",
            1,
            "requires 1 arguments",
        ),
        ("GOTO 100\nFUNCTION F\n100 FEND", 1, "current scope"),
        ("100 END\nFUNCTION F\nGOTO 100\nFEND", 3, "current scope"),
    ] {
        let error = compile(source).unwrap_err();
        assert_eq!(error.line, line, "{source}");
        assert!(error.message.contains(message), "{source}: {error}");
    }
}

#[test]
fn compile_errors_report_physical_source_lines() {
    for (source, line, message) in [
        ("10 PRINT 1\n10 END", 2, "duplicate line number"),
        ("PRINT 1\nGOTO 99", 2, "unknown line number"),
        ("1.5 END", 1, "line number must be"),
        ("LET X = (1 + 2", 1, "expected RightParen"),
        ("PRINT \"unfinished", 1, "unterminated string"),
        ("PRINT 1.2.3", 1, "invalid number"),
        ("PRINT 1e999", 1, "number must be finite"),
        ("END extra", 1, "unexpected tokens"),
    ] {
        let error = compile(source).unwrap_err();
        assert_eq!(error.line, line, "{source}");
        assert!(error.message.contains(message), "{error}");
    }
}

#[test]
fn runtime_errors_are_explicit() {
    for (source, input, message) in [
        ("PRINT MISSING", "", "undefined variable"),
        ("PRINT 1 / 0", "", "division by zero"),
        ("PRINT 1e308 * 10", "", "not finite"),
        ("LET X = \"text\"", "", "type mismatch"),
        ("LET X$ = 1", "", "type mismatch"),
        ("PRINT \"x\" + 1", "", "expected a number"),
        ("IF \"x\" THEN END", "", "expected a number"),
        ("INPUT N", "", "end of input"),
        ("INPUT N", "NaN\n", "must be finite"),
        ("INPUT N", "oops\n", "must be a number"),
    ] {
        let mut vm = Vm::new(compile(source).unwrap()).unwrap();
        let error = vm
            .run(&mut Cursor::new(input), &mut Vec::new())
            .unwrap_err();
        assert_eq!(error.line, 1);
        assert!(error.message.contains(message), "{error}");
    }
}

#[test]
fn compiled_program_uses_bytecode_and_resolved_addresses() {
    let program = compile("10 X = 2 + 3\nGOTO 10").unwrap();
    assert!(
        program
            .instructions()
            .iter()
            .any(|i| matches!(i.op, Op::Binary(_)))
    );
    assert!(program.instructions().iter().any(|i| i.op == Op::Jump(0)));
}

#[test]
fn gc_preserves_operand_stack_and_variables() {
    let expression = vec!["\"a\""; 600].join(" + ");
    let source = format!("KEEP$ = \"keep\"\nPRINT \"prefix\" + ({expression})\nGC\nPRINT KEEP$");
    let (output, vm) = execute(&source, "");
    assert_eq!(output, format!("prefix{}\nkeep\n", "a".repeat(600)));
    assert!(vm.heap_stats().collections >= 3);
    assert_eq!(vm.heap_stats().used_bytes, 4);
    assert_eq!(vm.heap_stats().live_allocations, 1);
}

#[test]
fn program_heaps_are_private_and_resize_automatically() {
    let source = "S$ = \"x\"\nI = 0\n10 S$ = S$ + S$\nI = I + 1\nIF I < 21 THEN 10\nGC\n";
    let (_, grown) = execute(source, "");
    assert_eq!(grown.heap_stats().used_bytes, 2 * INITIAL_HEAP_BYTES);
    assert!(grown.heap_stats().capacity_bytes > INITIAL_HEAP_BYTES);
    let (_, shrunk) = execute(&format!("{source}S$ = \"small\"\nGC"), "");
    assert_eq!(shrunk.heap_stats().capacity_bytes, INITIAL_HEAP_BYTES);
    assert_eq!(shrunk.heap_stats().used_bytes, 5);
    assert_eq!(grown.heap_stats().used_bytes, 2 * INITIAL_HEAP_BYTES);
    let (_, automatic) = execute(
        &format!(
            "{source}S$ = \"small\"\nI = 0\n\
             20 TEMP$ = \"temporary\"\nI = I + 1\nIF I < 300 THEN 20"
        ),
        "",
    );
    assert_eq!(automatic.heap_stats().capacity_bytes, INITIAL_HEAP_BYTES);
    assert!(automatic.heap_stats().collections > grown.heap_stats().collections);
}

#[test]
fn output_failures_propagate() {
    struct BrokenOutput;
    impl Write for BrokenOutput {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("broken output"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("broken flush"))
        }
    }
    let mut vm = Vm::new(compile("PRINT 1").unwrap()).unwrap();
    let error = vm.run(&mut Cursor::new(""), &mut BrokenOutput).unwrap_err();
    assert!(error.message.contains("output failed"));
}

#[test]
fn arrays_lists_and_nested_index_assignment_are_one_based_and_shared() {
    let source = "A = ARRAY(10, 20)\nB = A\nB[1] = 99\nPRINT A[1], A[2], LEN(A)\n\
                  L = LIST(\"one\", \"two\")\nAPPEND L, \"three\"\nREMOVE L, 2\n\
                  L[1] = \"ONE\"\nPRINT L[1], L[2], LEN(L)\n\
                  NESTED = ARRAY(LIST(MAP(\"answer\", 42)))\n\
                  NESTED[1][1][\"answer\"] = 43\nGC\nPRINT NESTED[1][1][\"answer\"]";
    assert_eq!(execute(source, "").0, "99\t20\t2\nONE\tthree\t2\n43\n");
}

#[test]
fn maps_update_keys_and_iterate_values_in_insertion_order() {
    let source = "M = MAP(\"b\", 2, \"a\", 1, \"b\", 20)\n\
                  M[\"c\"] = 3\nM[\"a\"] = 10\n\
                  FOR EACH N IN M\nPRINT N\nNEXT N\n\
                  REMOVE M, \"b\"\nM[\"b\"] = 200\n\
                  FOR EACH N IN M\nPRINT N\nNEXT\nPRINT LEN(M)";
    assert_eq!(execute(source, "").0, "20\n10\n3\n10\n3\n200\n3\n");
}

#[test]
fn numeric_for_handles_steps_empty_ranges_and_evaluates_bounds_once() {
    let source = "LIMIT = 5\nFOR I = 1 TO LIMIT STEP 2\n\
                  LIMIT = 0\nPRINT I\nNEXT I\nPRINT I\n\
                  FOR I = 3 TO 1 STEP -1\nPRINT I\nNEXT\n\
                  FOR I = 2 TO 1\nPRINT \"bad\"\nNEXT\n\
                  FOR I = 1 TO 2 STEP -1\nPRINT \"bad\"\nNEXT\n\
                  FOR I = 0 TO 0.5 STEP 0.25\nPRINT I\nNEXT";
    assert_eq!(execute(source, "").0, "1\n3\n5\n7\n3\n2\n1\n0\n0.25\n0.5\n");
}

#[test]
fn while_reevaluates_conditions_and_structured_loops_nest() {
    let source = "I = 0\nWHILE I < 2\nI = I + 1\n\
                  FOR J = 1 TO 2\nFOR EACH N IN ARRAY(10, 20)\n\
                  PRINT I; J; \":\"; N\nNEXT N\nNEXT J\nWEND\n\
                  WHILE 0\nPRINT \"bad\"\nWEND";
    assert_eq!(
        execute(source, "").0,
        "11:10\n11:20\n12:10\n12:20\n21:10\n21:20\n22:10\n22:20\n"
    );
}

#[test]
fn foreach_uses_shallow_snapshots_and_allows_collection_mutation() {
    let source = "L = LIST(1, 2, 3)\nFOR EACH N IN L\nPRINT N\n\
                  REMOVE L, 1\nAPPEND L, 99\nNEXT\nPRINT LEN(L), L[1]\n\
                  A = ARRAY(LIST(1), LIST(2))\nFOR EACH ITEM IN A\n\
                  ITEM[1] = ITEM[1] + 10\nNEXT\nPRINT A[1][1], A[2][1]\n\
                  FOR EACH S$ IN LIST(\"one\", \"two\")\nPRINT S$\nNEXT\n\
                  M = MAP(\"first\", 1, \"second\", 2)\nFOR EACH N IN M\n\
                  M[\"second\"] = 99\nM[\"new\"] = 3\nPRINT N\nNEXT\n\
                  PRINT M[\"second\"], LEN(M)\n\
                  FOR EACH N IN ARRAY()\nPRINT \"bad\"\nNEXT\n\
                  FOR EACH N IN LIST()\nPRINT \"bad\"\nNEXT\n\
                  FOR EACH N IN MAP()\nPRINT \"bad\"\nNEXT";
    assert_eq!(
        execute(source, "").0,
        "1\n2\n3\n3\t99\n11\t12\none\ntwo\n1\n2\n99\t3\n"
    );
}

#[test]
fn iterator_snapshots_are_gc_roots_even_after_source_reassignment() {
    let source = "L = LIST(\"one\", \"two\")\nFOR EACH S$ IN L\n\
                  L = 0\nGC\nPRINT S$\nNEXT\nS$ = \"\"\nGC";
    let (output, vm) = execute(source, "");
    assert_eq!(output, "one\ntwo\n");
    assert_eq!(vm.heap_stats().used_bytes, 1);
    assert_eq!(vm.heap_stats().live_allocations, 1);
}

#[test]
fn jumps_out_of_loops_release_snapshots_and_header_jumps_restart_them() {
    let source = "FOR EACH N IN LIST(1, 2)\nGOTO 100\nNEXT\n100 GC\nPRINT N";
    let (output, vm) = execute(source, "");
    assert_eq!(output, "1\n");
    assert_eq!(vm.heap_stats().used_bytes, 0);
    let source = "COUNT = 0\n10 FOR EACH N IN LIST(1, 2)\nCOUNT = COUNT + 1\n\
                  IF COUNT < 2 THEN 10\nPRINT N\nNEXT\nGC";
    let (output, vm) = execute(source, "");
    assert_eq!(output, "1\n2\n");
    assert_eq!(vm.heap_stats().used_bytes, 0);
}

#[test]
fn gc_traces_nested_collections_and_reclaims_cycles() {
    let source = "A = LIST(\"survive\")\nB = MAP(\"a\", A)\nAPPEND A, B\n\
                  GC\nPRINT B[\"a\"][1]\nA = 0\nB = 0\nGC";
    let (output, vm) = execute(source, "");
    assert_eq!(output, "survive\n");
    assert_eq!(vm.heap_stats().used_bytes, 0);
    assert_eq!(vm.heap_stats().live_allocations, 0);
    let (_, vm) = execute("A = LIST()\nAPPEND A, A\nGC\nA = 0\nGC", "");
    assert_eq!(vm.heap_stats().live_allocations, 0);
}

#[test]
fn automatic_gc_preserves_constructor_arguments_and_mutated_children() {
    let source = "L = LIST()\nFOR I = 1 TO 300\nAPPEND L, LIST(\"keep\")\nNEXT\n\
                  PRINT LEN(L)\nFOR EACH ITEM IN L\nGC\nPRINT ITEM[1];\nNEXT\nPRINT";
    let (output, vm) = execute(source, "");
    assert_eq!(output, format!("300\n{}\n", "keep".repeat(300)));
    assert!(vm.heap_stats().collections > 300);
    let values = vec!["LIST(\"value\")"; 300].join(", ");
    let (output, _) = execute(&format!("A = ARRAY({values})\nGC\nPRINT A[300][1]"), "");
    assert_eq!(output, "value\n");
}

#[test]
fn invalid_collection_operations_and_loop_values_report_errors() {
    for (source, message) in [
        ("A = ARRAY(1)\nPRINT A[0]", "one-based"),
        ("A = ARRAY(1)\nPRINT A[2]", "out of bounds"),
        ("A = ARRAY(1)\nA[1.5] = 2", "one-based"),
        ("A = LIST()\nREMOVE A, 1", "out of bounds"),
        ("A = ARRAY(1)\nAPPEND A, 2", "APPEND requires a list"),
        ("A = ARRAY(1)\nREMOVE A, 1", "fixed-length array"),
        ("M = MAP(1, 2)", "keys must be strings"),
        ("M = MAP()\nPRINT M[\"missing\"]", "missing map key"),
        ("M = MAP()\nREMOVE M, \"missing\"", "missing map key"),
        ("M = MAP(\"Key\", 1)\nPRINT M[\"key\"]", "missing map key"),
        ("PRINT 1[1]", "expected a collection"),
        ("PRINT LIST(1)", "cannot PRINT a collection"),
        ("A$ = LIST()", "type mismatch"),
        ("PRINT LEN(1)", "LEN requires"),
        ("FOR EACH N IN 1\nNEXT", "expected a collection"),
        ("FOR EACH N IN LIST(\"text\")\nNEXT", "type mismatch"),
        ("FOR EACH N$ IN ARRAY(1)\nNEXT", "type mismatch"),
        ("FOR I = 1 TO 3 STEP 0\nNEXT", "must not be zero"),
        ("FOR I = 1e20 TO 1e20 STEP 1\nNEXT", "too small"),
        ("FOR I = 1e308 TO 1e308 STEP 1e308\nNEXT", "not finite"),
        ("WHILE \"text\"\nWEND", "expected a number"),
        (
            "GOTO 100\nFOR I = 1 TO 2\n100 NEXT",
            "without an active FOR",
        ),
    ] {
        let mut vm = Vm::new(compile(source).unwrap()).unwrap();
        let error = vm.run(&mut Cursor::new(""), &mut Vec::new()).unwrap_err();
        assert!(error.message.contains(message), "{source}: {error}");
    }
}

#[test]
fn compiler_rejects_mismatched_blocks_and_invalid_constructor_syntax() {
    for (source, line, message) in [
        ("FOR I = 1 TO 2", 1, "unterminated loop"),
        ("WHILE 1", 1, "unterminated loop"),
        ("NEXT", 1, "innermost FOR"),
        ("WEND", 1, "innermost WHILE"),
        ("FOR I = 1 TO 2\nWHILE 1\nNEXT", 3, "innermost FOR"),
        ("WHILE 1\nFOR I = 1 TO 2\nWEND", 3, "innermost WHILE"),
        ("FOR I = 1 TO 2\nNEXT J", 2, "does not match"),
        ("IF 1 THEN WHILE 1\nWEND", 1, "own lines"),
        ("FOR I$ = 1 TO 2\nNEXT", 1, "numeric variable"),
        ("A = MAP(\"key\")", 1, "key/value pairs"),
        ("PRINT LEN()", 1, "one argument"),
        ("PRINT LEN(1, 2)", 1, "one argument"),
        ("A = LIST(1,)", 1, "expected expression"),
        ("A = ARRAY(1)\nPRINT A[1", 2, "RightBracket"),
    ] {
        let error = compile(source).unwrap_err();
        assert_eq!(error.line, line, "{source}");
        assert!(error.message.contains(message), "{source}: {error}");
    }
}

fn run_error(source: &str) -> basic_vm::Error {
    let mut vm = Vm::new(compile(source).unwrap()).unwrap();
    vm.run(&mut Cursor::new(""), &mut Vec::new()).unwrap_err()
}

#[test]
fn block_if_supports_elseif_else_nesting_and_skipped_branches() {
    let source = "FOR I = 1 TO 5\nIF I = 1 THEN\nPRINT \"one\"\nELSEIF I = 2 THEN\nPRINT \"two\"\n\
                  ELSEIF I = 3 THEN\nIF TRUE THEN\nPRINT \"three-a\"\nELSE\nPRINT \"three-b\"\n\
                  END IF\nELSE\nPRINT \"other\"\nEND IF\nNEXT\n\
                  IF FALSE THEN\nCALL BOOM\nELSEIF FALSE THEN\nCALL BOOM\nEND IF\n\
                  IF 0 THEN\nCALL BOOM\nEND IF\nIF 1 THEN PRINT \"inline\"\n\
                  FUNCTION BOOM\nPRINT \"boom\"\nFEND";
    assert_eq!(
        execute(source, "").0,
        "one\ntwo\nthree-a\nother\nother\ninline\n"
    );
}

#[test]
fn logical_operators_have_documented_precedence_and_results() {
    let source = "PRINT 1 OR 0 AND 0, NOT 0 AND 0, NOT 1 = 2, 5 AND 2, NOT 5, TRUE + TRUE, \
                  1 + 1 = 2 AND 3 > 2, FALSE OR 7, NOT NOT 4";
    assert_eq!(execute(source, "").0, "1\t0\t1\t1\t0\t2\t1\t1\t1\n");
}

#[test]
fn and_or_evaluate_both_sides_while_andalso_orelse_short_circuit() {
    let source = "PRINT 0 AND SIDE(1)\nPRINT 1 OR SIDE(2)\nPRINT 0 ANDALSO SIDE(3)\n\
                  PRINT 1 ORELSE SIDE(4)\nPRINT 1 ANDALSO SIDE(5)\nPRINT 0 ORELSE SIDE(6)\n\
                  PRINT 0 ANDALSO 1 / 0, 2 ORELSE 1 / 0\n\
                  IF 0 ANDALSO SIDE(7) THEN PRINT \"no\"\n\
                  PRINT 1 ORELSE 0 ANDALSO SIDE(8)\n\
                  FUNCTION SIDE(PARAMETER V)\nPRINT \"side\"; V\nSIDE = V\nFEND";
    assert_eq!(
        execute(source, "").0,
        "side1\n0\nside2\n1\n0\n1\nside5\n1\nside6\n1\n0\t1\n1\n"
    );
    let error = run_error("PRINT 1 AND 1 / 0");
    assert!(error.message.contains("division by zero"), "{error}");
    let error = run_error("PRINT 1 ANDALSO 1 / 0");
    assert!(error.message.contains("division by zero"), "{error}");
}

#[test]
fn exit_and_continue_handle_nested_loops_of_every_kind() {
    let source = "FOR I = 1 TO 3\nFOR J = 1 TO 3\nIF J = 2 THEN CONTINUE FOR\n\
                  IF I = 2 THEN EXIT FOR\nPRINT I; \"-\"; J; \" \";\nNEXT\nPRINT \"|\";\nNEXT\nPRINT\n\
                  N = 0\nFOR I = 1 TO 5\nWHILE 1\nN = N + 1\nIF N = 2 THEN CONTINUE FOR\n\
                  IF N >= 4 THEN EXIT FOR\nEXIT WHILE\nWEND\nNEXT\nPRINT I; \",\"; N\n\
                  K = 0\nS = 0\nDO\nK = K + 1\nIF K > 6 THEN EXIT DO\n\
                  IF K MOD 2 = 1 THEN CONTINUE DO\nS = S + K\nLOOP\nPRINT S\n\
                  K = 0\nDO\nK = K + 1\nCONTINUE DO\nPRINT \"never\"\nLOOP WHILE K < 3\nPRINT K\n\
                  FOR EACH V IN LIST(1, 2, 3)\nIF V = 1 THEN CONTINUE FOR\nPRINT V;\n\
                  IF V = 2 THEN EXIT FOR\nNEXT\nPRINT\n\
                  W = 0\nWHILE W < 10\nW = W + 1\nIF W < 3 THEN CONTINUE WHILE\nEXIT WHILE\nWEND\nPRINT W";
    assert_eq!(
        execute(source, "").0,
        "1-1 1-3 ||3-1 3-3 |\n4,4\n12\n3\n2\n3\n"
    );
}

#[test]
fn exit_function_inside_loops_keeps_caller_loops_and_results() {
    let source = "FOR I = 1 TO 3\nPRINT FIND(8);\nNEXT\nPRINT\nPRINT FIND(3), FIND(99)\n\
                  PRINT SUM_TO(4)\nGC\n\
                  FUNCTION FIND(PARAMETER T)\nLOCAL X\nFIND = -1\nFOR EACH X IN LIST(5, 8, 3)\n\
                  IF X = T THEN\nFIND = X * 10\nEXIT FUNCTION\nEND IF\nNEXT\nFEND\n\
                  FUNCTION SUM_TO(PARAMETER N)\nSUM_TO = 0\nWHILE 1\nIF N = 0 THEN EXIT FUNCTION\n\
                  SUM_TO = N + SUM_TO(N - 1)\nEXIT FUNCTION\nWEND\nFEND";
    let (output, vm) = execute(source, "");
    assert_eq!(output, "808080\n30\t-1\n10\n");
    assert_eq!(vm.heap_stats().used_bytes, 0);
    assert_eq!(
        execute(
            "CALL NORES\nPRINT \"ok\"\nFUNCTION NORES\nEXIT FUNCTION\nFEND",
            ""
        )
        .0,
        "ok\n"
    );
    let error = run_error("PRINT NORES()\nFUNCTION NORES\nEXIT FUNCTION\nFEND");
    assert!(error.message.contains("did not assign"), "{error}");
}

#[test]
fn select_case_matches_numbers_and_strings_without_fallthrough() {
    let source = "FOR EACH S$ IN LIST(\"apple\", \"kiwi\", \"zzz\", \"m\", \"\")\nSELECT CASE S$\n\
                  CASE \"apple\", \"pear\"\nPRINT \"fruit \";\nCASE \"b\" TO \"n\"\nPRINT \"range \";\n\
                  CASE IS > \"x\"\nPRINT \"late \";\nCASE ELSE\nPRINT \"else \";\nEND SELECT\nNEXT\nPRINT\n\
                  FOR N = -2 TO 4\nSELECT CASE N\nCASE -1\nPRINT \"m1 \";\nCASE 1, 2\nPRINT \"small \";\n\
                  CASE 3 TO 3 + 1\nPRINT \"mid \";\nCASE IS <> 0\nPRINT \"nz \";\nEND SELECT\nNEXT\nPRINT\n\
                  C = 0\nSELECT CASE BUMP()\nCASE 1\nPRINT \"one \";\nCASE 1 TO 5\nPRINT \"range \";\n\
                  CASE ELSE\nPRINT \"else \";\nEND SELECT\nPRINT C\n\
                  SELECT CASE 9\nCASE 1\nPRINT \"x\"\nEND SELECT\nSELECT CASE 1\nEND SELECT\n\
                  PRINT PICK(2), PICK(7)\n\
                  FUNCTION BUMP\nC = C + 1\nBUMP = C\nFEND\n\
                  FUNCTION PICK(PARAMETER N)\nPICK = 0\nSELECT CASE N\nCASE 1, 2\nPICK = 12\n\
                  EXIT FUNCTION\nCASE ELSE\nPICK = 99\nEND SELECT\nFEND";
    assert_eq!(
        execute(source, "").0,
        "fruit range late range else \nnz m1 small small mid mid \none 1\n12\t99\n"
    );
    let error = run_error("SELECT CASE 1\nCASE \"a\"\nEND SELECT");
    assert!(error.message.contains("expected a number"), "{error}");
}

#[test]
fn select_inside_loops_supports_exit_and_nested_selects() {
    let source = "FOR I = 1 TO 10\nSELECT CASE I MOD 3\nCASE 0\nSELECT CASE I\nCASE 6\nEXIT FOR\n\
                  CASE ELSE\nPRINT \"three \";\nEND SELECT\nCASE 1\nCONTINUE FOR\nCASE ELSE\n\
                  PRINT I; \" \";\nEND SELECT\nNEXT\nPRINT I";
    assert_eq!(execute(source, "").0, "2 three 5 6\n");
}

#[test]
fn do_loops_support_leading_and_trailing_conditions() {
    let source = "I = 0\nDO WHILE I < 3\nI = I + 1\nLOOP\nPRINT I; \",\";\n\
                  DO UNTIL I >= 5\nI = I + 1\nLOOP\nPRINT I; \",\";\nDO\nI = I + 10\nLOOP WHILE I < 30\n\
                  PRINT I; \",\";\nDO UNTIL TRUE\nPRINT \"never\"\nLOOP\n\
                  DO\nPRINT \"once,\";\nLOOP UNTIL TRUE\nDO WHILE FALSE\nPRINT \"never\"\nLOOP\n\
                  DO\nI = I - 20\nLOOP UNTIL I < 0\nPRINT I";
    assert_eq!(execute(source, "").0, "3,5,35,once,-5\n");
}

#[test]
fn constants_are_evaluated_once_and_scoped_to_main_or_function() {
    let source = "CONST MAX = 3\nCONST NAME$ = \"n\" + \"ame\"\nCONST DOUBLE = MAX * 2\n\
                  PRINT MAX * 2, NAME$, DOUBLE\nPRINT TWICE(), USES_GLOBAL()\nPRINT MAX\n\
                  FUNCTION TWICE\nCONST MAX = 10\nTWICE = MAX * 2\nFEND\n\
                  FUNCTION USES_GLOBAL\nLOCAL X\nX = 4\nUSES_GLOBAL = MAX + X\nFEND";
    assert_eq!(execute(source, "").0, "6\tname\t6\n20\t7\n3\n");
    let error = run_error("CONST A$ = 1");
    assert!(error.message.contains("type mismatch"), "{error}");
    let error = run_error("FUNCTION F\nCONST K = 1\nFEND\nCALL F\nPRINT K");
    assert!(error.message.contains("undefined variable K"), "{error}");
}

#[test]
fn constants_reject_assignment_and_redeclaration() {
    for (source, line, message) in [
        ("CONST X = 1\nX = 2", 2, "cannot assign to constant"),
        ("CONST X = 1\nLET X = 2", 2, "cannot assign to constant"),
        ("CONST X = 1\nINPUT X", 2, "cannot assign to constant"),
        (
            "CONST X = 1\nFOR X = 1 TO 2\nNEXT",
            2,
            "cannot assign to constant",
        ),
        (
            "CONST X = 1\nFOR EACH X IN LIST()\nNEXT",
            2,
            "cannot assign",
        ),
        ("CONST X = 1\nCONST X = 2", 2, "already declared"),
        ("CONST X = 1\nFUNCTION F\nX = 2\nFEND", 3, "cannot assign"),
        ("FUNCTION F\nCONST K = 1\nK = 2\nFEND", 3, "cannot assign"),
        (
            "FUNCTION F\nCONST K = 1\nCONST K = 2\nFEND",
            3,
            "duplicate constant",
        ),
        (
            "FUNCTION F\nLOCAL K\nCONST K = 1\nFEND",
            3,
            "duplicate constant",
        ),
        (
            "FUNCTION F(PARAMETER K)\nCONST K = 1\nFEND",
            2,
            "duplicate constant",
        ),
        ("IF 1 THEN CONST X = 1", 1, "own lines"),
        (
            "FOR I = 1 TO 2\nCONST X = 1\nNEXT",
            2,
            "inside a loop or block",
        ),
        (
            "IF 1 THEN\nCONST X = 1\nEND IF",
            2,
            "inside a loop or block",
        ),
        ("CONST X", 1, "Equal"),
        ("CONST TRUE = 1", 1, "expected variable name"),
    ] {
        let error = compile(source).unwrap_err();
        assert_eq!(error.line, line, "{source}");
        assert!(error.message.contains(message), "{source}: {error}");
    }
    assert!(compile("CONST X = 1\nFUNCTION F\nLOCAL X\nX = 5\nF = X\nFEND").is_ok());
    assert!(compile("CONST L = LIST()\nAPPEND L, 1\nL[1] = 2").is_ok());
}

#[test]
fn compiler_rejects_invalid_structured_control_flow() {
    for (source, line, message) in [
        ("END IF", 1, "END IF without"),
        ("IF 1 THEN\nPRINT 1", 1, "unterminated block IF"),
        ("IF 1 THEN\nELSE\nELSE\nEND IF", 3, "ELSE after ELSE"),
        (
            "IF 1 THEN\nELSE\nELSEIF 1 THEN\nEND IF",
            3,
            "ELSEIF after ELSE",
        ),
        ("ELSE", 1, "ELSE without"),
        ("ELSEIF 1 THEN", 1, "ELSEIF without"),
        (
            "IF 1 THEN\nELSEIF 1 THEN PRINT 1\nEND IF",
            2,
            "must end the line",
        ),
        ("WHILE 1\nEND IF\nWEND", 2, "END IF without"),
        ("IF 1 THEN\nWEND", 2, "innermost WHILE"),
        ("IF 1 THEN\nNEXT", 2, "innermost FOR"),
        ("CASE 1", 1, "CASE outside SELECT CASE"),
        ("IF 1 THEN\nCASE 1\nEND IF", 2, "CASE outside"),
        (
            "SELECT CASE 1\nCASE ELSE\nCASE 2\nEND SELECT",
            3,
            "CASE after CASE ELSE",
        ),
        ("SELECT CASE 1\nPRINT 1\nEND SELECT", 2, "expected CASE"),
        ("SELECT CASE 1\nCASE 1", 1, "unterminated SELECT"),
        (
            "SELECT CASE 1\nCASE IS 3\nEND SELECT",
            2,
            "comparison operator",
        ),
        ("SELECT 1\nEND SELECT", 1, "Word(\"CASE\")"),
        ("END SELECT", 1, "END SELECT without"),
        ("IF 1 THEN\nEND SELECT", 2, "END SELECT without"),
        ("SELECT CASE 1\nEND IF", 2, "END IF without"),
        ("IF 1 THEN IF 1 THEN", 1, "own line"),
        ("IF 1 THEN END IF", 1, "own lines"),
        ("IF 1 THEN SELECT CASE 1", 1, "own lines"),
        ("IF 1 THEN DO", 1, "own lines"),
        ("DO", 1, "unterminated loop"),
        ("LOOP", 1, "LOOP must match"),
        ("WHILE 1\nLOOP\nWEND", 2, "LOOP must match"),
        ("DO\nNEXT", 2, "innermost FOR"),
        ("DO\nWEND", 2, "innermost WHILE"),
        ("DO WHILE 1\nLOOP UNTIL 1", 2, "both"),
        ("EXIT FOR", 1, "EXIT FOR outside"),
        ("CONTINUE WHILE", 1, "CONTINUE WHILE outside"),
        ("EXIT DO", 1, "EXIT DO outside"),
        ("FOR I = 1 TO 2\nEXIT WHILE\nNEXT", 2, "EXIT WHILE outside"),
        ("WHILE 1\nCONTINUE FOR\nWEND", 2, "CONTINUE FOR outside"),
        ("EXIT FUNCTION", 1, "only valid inside"),
        ("FUNCTION F\nEXIT FOR\nFEND", 2, "EXIT FOR outside"),
        ("EXIT", 1, "must be followed"),
        ("EXIT SELECT", 1, "must be followed"),
        ("CONTINUE FUNCTION", 1, "CONTINUE must be"),
        ("FUNCTION F\nIF 1 THEN\nFEND", 3, "unfinished loop or block"),
        ("TRUE = 1", 1, "expected variable name"),
        ("PRINT 1 ANDALSO", 1, "expected expression"),
        ("PRINT NOT", 1, "expected expression"),
    ] {
        let error = compile(source).unwrap_err();
        assert_eq!(error.line, line, "{source}");
        assert!(error.message.contains(message), "{source}: {error}");
    }
    assert!(compile("FOR I = 1 TO 2\nIF I = 1 THEN EXIT FOR\nNEXT").is_ok());
}

#[test]
fn early_loop_exits_release_snapshots_and_collect_safely() {
    let source = "FOR EACH N IN LIST(1, 2, 3)\nIF N = 2 THEN EXIT FOR\nNEXT\nGC\nPRINT N";
    let (output, vm) = execute(source, "");
    assert_eq!(output, "2\n");
    assert_eq!(vm.heap_stats().used_bytes, 0);
    let source = "DO\nFOR EACH S$ IN LIST(\"a\", \"b\")\nGC\nIF S$ = \"b\" THEN EXIT DO\nNEXT\nLOOP\n\
                  PRINT S$\nS$ = \"\"\nGC";
    let (output, vm) = execute(source, "");
    assert_eq!(output, "b\n");
    assert_eq!(vm.heap_stats().used_bytes, 1);
    assert_eq!(vm.heap_stats().live_allocations, 1);
    let source = "FOR I = 1 TO 3\nFOR EACH S$ IN LIST(\"x\", \"y\")\nCONTINUE FOR\nNEXT\nNEXT\n\
                  S$ = \"\"\nGC\nSELECT CASE \"ab\" + \"c\"\nCASE \"abc\"\nGC\nPRINT \"kept\"\nEND SELECT\nGC";
    let (output, vm) = execute(source, "");
    assert_eq!(output, "kept\n");
    assert_eq!(vm.heap_stats().used_bytes, 1);
}

#[test]
fn bytecode_for_new_constructs_is_dumpable() {
    let program = compile("DO\nEXIT DO\nLOOP\nPRINT 1 ANDALSO 0").unwrap();
    assert!(
        program
            .instructions()
            .iter()
            .any(|i| matches!(i.op, Op::AndJump(_)))
    );
}

// ---- Phase 2: standard library ----

fn out(source: &str) -> String {
    execute(source, "").0
}

#[test]
fn arithmetic_never_produces_negative_zero() {
    assert_eq!(
        out(
            "X = 0\nPRINT -0; \"|\"; -X; \"|\"; 0 * -1; \"|\"; X * -1; \"|\"; 0 / -5; \"|\"; STR$(-0); \"|\"; -0 MOD 3\n"
        ),
        "0|0|0|0|0|0|0\n"
    );
}

/// Compile-time or run-time failure message.
fn failure(source: &str) -> String {
    match compile(source) {
        Err(error) => error.to_string(),
        Ok(program) => {
            let mut vm = Vm::new(program).unwrap();
            vm.run(&mut Cursor::new(""), &mut Vec::new())
                .expect_err("program should fail")
                .to_string()
        }
    }
}

fn assert_fails(source: &str, expected: &str) {
    let message = failure(source);
    assert!(
        message.contains(expected),
        "{source:?}: expected {expected:?} in {message:?}"
    );
}

#[test]
fn math_functions_return_expected_values() {
    let source = "PRINT ABS(-3.5); \" \"; SQRT(16); \" \"; SQR(9); \" \"; MIN(3, 1, 2); \" \"; MAX(3, 1, 7, 2)\n\
                  PRINT ROUND(2.5); \" \"; ROUND(-2.5); \" \"; ROUND(2.567, 2); \" \"; ROUND(1234.5678, -2); \" \"; ROUND(7, 400 - 399)\n\
                  PRINT INT(-0.5); \" \"; INT(2.9); \" \"; FIX(-2.9); \" \"; FIX(-0.5)\n\
                  PRINT SIN(0); \" \"; COS(0); \" \"; LOG(1); \" \"; EXP(0); \" \"; ROUND(EXP(1), 4)\n";
    assert_eq!(
        out(source),
        "3.5 4 3 1 7\n3 -3 2.57 1200 7\n-1 2 -2 0\n0 1 0 1 2.7183\n"
    );
}

#[test]
fn power_operator_is_right_associative_and_binds_above_unary_minus() {
    let source = "PRINT 2 ^ 10; \" \"; 2 ^ 3 ^ 2; \" \"; -2 ^ 2; \" \"; 2 ^ -1; \" \"; 2 * 3 ^ 2\n\
                  PRINT (-2) ^ 2; \" \"; 0 ^ 0; \" \"; 4 ^ 0.5; \" \"; 2 ^ 2 MOD 3; \" \"; 1 + 2 ^ 2 * 2\n";
    assert_eq!(out(source), "1024 512 -4 0.5 18\n4 1 2 1 9\n");
}

#[test]
fn math_domain_and_range_errors_are_explicit() {
    assert_fails("PRINT SQRT(-1)", "SQRT: argument must not be negative");
    assert_fails("PRINT SQR(-0.5)", "SQRT");
    assert_fails("PRINT LOG(0)", "LOG: argument must be greater than zero");
    assert_fails("PRINT LOG(-2)", "LOG");
    assert_fails("PRINT EXP(1000)", "EXP: result is not a finite number");
    assert_fails("PRINT 10 ^ 400", "not a finite number");
    assert_fails("PRINT (-8) ^ 0.5", "not a finite number");
    assert_fails("PRINT 0 ^ -1", "not a finite number");
    assert_fails("PRINT ABS(\"x\")", "ABS");
    assert_fails("PRINT ROUND(1.5, 0.5)", "digits must be a whole number");
    assert_fails("PRINT ROUND(1.5, 309)", "digits");
    assert_fails("PRINT MAX(1, \"a\")", "MAX");
    assert_fails("PRINT 1 MOD 0", "MOD divisor");
    assert_fails("PRINT RND(1)", "built-in RND requires 0 arguments, got 1");
    assert_fails("PRINT MAX(1)", "at least 2");
    assert_fails("PRINT ROUND()", "1 to 2");
}

#[test]
fn trigonometric_builtins_pi_atn_atn2_and_tan() {
    let source = "\
        PRINT ABS(PI() - 3.141592653589793) < 1E-15\n\
        PRINT ABS(ATN(1) * 4 - PI()) < 1E-12\n\
        PRINT ABS(ATN2(1, 1) - PI() / 4) < 1E-12\n\
        PRINT ABS(ATN2(1, -1) - 3 * PI() / 4) < 1E-12\n\
        PRINT ABS(ATN2(-1, -1) + 3 * PI() / 4) < 1E-12\n\
        PRINT ABS(ATN2(-1, 1) + PI() / 4) < 1E-12\n\
        PRINT ABS(ATN2(0, -1) - PI()) < 1E-15\n\
        PRINT ABS(ATN2(1, 0) - PI() / 2) < 1E-15\n\
        PRINT ABS(ATN2(-1, 0) + PI() / 2) < 1E-15\n\
        PRINT ATN2(0, 1); \" \"; ATN2(0, 0); \" \"; ATN(0); \" \"; TAN(0)\n\
        PRINT ABS(TAN(PI() / 4) - 1) < 1E-12\n\
        PRINT ABS(TAN(-PI() / 4) + 1) < 1E-12\n\
        PRINT ROUND(ATN(1000000), 6); \" \"; ROUND(TAN(1), 6)\n\
        PRINT ATN2(-0, -0); \" \"; ATN(3 - 3); \" \"; ATN2(0 * -1, -1) > 3\n";
    assert_eq!(
        out(source),
        "1\n1\n1\n1\n1\n1\n1\n1\n1\n0 0 0 0\n1\n1\n1.570795 1.557408\n0 0 1\n"
    );
}

#[test]
fn trigonometric_builtin_names_remain_usable_as_variables() {
    let source = "PI = 3\nTAN = 4\nATN = 5\nATN2 = 6\n\
                  PRINT PI + TAN + ATN + ATN2; \" \"; PI(); \" \"; ATN(0)\n\
                  FUNCTION TAN(PARAMETER X)\n\
                  TAN = X * 100\n\
                  FEND\n\
                  PRINT TAN(2)\n";
    assert_eq!(out(source), "18 3.141592653589793 0\n200\n");
}

#[test]
fn trigonometric_builtins_check_arity_and_types() {
    assert_fails("PRINT PI(1)", "built-in PI requires 0 arguments, got 1");
    assert_fails("PRINT PI", "PI");
    assert_fails("PRINT ATN()", "built-in ATN requires 1 arguments, got 0");
    assert_fails(
        "PRINT ATN(1, 2)",
        "built-in ATN requires 1 arguments, got 2",
    );
    assert_fails("PRINT TAN()", "built-in TAN requires 1 arguments, got 0");
    assert_fails(
        "PRINT TAN(1, 2)",
        "built-in TAN requires 1 arguments, got 2",
    );
    assert_fails("PRINT ATN2(1)", "built-in ATN2 requires 2 arguments, got 1");
    assert_fails(
        "PRINT ATN2(1, 2, 3)",
        "built-in ATN2 requires 2 arguments, got 3",
    );
    assert_fails("PRINT ATN(\"x\")", "ATN");
    assert_fails("PRINT TAN(\"x\")", "TAN");
    assert_fails("PRINT ATN2(1, \"x\")", "ATN2");
}

#[test]
fn rnd_is_seedable_deterministic_and_in_range() {
    let source = "FUNCTION DRAW(PARAMETER SEED)\n\
                  RANDOMIZE SEED\n\
                  LOCAL L\n\
                  L = LIST()\n\
                  FOR I = 1 TO 20\n\
                  APPEND L, RND()\n\
                  NEXT\n\
                  DRAW = L\n\
                  FEND\n\
                  A = DRAW(5)\n\
                  B = DRAW(5)\n\
                  C = DRAW(6)\n\
                  SAME = 1\n\
                  OTHER = 0\n\
                  RANGE = 1\n\
                  FOR I = 1 TO 20\n\
                  IF A[I] <> B[I] THEN SAME = 0\n\
                  IF A[I] <> C[I] THEN OTHER = 1\n\
                  IF A[I] < 0 OR A[I] >= 1 THEN RANGE = 0\n\
                  NEXT\n\
                  RANDOMIZE\n\
                  X = RND()\n\
                  IF X < 0 OR X >= 1 THEN RANGE = 0\n\
                  PRINT SAME; OTHER; RANGE\n";
    assert_eq!(out(source), "111\n");
}

#[test]
fn string_functions_count_unicode_scalars_one_based() {
    let source = "S$ = \"a\u{e9}\u{1f600}e\u{301}z\"\n\
                  PRINT LEN(S$); LEFT$(S$, 3); \"|\"; RIGHT$(S$, 2); \"|\"; MID$(S$, 2, 2); \"|\"; MID$(S$, 3)\n\
                  PRINT INSTR(S$, \"\u{1f600}\"); INSTR(S$, \"z\"); INSTR(S$, \"a\", 2); INSTR(S$, \"\u{301}\")\n\
                  PRINT LEN(LEFT$(S$, 99)); LEN(RIGHT$(S$, 99)); LEN(MID$(S$, 7)); LEN(MID$(S$, 9, 2)); LEN(LEFT$(S$, 0))\n\
                  PRINT UCASE$(\"stra\u{df}e \u{e9}\"); \"|\"; LCASE$(\"\u{c0}B\u{130}\"); \"|\"; TRIM$(\"\u{a0} x y \t\"); \"|\"\n\
                  PRINT REPLACE$(\"a-b-c\", \"-\", \"\u{2192}\"); REPLACE$(\"aaa\", \"aa\", \"b\"); REPLACE$(\"abc\", \"x\", \"y\")\n";
    assert_eq!(
        out(source),
        "6a\u{e9}\u{1f600}|\u{301}z|\u{e9}\u{1f600}|\u{1f600}e\u{301}z\n\
         3605\n\
         66000\n\
         STRASSE \u{c9}|\u{e0}bi\u{307}|x y|\n\
         a\u{2192}b\u{2192}cbaabc\n"
    );
}

#[test]
fn str_and_val_follow_print_formatting_and_documented_parsing() {
    let source = "PRINT STR$(42); \"|\"; STR$(-1.5); \"|\"; STR$(0.1 + 0.2); \"|\"; STR$(1E21)\n\
                  PRINT VAL(\"  12.5  \") + 1; \" \"; VAL(\"-3\"); \" \"; VAL(\"1e3\"); \" \"; VAL(\".5\"); \" \"; VAL(\"+7\")\n";
    assert_eq!(
        out(source),
        "42|-1.5|0.30000000000000004|1000000000000000000000\n13.5 -3 1000 0.5 7\n"
    );
    for bad in [
        "abc", "", "  ", "1 2", "inf", "NaN", "1e999", "0x10", "12abc",
    ] {
        assert_fails(&format!("PRINT VAL(\"{bad}\")"), "VAL");
    }
}

#[test]
fn string_functions_report_type_and_bounds_errors() {
    assert_fails(
        "PRINT LEFT$(\"a\", -1)",
        "LEFT$: length must be a non-negative whole number",
    );
    assert_fails("PRINT RIGHT$(\"a\", 1.5)", "RIGHT$");
    assert_fails(
        "PRINT MID$(\"abc\", 0)",
        "start must be a whole number of at least 1",
    );
    assert_fails("PRINT MID$(\"abc\", 1, -1)", "MID$");
    assert_fails(
        "PRINT INSTR(\"abc\", \"a\", 0)",
        "INSTR: start must be a whole number of at least 1",
    );
    assert_fails(
        "PRINT INSTR(\"abc\", \"a\", 1.5)",
        "whole number of at least 1",
    );
    assert_fails("PRINT LEFT$(5, 1)", "LEFT$: expected a string");
    assert_fails("PRINT LEFT$(\"a\", \"b\")", "LEFT$");
    assert_fails("PRINT REPLACE$(\"a\", \"\", \"b\")", "must not be empty");
    assert_fails("PRINT UCASE$(1)", "UCASE$");
    assert_fails("PRINT STR$(\"a\")", "STR$");
    assert_fails("PRINT SPLIT(\"a\", \"\")", "separator must not be empty");
    assert_fails(
        "PRINT JOIN$(LIST(1), \",\")",
        "every element must be a string",
    );
    assert_fails("PRINT JOIN$(MAP(\"a\", \"b\"), \",\")", "found a map");
    assert_fails("PRINT JOIN$(5, \",\")", "expected a collection");
    assert_fails("PRINT ARG$(0)", "ARG$: index 0 out of range");
    assert_fails("PRINT ARG$(1)", "no command-line arguments");
    assert_fails("PRINT ENV$(\"\")", "invalid environment variable name");
    assert_fails("PRINT ENV$(\"A=B\")", "invalid environment variable name");
}

#[test]
fn split_and_join_handle_empty_pieces_unicode_and_arrays() {
    let source = "P = SPLIT(\"a,,b,\", \",\")\n\
                  PRINT LEN(P); \"[\"; P[1]; \"][\"; P[2]; \"][\"; P[3]; \"][\"; P[4]; \"]\"\n\
                  E = SPLIT(\"\", \",\")\n\
                  PRINT LEN(E); LEN(E[1])\n\
                  U = SPLIT(\"\u{1f600}\u{2192}\u{e9}\u{2192}z\", \"\u{2192}\")\n\
                  PRINT LEN(U); U[1]; U[2]; U[3]\n\
                  PRINT JOIN$(SPLIT(\"one two  three\", \" \"), \"_\"); \"|\"; JOIN$(ARRAY(\"x\", \"y\"), \"\"); \"|\"; JOIN$(LIST(), \",\"); \"|\"; JOIN$(LIST(\"solo\"), \",\")\n\
                  P[1] = \"changed\"\n\
                  PRINT JOIN$(P, \"/\")\n\
                  PRINT LEN(SPLIT(\"abcabc\", \"bc\"))\n";
    assert_eq!(
        out(source),
        "4[a][][b][]\n10\n3\u{1f600}\u{e9}z\none_two__three|xy||solo\nchanged//b/\n3\n"
    );
}

#[test]
fn collection_mutators_modify_in_place_and_are_visible_through_aliases() {
    let source = "L = LIST(3, 1, 2)\n\
                  B = L\n\
                  SORT L\n\
                  PRINT B[1]; B[2]; B[3]\n\
                  INSERT B, 4, 9\n\
                  INSERT L, 1, 0\n\
                  PRINT LEN(L); L[1]; L[5]\n\
                  X = POP(L)\n\
                  PRINT X; LEN(B)\n\
                  REVERSE B\n\
                  PRINT L[1]; L[2]; L[3]; L[4]\n\
                  A = ARRAY(\"b\", \"B\", \"a\", \"\u{e9}\", \"\u{1f600}\", \"Z\")\n\
                  A2 = A\n\
                  SORT A\n\
                  PRINT JOIN$(A2, \"\")\n\
                  REVERSE A2\n\
                  PRINT A[1]; LEN(A)\n\
                  N = LIST(2.5, -1, 10, -1)\n\
                  SORT N\n\
                  PRINT N[1]; N[2]; N[3]; N[4]\n\
                  E = LIST()\n\
                  SORT E\n\
                  REVERSE E\n\
                  INSERT E, 1, \"first\"\n\
                  INSERT E, 2, LIST(1)\n\
                  PRINT LEN(E); E[2][1]; POP(POP(LIST(LIST(7))))\n";
    assert_eq!(
        out(source),
        "123\n509\n94\n3210\nBZab\u{e9}\u{1f600}\n\u{1f600}6\n-1-12.510\n217\n"
    );
}

#[test]
fn sort_requires_uniform_types_and_collection_functions_report_errors() {
    assert_fails("L = LIST(1, \"a\")\nSORT L", "all numbers or all strings");
    assert_fails("L = LIST(LIST(1))\nSORT L", "all numbers or all strings");
    assert_fails("M = MAP(\"a\", 1)\nSORT M", "found a map");
    assert_fails("REVERSE MAP()", "found a map");
    assert_fails("SORT 5", "expected a collection");
    assert_fails("A = ARRAY(1)\nINSERT A, 1, 2", "INSERT: expected a list");
    assert_fails("L = LIST(1)\nINSERT L, 0, 2", "INSERT: index out of bounds");
    assert_fails("L = LIST(1)\nINSERT L, 3, 2", "INSERT: index out of bounds");
    assert_fails("L = LIST(1)\nINSERT L, 1.5, 2", "INSERT: index");
    assert_fails("L = LIST()\nPRINT POP(L)", "POP: the list is empty");
    assert_fails("A = ARRAY(1)\nPRINT POP(A)", "POP: expected a list");
    assert_fails("PRINT KEYS(LIST())", "KEYS: expected a map");
    assert_fails("PRINT VALUES(ARRAY())", "VALUES: expected a map");
    assert_fails("PRINT HASKEY(MAP(), 1)", "map keys must be strings");
    assert_fails("PRINT HASKEY(LIST(), \"a\")", "HASKEY: expected a map");
    assert_fails("PRINT CONTAINS(5, 1)", "CONTAINS: expected a collection");
    assert_fails("INSERT L, 1", "INSERT requires 3 arguments, got 2");
    assert_fails("SORT", "expected expression");
    assert_fails("SORT A, B", "SORT requires 1 arguments, got 2");
    assert_fails("RANDOMIZE 1, 2", "RANDOMIZE requires 0 to 1 arguments");
}

#[test]
fn contains_keys_values_and_haskey_follow_documented_semantics() {
    let source = "L = LIST(1, \"1\", \"x\")\n\
                  N = LIST(1)\n\
                  H = LIST(N)\n\
                  M = MAP(\"a\", \"x\", \"b\", 2, \"c\", N)\n\
                  PRINT CONTAINS(L, 1); CONTAINS(L, \"1\"); CONTAINS(L, 2); CONTAINS(L, \"y\")\n\
                  PRINT CONTAINS(H, N); CONTAINS(H, LIST(1)); CONTAINS(ARRAY(), 1)\n\
                  PRINT CONTAINS(M, \"x\"); CONTAINS(M, \"a\"); CONTAINS(M, 2); CONTAINS(M, N)\n\
                  PRINT HASKEY(M, \"a\"); HASKEY(M, \"A\"); HASKEY(M, \"zz\"); HASKEY(MAP(), \"a\")\n\
                  K = KEYS(M)\n\
                  V = VALUES(M)\n\
                  PRINT LEN(K); K[1]; K[2]; K[3]; LEN(V); V[1]; V[2]; V[3][1]\n\
                  K[1] = \"changed\"\n\
                  V[3][1] = 99\n\
                  PRINT HASKEY(M, \"a\"); N[1]; LEN(KEYS(MAP())); LEN(VALUES(MAP()))\n\
                  REMOVE M, \"a\"\n\
                  PRINT LEN(K); LEN(KEYS(M))\n";
    assert_eq!(out(source), "1100\n100\n1011\n1000\n3abc3x21\n19900\n32\n");
}

#[test]
fn collection_builtins_survive_gc_pressure() {
    let source = "GOOD = LIST()\n\
                  FOR I = 1 TO 600\n\
                  P = SPLIT(\"alpha,beta,gamma,delta,epsilon\", \",\")\n\
                  K = KEYS(MAP(\"k\" + STR$(I), I, \"z\", 0))\n\
                  V = VALUES(MAP(\"k\", LIST(I), \"q\", \"s\"))\n\
                  IF I MOD 100 = 0 THEN APPEND GOOD, JOIN$(P, \"+\") + \"/\" + K[1] + \"/\" + STR$(V[1][1])\n\
                  NEXT\n\
                  FOR EACH G$ IN GOOD\n\
                  PRINT G$\n\
                  NEXT\n\
                  BIG$ = \"x0\"\n\
                  FOR I = 1 TO 1500\n\
                  BIG$ = BIG$ + \",x\" + STR$(I)\n\
                  NEXT\n\
                  PIECES = SPLIT(BIG$, \",\")\n\
                  SORT PIECES\n\
                  REVERSE PIECES\n\
                  PRINT LEN(PIECES); \" \"; PIECES[1]; \" \"; PIECES[1501]; \" \"; CONTAINS(PIECES, \"x777\")\n\
                  WHILE LEN(PIECES) > 1490\n\
                  X$ = \"\"\n\
                  INSERT PIECES, 1, X$\n\
                  X$ = POP(PIECES)\n\
                  X$ = POP(PIECES)\n\
                  WEND\n\
                  PRINT LEN(PIECES)\n";
    let (output, vm) = execute(source, "");
    let mut expected: String = (1..=6)
        .map(|n| format!("alpha+beta+gamma+delta+epsilon/k{}/{}\n", n * 100, n * 100))
        .collect();
    expected.push_str("1501 x999 x0 1\n1490\n");
    assert_eq!(output, expected);
    assert!(vm.heap_stats().collections > 5);
}

#[test]
fn explicit_gc_after_builtins_keeps_results_reachable() {
    let source = "L = SPLIT(\"a b c\", \" \")\n\
                  K = KEYS(MAP(\"k1\", 1, \"k2\", 2))\n\
                  GC\n\
                  SORT K\n\
                  REVERSE L\n\
                  GC\n\
                  PRINT JOIN$(L, \"\"); JOIN$(K, \",\"); POP(L)\n";
    assert_eq!(out(source), "cbak1,k2a\n");
}

#[test]
fn user_functions_shadow_builtins_and_builtin_names_remain_usable_as_variables() {
    let source = "PRINT MAX(1, 2); \" \"; ABS(-4)\n\
                  FUNCTION MAX(PARAMETER A, PARAMETER B)\n\
                  MAX = 100\n\
                  FEND\n\
                  SORT = 3\n\
                  ABS = 4\n\
                  RND = 5\n\
                  LEFT$ = \"x\"\n\
                  INSERT = 2\n\
                  RANDOMIZE = 6\n\
                  REVERSE = LIST(1, 2)\n\
                  REVERSE[1] = 7\n\
                  PRINT SORT + ABS + RND + INSERT + RANDOMIZE; LEFT$; REVERSE[1]\n\
                  IF SORT = 3 THEN PRINT SQRT(SORT + 1)\n";
    assert_eq!(out(source), "100 4\n20x7\n2\n");
    assert_fails(
        "FUNCTION MAX(PARAMETER A, PARAMETER B)\nFEND\nPRINT MAX(1, 2, 3)",
        "function MAX requires 2 arguments, got 3",
    );
    assert_fails("PRINT FROB(1)", "unknown function FROB");
}

#[test]
fn builtins_can_be_discarded_with_call_and_used_inline() {
    let source = "L = LIST(5, 6)\n\
                  CALL POP(L)\n\
                  IF LEN(L) = 1 THEN SORT L\n\
                  IF LEN(L) = 1 THEN INSERT L, 1, 4\n\
                  RANDOMIZE 3\n\
                  PRINT L[1]; L[2]\n";
    assert_eq!(out(source), "45\n");
}

#[test]
fn date_time_and_timer_have_stable_shapes() {
    let source = "D$ = DATE$()\n\
                  T$ = TIME$()\n\
                  N$ = NOW$()\n\
                  PRINT LEN(D$); MID$(D$, 5, 1); MID$(D$, 8, 1); LEN(T$); MID$(T$, 3, 1); MID$(T$, 6, 1); LEN(N$); MID$(N$, 11, 1)\n\
                  PRINT VAL(LEFT$(D$, 4)) >= 2024; VAL(MID$(D$, 6, 2)) >= 1; VAL(MID$(D$, 6, 2)) <= 12; VAL(LEFT$(T$, 2)) <= 23\n\
                  PRINT UNIXTIME() > 1700000000; INT(UNIXTIME()) = UNIXTIME()\n\
                  A = TIMER()\n\
                  B = TIMER()\n\
                  PRINT A >= 0; B >= A\n";
    assert_eq!(out(source), "10--8::19 \n1111\n11\n11\n");
}

#[test]
fn arguments_default_to_empty_and_can_be_set_by_the_host() {
    let program = compile("PRINT ARGC(); \"|\"; ARG$(1); \"|\"; ARG$(2)").unwrap();
    let mut vm = Vm::new(program).unwrap();
    vm.set_arguments(vec!["h\u{e9}llo".into(), "".into()]);
    let mut output = Vec::new();
    vm.run(&mut Cursor::new(""), &mut output).unwrap();
    assert_eq!(String::from_utf8(output).unwrap(), "2|h\u{e9}llo|\n");
    assert_eq!(out("PRINT ARGC()"), "0\n");
}

#[test]
fn stdlib_example_runs_deterministically() {
    let source = include_str!("../examples/stdlib.bas");
    assert_eq!(out(source), STDLIB_EXAMPLE_OUTPUT);
}

const STDLIB_EXAMPLE_OUTPUT: &str = "Math: 3 4 9 2.57 -3 -2 1024\n\
    HELLO, WORLD|hello, world|Hello|World|World\n\
    8 HeLLo, WorLd\n\
    apple-fig-pear\n\
    pear fig apple kiwi / popped kiwi / left 3\n\
    x+y 1 0 1\n\
    same seed, same number\n\
    13.5 7!\n\
    Arguments: 0\n";
