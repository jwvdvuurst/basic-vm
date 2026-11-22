//! The Complex class in `examples/lib/complexutil.bas` and `examples/complex.bas`.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;

use basic_vm::{Error, Vm, compile_from};

/// Helpers shared by all fixtures. `Near` compares within 1e-9.
const PRELUDE: &str = r#"IMPORT "lib/complexutil.bas" AS Cx

FUNCTION C(PARAMETER a AS DOUBLE, PARAMETER b AS DOUBLE) AS Cx.Complex
  C = NEW Cx.Complex(a, b)
FEND

FUNCTION Near(PARAMETER z AS Cx.Complex, PARAMETER re AS DOUBLE, PARAMETER im AS DOUBLE) AS BOOLEAN
  Near = (ABS(z.Real() - re) < 1E-9 AND ABS(z.Imaginary() - im) < 1E-9)
FEND

FUNCTION Close(PARAMETER a AS DOUBLE, PARAMETER b AS DOUBLE) AS BOOLEAN
  Close = (ABS(a - b) < 1E-9)
FEND

"#;

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples")
}

fn run_program(body: &str) -> (String, Result<(), Error>) {
    let source = format!("{PRELUDE}{body}");
    // The program lives "in" the examples directory, so the import of
    // lib/complexutil.bas resolves to the shipped library.
    let program = match compile_from(&source, &examples_dir().join("complex_test.bas")) {
        Ok(program) => program,
        Err(error) => return (String::new(), Err(error)),
    };
    let mut vm = Vm::new(program).unwrap();
    let mut output = Vec::new();
    let result = vm.run(&mut Cursor::new(""), &mut output);
    (String::from_utf8(output).unwrap(), result)
}

fn run(body: &str) -> String {
    let (output, result) = run_program(body);
    result.unwrap_or_else(|error| panic!("{error}\noutput so far:\n{output}"));
    output
}

fn failure(body: &str) -> String {
    run_program(body)
        .1
        .expect_err("expected an error")
        .to_string()
}

fn lines(text: &str) -> Vec<&str> {
    text.lines().collect()
}

#[test]
fn add_subtract_and_negate_including_the_subtract_regression() {
    let output = run(r#"
PRINT C(3, 4).Add(C(1, 2)).ToString$()
PRINT C(3, 4).Subtract(C(1, 2)).ToString$()
PRINT C(1, 2).Subtract(C(3, 4)).ToString$()
PRINT C(3, 4).Subtract(C(3, 4)).ToString$()
PRINT C(3, 4).Negate().ToString$()
PRINT Near(C(3, 4).Subtract(C(1, 2)), 2, 2)
"#);
    assert_eq!(
        lines(&output),
        ["4 + 6i", "2 + 2i", "-2 - 2i", "0", "-3 - 4i", "1"]
    );
}

#[test]
fn multiply_square_and_divide() {
    let output = run(r#"
PRINT C(3, 4).Multiply(C(1, 2)).ToString$()
PRINT C(0, 1).Multiply(C(0, 1)).ToString$()
PRINT C(3, 4).Square().ToString$()
PRINT Near(C(3, 4).Divide(C(1, 2)), 2.2, -0.4)
PRINT C(4, 2).Divide(C(2, 0)).ToString$()
PRINT C(3, 4).Divide(C(3, 4)).ToString$()
PRINT Near(C(1, 1).Divide(C(0, 1)), 1, -1)
"#);
    assert_eq!(
        lines(&output),
        ["-5 + 10i", "-1", "-7 + 24i", "1", "2 + 1i", "1", "1"]
    );
}

#[test]
fn division_by_zero_throws_a_catchable_message() {
    let output = run(r#"
DIM z AS Cx.Complex = C(3, 4)
TRY
  PRINT z.Divide(Cx.ComplexZero()).ToString$()
CATCH e$
  PRINT "caught: "; e$
END TRY
TRY
  PRINT Cx.ComplexZero().Reciprocal().ToString$()
CATCH e$
  PRINT "caught: "; e$
END TRY
TRY
  PRINT C(0, 0).Power(-1).ToString$()
CATCH e$
  PRINT "caught: "; e$
END TRY
"#);
    assert_eq!(
        lines(&output),
        [
            "caught: complex division by zero",
            "caught: complex division by zero",
            "caught: complex division by zero"
        ]
    );
    let message = failure("PRINT C(1, 1).Divide(C(0, 0)).ToString$()\n");
    assert!(message.contains("complex division by zero"), "{message}");
}

#[test]
fn square_roots_are_principal_and_exact_for_perfect_squares() {
    let output = run(r#"
PRINT C(-4, 0).Sqrt().ToString$()
PRINT C(3, 4).Sqrt().ToString$()
PRINT C(3, -4).Sqrt().ToString$()
PRINT C(-3, -4).Sqrt().ToString$()
PRINT C(9, 0).Sqrt().ToString$()
PRINT C(0, 0).Sqrt().ToString$()
PRINT Near(C(0, 2).Sqrt(), 1, 1)
PRINT Near(C(-1, 0).Sqrt(), 0, 1)
PRINT Near(C(2, 5).Sqrt().Square(), 2, 5)
PRINT Near(C(-2, -5).Sqrt().Square(), -2, -5)
PRINT C(-2, -5).Sqrt().Real() > 0
"#);
    assert_eq!(
        lines(&output),
        [
            "0 + 2i", "2 + 1i", "2 - 1i", "1 - 2i", "3", "0", "1", "1", "1", "1", "1"
        ]
    );
}

#[test]
fn conjugate_magnitude_argument_and_aliases() {
    let output = run(r#"
PRINT C(3, 4).Conjugate().ToString$()
PRINT C(3, -4).Conjugate().ToString$()
PRINT C(3, 4).Magnitude()
PRINT C(3, 4).MagnitudeSquared(); " "; C(3, 4).Norm()
PRINT Close(C(1, 1).Argument(), PI() / 4)
PRINT Close(C(0, 1).Argument(), PI() / 2)
PRINT Close(C(-1, 0).Argument(), PI())
PRINT Close(C(0, -1).Argument(), -PI() / 2)
PRINT Close(C(-1, -1).Argument(), -3 * PI() / 4)
PRINT C(0, 0).Argument()
PRINT C(3, 4).Phase() = C(3, 4).Argument()
"#);
    assert_eq!(
        lines(&output),
        [
            "3 - 4i", "3 + 4i", "5", "25 25", "1", "1", "1", "1", "1", "0", "1"
        ]
    );
}

#[test]
fn polar_round_trip_and_polar_string() {
    let output = run(r#"
DIM z AS Cx.Complex = C(-3, 4)
DIM back AS Cx.Complex = Cx.ComplexFromPolar(z.Magnitude(), z.Argument())
PRINT Near(back, -3, 4)
PRINT Near(Cx.ComplexFromPolar(5, 0), 5, 0)
PRINT Near(Cx.ComplexFromPolar(2, PI() / 2), 0, 2)
PRINT Near(Cx.ComplexFromPolar(1, PI()), -1, 0)
PRINT Near(Cx.ComplexFromPolar(2, -PI() / 2), 0, -2)
PRINT C(2, 0).ToPolarString$()
PRINT C(-1, 0).ToPolarString$()
PRINT C(0, -2).ToPolarString$()
PRINT C(0, 0).ToPolarString$()
"#);
    assert_eq!(
        lines(&output),
        [
            "1",
            "1",
            "1",
            "1",
            "1",
            "2 * e^(i*0)",
            "1 * e^(i*3.141592653589793)",
            "2 * e^(-i*1.5707963267948966)",
            "0 * e^(i*0)"
        ]
    );
}

#[test]
fn whole_number_powers_including_zero_and_negative() {
    let output = run(r#"
PRINT C(2, 1).Power(3).ToString$()
PRINT C(2, 1).Power(0).ToString$()
PRINT C(2, 1).Power(1).ToString$()
PRINT C(2, 1).Power(2).ToString$()
PRINT C(1, 1).Power(10).ToString$()
PRINT C(0, 1).Power(4).ToString$()
PRINT C(0, 1).Power(2).ToString$()
PRINT C(0, 0).Power(0).ToString$()
PRINT C(0, 0).Power(5).ToString$()
PRINT Near(C(2, 1).Power(-2), 0.12, -0.16)
PRINT Near(C(2, 1).Power(-1), 0.4, -0.2)
PRINT C(1, 0).Power(1000).ToString$()
PRINT C(2, 0).Power(20).ToString$()
PRINT Near(C(2, 1).Power(-3).Multiply(C(2, 1).Power(3)), 1, 0)
"#);
    let lines = lines(&output);
    assert_eq!(
        &lines[..9],
        [
            "2 + 11i", "1", "2 + 1i", "3 + 4i", "0 + 32i", "1", "-1", "1", "0"
        ]
    );
    assert_eq!(&lines[9..12], ["1", "1", "1"]);
    assert_eq!(lines[12], "1048576");
    assert_eq!(lines[13], "1");
}

#[test]
fn power_rejects_non_whole_exponents() {
    let output = run(r#"
TRY
  PRINT C(1, 1).Power(2.5).ToString$()
CATCH e$
  PRINT e$
END TRY
"#);
    assert_eq!(
        lines(&output),
        ["complex Power: exponent must be a whole number"]
    );
}

#[test]
fn nth_roots_are_principal() {
    let output = run(r#"
PRINT Near(C(2, 11).NthRoot(3), 2, 1)
PRINT Near(C(-8, 0).NthRoot(3), 1, SQR(3))
PRINT Near(C(16, 0).NthRoot(4), 2, 0)
PRINT Near(C(-4, 0).NthRoot(2), 0, 2)
PRINT C(3, 4).NthRoot(2).ToString$()
PRINT C(3, 4).NthRoot(1).ToString$()
PRINT Near(C(3, 4).NthRoot(5).Power(5), 3, 4)
PRINT C(0, 0).NthRoot(3).ToString$()
FOR n = 1 TO 4
  TRY
    PRINT C(1, 1).NthRoot(0.5 * (n - 2)).ToString$()
  CATCH e$
    PRINT e$
  END TRY
NEXT
"#);
    let lines = lines(&output);
    assert_eq!(
        &lines[..8],
        ["1", "1", "1", "1", "2 + 1i", "3 + 4i", "1", "0"]
    );
    // n = 1 -> 0.5 * -1 and n = 2 -> 0 are rejected; n = 3 -> 0.5 is not whole;
    // n = 4 -> 1 is accepted.
    assert_eq!(
        &lines[8..11],
        [
            "complex NthRoot: n must be a whole number >= 1",
            "complex NthRoot: n must be a whole number >= 1",
            "complex NthRoot: n must be a whole number >= 1"
        ]
    );
    assert_eq!(lines[11], "1 + 1i");
}

#[test]
fn exp_and_log() {
    let output = run(r#"
PRINT Near(C(0, PI()).Exp(), -1, 0)
PRINT C(0, 0).Exp().ToString$()
PRINT Near(C(1, 0).Exp(), EXP(1), 0)
PRINT Near(C(EXP(1), 0).Log(), 1, 0)
PRINT Near(C(-1, 0).Log(), 0, PI())
PRINT Near(C(3, 4).Log().Exp(), 3, 4)
PRINT Near(C(3, 4).Log(), LOG(5), ATN2(4, 3))
PRINT C(1, 0).Log().ToString$()
TRY
  PRINT C(0, 0).Log().ToString$()
CATCH e$
  PRINT e$
END TRY
"#);
    assert_eq!(
        lines(&output),
        [
            "1",
            "1",
            "1",
            "1",
            "1",
            "1",
            "1",
            "0",
            "complex Log: logarithm of zero"
        ]
    );
}

#[test]
fn complex_powers_via_exp_and_log() {
    let output = run(r#"
PRINT Near(C(0, 1).Pow(C(0, 1)), EXP(-PI() / 2), 0)
PRINT Near(C(1, 1).Pow(C(2, 0)), 0, 2)
PRINT Near(C(2, 0).Pow(C(0.5, 0)), SQR(2), 0)
PRINT Near(C(3, 4).Pow(C(0.5, 0)), 2, 1)
PRINT C(0, 0).Pow(C(0, 0)).ToString$()
PRINT C(0, 0).Pow(C(2, 1)).ToString$()
TRY
  PRINT C(0, 0).Pow(C(-1, 0)).ToString$()
CATCH e$
  PRINT e$
END TRY
"#);
    assert_eq!(
        lines(&output),
        [
            "1",
            "1",
            "1",
            "1",
            "1",
            "0",
            "complex Pow: zero needs an exponent with a positive real part"
        ]
    );
}

#[test]
fn sine_and_cosine_follow_the_hyperbolic_expansions() {
    let output = run(r#"
DIM zs AS LIST = LIST(C(0.3, 0.7), C(1, -2), C(-2.5, 0.1), C(0, 0), C(3, 3))
FOR EACH z IN zs
  DIM s AS Cx.Complex = z.Sin()
  DIM c2 AS Cx.Complex = z.Cos()
  PRINT Near(s.Square().Add(c2.Square()), 1, 0)
NEXT
PRINT C(0, 0).Sin().ToString$()
PRINT C(0, 0).Cos().ToString$()
PRINT Near(C(PI() / 2, 0).Sin(), 1, 0)
PRINT Near(C(0, 1).Sin(), 0, (EXP(1) - EXP(-1)) / 2)
PRINT Near(C(0, 1).Cos(), (EXP(1) + EXP(-1)) / 2, 0)
PRINT Near(C(1, 2).Sin(), SIN(1) * (EXP(2) + EXP(-2)) / 2, COS(1) * (EXP(2) - EXP(-2)) / 2)
"#);
    assert_eq!(
        lines(&output),
        ["1", "1", "1", "1", "1", "0", "1", "1", "1", "1", "1"]
    );
}

#[test]
fn equals_uses_a_tolerance() {
    let output = run(r#"
PRINT C(1, 2).Equals(C(1, 2))
PRINT C(1, 2).Equals(C(1, 2.0000001))
PRINT C(1, 2).Equals(C(1, 2.0000001), 0.001)
PRINT C(1, 2).Equals(C(1.5, 2), 0.1)
PRINT C(1, 2).Equals(C(1, 2.1), 0.1)
PRINT C(0, 1).Exp().Equals(Cx.ComplexFromPolar(1, 1), 1E-12)
PRINT C(0, PI()).Exp().Equals(C(-1, 0), 1E-12)
TRY
  PRINT C(1, 2).Equals(C(1, 2), -1)
CATCH e$
  PRINT e$
END TRY
"#);
    assert_eq!(
        lines(&output),
        [
            "1",
            "0",
            "1",
            "0",
            "0",
            "1",
            "1",
            "complex Equals: tolerance must not be negative"
        ]
    );
}

#[test]
fn constructors_helpers_and_real_scalar_operations() {
    let output = run(r#"
PRINT Cx.ComplexZero().ToString$(); " | "; Cx.ComplexOne().ToString$(); " | "; Cx.ComplexI().ToString$()
PRINT Cx.ComplexFromReal(2.5).ToString$()
PRINT C(3, 4).AddReal(2).ToString$()
PRINT C(3, 4).SubtractReal(5).ToString$()
PRINT C(3, 4).MultiplyReal(2).ToString$()
PRINT C(3, 4).MultiplyReal(-0.5).ToString$()
PRINT Near(C(3, 4).Reciprocal(), 0.12, -0.16)
PRINT C(0, 0).IsZero(); C(0, 1).IsZero(); C(3, 0).IsReal(); C(3, 1).IsReal(); C(0, 5).IsImaginary(); C(0, 0).IsImaginary(); C(1, 5).IsImaginary()
PRINT C(3, 4).Real(); " "; C(3, 4).Imaginary()
PRINT Cx.ComplexI().Multiply(Cx.ComplexI()).ToString$()
PRINT C(3, 4).Copy().ToString$()
DIM n AS Cx.Complex = C(0, 0).Negate()
PRINT n.ToString$()
PRINT C(3, 0).Conjugate().ToString$()
"#);
    assert_eq!(
        lines(&output),
        [
            "0 | 1 | 0 + 1i",
            "2.5",
            "5 + 4i",
            "-2 + 4i",
            "6 + 8i",
            "-1.5 - 2i",
            "1",
            "1010100",
            "3 4",
            "-1",
            "3 + 4i",
            "0",
            "3"
        ]
    );
}

#[test]
fn to_string_forms_are_backward_compatible() {
    let output = run(r#"
PRINT C(3, 4).ToString$()
PRINT C(3, -4).ToString$()
PRINT C(-3, 4).ToString$()
PRINT C(3, 0).ToString$()
PRINT C(0, 0).ToString$()
PRINT C(0, 2).ToString$()
PRINT C(-1.5, -0.5).ToString$()
PRINT C(0, 0.25).ToString$()
"#);
    assert_eq!(
        lines(&output),
        [
            "3 + 4i",
            "3 - 4i",
            "-3 + 4i",
            "3",
            "0",
            "0 + 2i",
            "-1.5 - 0.5i",
            "0 + 0.25i"
        ]
    );
}

#[test]
fn parses_every_documented_form() {
    let cases = [
        ("3", "3"),
        ("-3", "-3"),
        ("2.5", "2.5"),
        ("2i", "0 + 2i"),
        ("-2.5i", "0 - 2.5i"),
        ("i", "0 + 1i"),
        ("-i", "0 - 1i"),
        ("+i", "0 + 1i"),
        ("3 + 4i", "3 + 4i"),
        ("3 - 4i", "3 - 4i"),
        ("3+4i", "3 + 4i"),
        ("3-4i", "3 - 4i"),
        ("-3 - 4i", "-3 - 4i"),
        ("-3+4i", "-3 + 4i"),
        ("  3   +   4i  ", "3 + 4i"),
        ("3 +4i", "3 + 4i"),
        ("3+ 4i", "3 + 4i"),
        ("3 + 4 i", "3 + 4i"),
        ("3 + i", "3 + 1i"),
        ("3 - i", "3 - 1i"),
        ("3+i", "3 + 1i"),
        ("3-i", "3 - 1i"),
        ("- i", "0 - 1i"),
        ("1.5e-3+2e2i", "0.0015 + 200i"),
        ("1e3", "1000"),
        ("2e-1i", "0 + 0.2i"),
    ];
    let mut body = String::new();
    for (text, _) in &cases {
        body.push_str(&format!(
            "PRINT Cx.ComplexFromString(\"{text}\").ToString$()\n"
        ));
    }
    let output = run(&body);
    let expected: Vec<&str> = cases.iter().map(|(_, expected)| *expected).collect();
    assert_eq!(lines(&output), expected);
}

#[test]
fn parsing_round_trips_to_string_and_rejects_bad_text() {
    let output = run(r#"
DIM zs AS LIST = LIST(C(3, 4), C(3, -4), C(-1.5, 0.25), C(0, 7), C(8, 0), C(0, 0))
FOR EACH z IN zs
  PRINT Cx.ComplexFromString(z.ToString$()).Equals(z)
NEXT
DIM bad AS LIST = LIST("", "   ", "abc", "1+2", "1 2", "3 + 4x", "i i", "1++2i", "--1", "1+2ii", "3 + * 4i", "1 + 2i + 3", "i2")
FOR EACH s$ IN bad
  TRY
    PRINT Cx.ComplexFromString(s$).ToString$()
  CATCH e$
    PRINT e$
  END TRY
NEXT
"#);
    let lines = lines(&output);
    assert_eq!(&lines[..6], ["1", "1", "1", "1", "1", "1"]);
    let bad = [
        "",
        "   ",
        "abc",
        "1+2",
        "1 2",
        "3 + 4x",
        "i i",
        "1++2i",
        "--1",
        "1+2ii",
        "3 + * 4i",
        "1 + 2i + 3",
        "i2",
    ];
    assert_eq!(lines.len(), 6 + bad.len(), "{lines:?}");
    for (line, text) in lines[6..].iter().zip(bad) {
        assert_eq!(*line, format!("invalid complex number: \"{text}\""));
    }
}

#[test]
fn methods_never_mutate_the_receiver() {
    let output = run(r#"
DIM a AS Cx.Complex = C(3, 4)
DIM o AS Cx.Complex = C(1, 2)
DIM r AS Cx.Complex = a.Add(o)
r = a.Subtract(o)
r = a.Multiply(o)
r = a.Divide(o)
r = a.Negate()
r = a.Conjugate()
r = a.Square()
r = a.Sqrt()
r = a.Reciprocal()
r = a.Power(3)
r = a.Power(-2)
r = a.Pow(o)
r = a.NthRoot(3)
r = a.Exp()
r = a.Log()
r = a.Sin()
r = a.Cos()
r = a.AddReal(1)
r = a.SubtractReal(1)
r = a.MultiplyReal(3)
r = a.Copy()
PRINT a.ToString$(); " | "; o.ToString$()
REM Results are independent objects, not aliases of the receiver.
DIM dup AS Cx.Complex = a.Copy()
dup.x = 100
PRINT a.Real(); " "; dup.Real()
DIM sum AS Cx.Complex = a.Add(Cx.ComplexZero())
sum.x = -1
PRINT a.Real(); " "; sum.Real()
DIM p AS Cx.Complex = a.Power(1)
p.y = 0
PRINT a.Imaginary(); " "; p.Imaginary()
"#);
    assert_eq!(lines(&output), ["3 + 4i | 1 + 2i", "3 100", "3 -1", "4 0"]);
}

#[test]
fn instances_are_isolated() {
    let output = run(r#"
DIM a AS Cx.Complex = C(1, 1)
DIM b AS Cx.Complex = C(1, 1)
b.x = 5
PRINT a.ToString$(); " | "; b.ToString$()
DIM z1 AS Cx.Complex = Cx.ComplexZero()
DIM z2 AS Cx.Complex = Cx.ComplexZero()
z1.x = 9
PRINT z1.ToString$(); " | "; z2.ToString$(); " | "; Cx.ComplexZero().ToString$()
DIM i1 AS Cx.Complex = Cx.ComplexI()
i1.y = 3
PRINT Cx.ComplexI().ToString$(); " | "; Cx.ComplexOne().ToString$()
PRINT TYPENAME$(a)
"#);
    assert_eq!(
        lines(&output),
        ["1 + 1i | 5 + 1i", "9 | 0 | 0", "0 + 1i | 1", "CX.COMPLEX"]
    );
}

#[test]
fn a_constructor_with_required_parameters_forbids_bare_dim() {
    let (_, result) = run_program("DIM z AS Cx.Complex\n");
    let message = result.expect_err("bare DIM must not compile").to_string();
    assert!(message.contains("constructor"), "{message}");
}

#[test]
fn library_builds_values_through_new_and_module_helpers() {
    let output = run(r#"
DIM z AS Cx.Complex = NEW Cx.Complex(1, -1)
PRINT z.ToString$()
PRINT Cx.ComplexFromPolar(1, 0).ToString$(); " "; Cx.ComplexFromReal(2).ToString$()
"#);
    assert_eq!(lines(&output), ["1 - 1i", "1 2"]);
}

#[test]
fn complex_example_prints_the_expected_output() {
    let path = examples_dir().join("complex.bas");
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "a = 3 + 4i, b = 1 + 2i\n\
         a + b = 4 + 6i\n\
         a - b = 2 + 2i\n\
         a * b = -5 + 10i\n\
         a / b = 2.2 - 0.4i\n\
         conjugate of a = 3 - 4i\n\
         |a| = 5, arg(a) = 0.927295\n\
         sqrt(-4) = 0 + 2i\n\
         sqrt(a) = 2 + 1i\n\
         (2 + i)^3 = 2 + 11i\n\
         (2 + i)^-2 = 0.12 - 0.16i\n\
         exp(i*pi) = -1\n\
         polar: 5 * e^(i*0.9272952180016122)\n\
         from polar: 0 + 2i\n\
         parsed: 0 - 1i, 2.5 - 1.5i\n\
         caught: complex division by zero\n\
         caught: invalid complex number: \"1 + 2x\"\n"
    );
}
