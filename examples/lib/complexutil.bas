REM Complex numbers: a CLASS Complex plus module-level helpers.
REM
REM Usage:
REM   IMPORT "lib/complexutil.bas" AS Cx
REM   DIM z AS Cx.Complex = NEW Cx.Complex(3, 4)      ' 3 + 4i
REM   PRINT z.Multiply(Cx.ComplexI()).ToString$()      ' -4 + 3i
REM
REM Notes:
REM - The constructor needs both parts, so always construct with
REM   NEW Complex(x, y). A bare "DIM c AS Complex" is a compile error.
REM - Objects are immutable by convention: every method that returns a Complex
REM   returns a NEW instance and never changes its receiver.
REM - There are no static methods, so the "constructors" are module-level
REM   functions: ComplexFromString, ComplexFromPolar, ComplexFromReal,
REM   ComplexZero, ComplexOne and ComplexI.
REM - Errors are raised with THROW (division by zero, log of zero, bad text,
REM   non-whole exponents) and can be caught with TRY ... CATCH.
REM - ToString$ gives "a", "a + bi" or "a - bi"; ComplexFromString reads those
REM   forms back (and "bi", "i", "-i", "a+bi", with spaces around the signs).
REM - ToPolarString$ gives "r * e^(i*theta)", or "r * e^(-i*theta)" for a
REM   negative angle (ASCII only), theta in radians.
OPTION EXPLICIT

CLASS Complex
  x AS DOUBLE
  y AS DOUBLE

  FUNCTION NEW(PARAMETER x AS DOUBLE, PARAMETER y AS DOUBLE)
    REM "+ 0" turns a negative zero into zero so that it never prints as -0.
    SELF.x = x + 0
    SELF.y = y + 0
  FEND

  FUNCTION Copy() AS Complex
    Copy = NEW Complex(SELF.x, SELF.y)
  FEND

  FUNCTION IsZero() AS BOOLEAN
    IsZero = (SELF.x = 0 AND SELF.y = 0)
  FEND

  FUNCTION IsReal() AS BOOLEAN
    IsReal = (SELF.y = 0)
  FEND

  FUNCTION IsImaginary() AS BOOLEAN
    IsImaginary = (SELF.x = 0 AND SELF.y <> 0)
  FEND

  FUNCTION Real() AS DOUBLE
    Real = SELF.x
  FEND

  FUNCTION Imaginary() AS DOUBLE
    Imaginary = SELF.y
  FEND

  FUNCTION ToString$() AS STRING
    IF SELF.y > 0 THEN
      ToString$ = STR$(SELF.x) + " + " + STR$(SELF.y) + "i"
    ELSEIF SELF.y < 0 THEN
      ToString$ = STR$(SELF.x) + " - " + STR$(-SELF.y) + "i"
    ELSE
      ToString$ = STR$(SELF.x)
    END IF
  FEND

  FUNCTION Negate() AS Complex
    Negate = NEW Complex(-SELF.x, -SELF.y)
  FEND

  FUNCTION Add(PARAMETER other AS Complex) AS Complex
    Add = NEW Complex(SELF.x + other.x, SELF.y + other.y)
  FEND

  FUNCTION Subtract(PARAMETER other AS Complex) AS Complex
    Subtract = NEW Complex(SELF.x - other.x, SELF.y - other.y)
  FEND

  FUNCTION Multiply(PARAMETER other AS Complex) AS Complex
    Multiply = NEW Complex(SELF.x * other.x - SELF.y * other.y, SELF.x * other.y + SELF.y * other.x)
  FEND

  FUNCTION Divide(PARAMETER other AS Complex) AS Complex
    DIM denom AS DOUBLE = other.x * other.x + other.y * other.y
    IF denom = 0 THEN
      THROW "complex division by zero"
    END IF
    Divide = NEW Complex((SELF.x * other.x + SELF.y * other.y) / denom, (SELF.y * other.x - SELF.x * other.y) / denom)
  FEND

  FUNCTION Square() AS Complex
    Square = NEW Complex(SELF.x * SELF.x - SELF.y * SELF.y, 2 * SELF.x * SELF.y)
  FEND

  REM Principal square root (real part >= 0; the root of a negative real is
  REM positive imaginary). Uses the algebraic form, which is exact for perfect
  REM squares such as sqrt(-4) = 2i and sqrt(3 + 4i) = 2 + i.
  FUNCTION Sqrt() AS Complex
    DIM m AS DOUBLE = SQR(SELF.x * SELF.x + SELF.y * SELF.y)
    DIM re AS DOUBLE = 0
    DIM im AS DOUBLE = 0
    IF m = 0 THEN
      Sqrt = NEW Complex(0, 0)
      EXIT FUNCTION
    END IF
    IF SELF.x >= 0 THEN
      re = SQR((m + SELF.x) / 2)
      im = SELF.y / (2 * re)
    ELSE
      im = SQR((m - SELF.x) / 2)
      IF SELF.y < 0 THEN
        im = -im
      END IF
      re = SELF.y / (2 * im)
    END IF
    Sqrt = NEW Complex(re, im)
  FEND

  REM Angle in radians, in (-pi, pi]; the argument of zero is 0.
  FUNCTION Argument() AS DOUBLE
    Argument = ATN2(SELF.y, SELF.x)
  FEND

  FUNCTION Conjugate() AS Complex
    Conjugate = NEW Complex(SELF.x, -SELF.y)
  FEND

  FUNCTION Magnitude() AS DOUBLE
    Magnitude = SQR(SELF.x * SELF.x + SELF.y * SELF.y)
  FEND

  FUNCTION Phase() AS DOUBLE
    Phase = SELF.Argument()
  FEND

  REM |z|^2 = x^2 + y^2, without the square root.
  FUNCTION MagnitudeSquared() AS DOUBLE
    MagnitudeSquared = SELF.x * SELF.x + SELF.y * SELF.y
  FEND

  REM Norm is the field norm: the same as MagnitudeSquared.
  FUNCTION Norm() AS DOUBLE
    Norm = SELF.MagnitudeSquared()
  FEND

  FUNCTION AddReal(PARAMETER a AS DOUBLE) AS Complex
    AddReal = NEW Complex(SELF.x + a, SELF.y)
  FEND

  FUNCTION SubtractReal(PARAMETER a AS DOUBLE) AS Complex
    SubtractReal = NEW Complex(SELF.x - a, SELF.y)
  FEND

  REM Scales both parts by the real factor k.
  FUNCTION MultiplyReal(PARAMETER k AS DOUBLE) AS Complex
    MultiplyReal = NEW Complex(SELF.x * k, SELF.y * k)
  FEND

  FUNCTION Reciprocal() AS Complex
    DIM denom AS DOUBLE = SELF.x * SELF.x + SELF.y * SELF.y
    IF denom = 0 THEN
      THROW "complex division by zero"
    END IF
    Reciprocal = NEW Complex(SELF.x / denom, -SELF.y / denom)
  FEND

  REM Equal when both parts differ by at most tolerance (default: exactly equal).
  FUNCTION Equals(PARAMETER other AS Complex, PARAMETER tolerance AS DOUBLE = 0.0) AS BOOLEAN
    IF tolerance < 0 THEN
      THROW "complex Equals: tolerance must not be negative"
    END IF
    Equals = (ABS(SELF.x - other.x) <= tolerance AND ABS(SELF.y - other.y) <= tolerance)
  FEND

  REM z^n for a whole number n (negative and 0 included), by repeated squaring.
  FUNCTION Power(PARAMETER n AS DOUBLE) AS Complex
    IF n <> INT(n) THEN
      THROW "complex Power: exponent must be a whole number"
    END IF
    DIM k AS DOUBLE = ABS(n)
    DIM acc AS Complex = ComplexOne()
    DIM factor AS Complex = SELF.Copy()
    WHILE k >= 1
      IF k MOD 2 = 1 THEN
        acc = acc.Multiply(factor)
      END IF
      k = INT(k / 2)
      IF k >= 1 THEN
        factor = factor.Square()
      END IF
    WEND
    IF n < 0 THEN
      acc = acc.Reciprocal()
    END IF
    Power = acc
  FEND

  REM z^w for a complex w (principal value: exp(w * log(z))). 0^0 is 1 and
  REM 0^w is 0 when the real part of w is positive; other powers of 0 are errors.
  FUNCTION Pow(PARAMETER other AS Complex) AS Complex
    IF SELF.IsZero() THEN
      IF other.IsZero() THEN
        Pow = ComplexOne()
      ELSEIF other.x > 0 THEN
        Pow = ComplexZero()
      ELSE
        THROW "complex Pow: zero needs an exponent with a positive real part"
      END IF
      EXIT FUNCTION
    END IF
    Pow = SELF.Log().Multiply(other).Exp()
  FEND

  REM Principal n-th root for a whole number n >= 1.
  FUNCTION NthRoot(PARAMETER n AS DOUBLE) AS Complex
    IF n < 1 OR n <> INT(n) THEN
      THROW "complex NthRoot: n must be a whole number >= 1"
    END IF
    IF n = 1 THEN
      NthRoot = SELF.Copy()
    ELSEIF n = 2 THEN
      NthRoot = SELF.Sqrt()
    ELSE
      DIM r AS DOUBLE = SELF.Magnitude() ^ (1 / n)
      DIM theta AS DOUBLE = SELF.Argument() / n
      NthRoot = NEW Complex(r * COS(theta), r * SIN(theta))
    END IF
  FEND

  FUNCTION Exp() AS Complex
    DIM scale AS DOUBLE = EXP(SELF.x)
    Exp = NEW Complex(scale * COS(SELF.y), scale * SIN(SELF.y))
  FEND

  REM Principal natural logarithm: ln|z| + i*arg(z). The logarithm of 0 is an error.
  FUNCTION Log() AS Complex
    IF SELF.IsZero() THEN
      THROW "complex Log: logarithm of zero"
    END IF
    Log = NEW Complex(LOG(SELF.Magnitude()), SELF.Argument())
  FEND

  REM sin(a + bi) = sin a cosh b + i cos a sinh b
  FUNCTION Sin() AS Complex
    DIM up AS DOUBLE = EXP(SELF.y)
    DIM down AS DOUBLE = EXP(-SELF.y)
    Sin = NEW Complex(SIN(SELF.x) * (up + down) / 2, COS(SELF.x) * (up - down) / 2)
  FEND

  REM cos(a + bi) = cos a cosh b - i sin a sinh b
  FUNCTION Cos() AS Complex
    DIM up AS DOUBLE = EXP(SELF.y)
    DIM down AS DOUBLE = EXP(-SELF.y)
    Cos = NEW Complex(COS(SELF.x) * (up + down) / 2, -SIN(SELF.x) * (up - down) / 2)
  FEND

  FUNCTION ToPolarString$() AS STRING
    DIM theta AS DOUBLE = SELF.Argument()
    IF theta < 0 THEN
      ToPolarString$ = STR$(SELF.Magnitude()) + " * e^(-i*" + STR$(-theta) + ")"
    ELSE
      ToPolarString$ = STR$(SELF.Magnitude()) + " * e^(i*" + STR$(theta) + ")"
    END IF
  FEND
END CLASS

REM There are no static methods: these module-level functions build Complex values.

FUNCTION ComplexZero() AS Complex
  ComplexZero = NEW Complex(0, 0)
FEND

FUNCTION ComplexOne() AS Complex
  ComplexOne = NEW Complex(1, 0)
FEND

REM The imaginary unit.
FUNCTION ComplexI() AS Complex
  ComplexI = NEW Complex(0, 1)
FEND

FUNCTION ComplexFromReal(PARAMETER a AS DOUBLE) AS Complex
  ComplexFromReal = NEW Complex(a, 0)
FEND

REM r * (cos theta + i sin theta), theta in radians.
FUNCTION ComplexFromPolar(PARAMETER r AS DOUBLE, PARAMETER theta AS DOUBLE) AS Complex
  ComplexFromPolar = NEW Complex(r * COS(theta), r * SIN(theta))
FEND

REM Parses "a", "bi", "i", "-i", "a + bi", "a - bi", "a+bi" and "a-bi". Spaces are
REM allowed around the signs; anything else THROWs 'invalid complex number: "..."'.
FUNCTION ComplexFromString(PARAMETER s$ AS STRING) AS Complex
  DIM text$ AS STRING = TRIM$(s$)
  CALL ComplexCheckSpaces(text$, s$)
  DIM t$ AS STRING = REPLACE$(text$, " ", "")
  DIM re AS DOUBLE = 0
  DIM im AS DOUBLE = 0
  IF RIGHT$(t$, 1) = "i" THEN
    DIM body$ AS STRING = LEFT$(t$, LEN(t$) - 1)
    DIM split AS INTEGER = 0
    DIM k AS INTEGER = LEN(body$)
    DIM prev$ AS STRING
    REM Find the last + or - that separates the parts: not a leading sign and
    REM not the sign of an exponent such as 1e-3.
    WHILE k >= 2 AND split = 0
      prev$ = MID$(body$, k - 1, 1)
      IF (MID$(body$, k, 1) = "+" OR MID$(body$, k, 1) = "-") AND prev$ <> "e" AND prev$ <> "E" THEN
        split = k
      END IF
      k = k - 1
    WEND
    IF split > 0 THEN
      re = ComplexParseNumber(LEFT$(body$, split - 1), s$)
      im = ComplexParseImaginary(MID$(body$, split), s$)
    ELSE
      im = ComplexParseImaginary(body$, s$)
    END IF
  ELSE
    re = ComplexParseNumber(t$, s$)
  END IF
  ComplexFromString = NEW Complex(re, im)
FEND

REM Helpers of ComplexFromString.

REM A space is only allowed next to a sign (or before the final i).
FUNCTION ComplexCheckSpaces(PARAMETER text$ AS STRING, PARAMETER whole$ AS STRING)
  DIM prev$ AS STRING = ""
  DIM ch$ AS STRING
  DIM spaced AS BOOLEAN = FALSE
  DIM k AS INTEGER
  FOR k = 1 TO LEN(text$)
    ch$ = MID$(text$, k, 1)
    IF ch$ = " " THEN
      spaced = TRUE
    ELSE
      IF spaced AND prev$ <> "+" AND prev$ <> "-" AND ch$ <> "+" AND ch$ <> "-" AND ch$ <> "i" THEN
        THROW "invalid complex number: """ + whole$ + """"
      END IF
      spaced = FALSE
      prev$ = ch$
    END IF
  NEXT
FEND

FUNCTION ComplexParseNumber(PARAMETER t$ AS STRING, PARAMETER whole$ AS STRING) AS DOUBLE
  TRY
    ComplexParseNumber = VAL(t$)
  CATCH
    THROW "invalid complex number: """ + whole$ + """"
  END TRY
FEND

REM The coefficient of i: "" and "+" mean 1, "-" means -1.
FUNCTION ComplexParseImaginary(PARAMETER t$ AS STRING, PARAMETER whole$ AS STRING) AS DOUBLE
  IF t$ = "" OR t$ = "+" THEN
    ComplexParseImaginary = 1
  ELSEIF t$ = "-" THEN
    ComplexParseImaginary = -1
  ELSE
    ComplexParseImaginary = ComplexParseNumber(t$, whole$)
  END IF
FEND

REM ---------------------------------------------------------------------------
REM Unit tests. They are skipped when the file is run or imported normally.
REM   basic-vm --test lib/complexutil.bas                  (these tests only)
REM   basic-vm --test --test-imports complex.bas           (also from a program
REM                                                         that imports this file)
REM Floating point results are compared with ASSERTNEAR or ComplexNear; exact
REM results with ASSERTEQ; error paths with TRY ... CATCH.
REM
REM Note: the "+ 0" in the constructor cannot be observed from BASIC. Every
REM arithmetic operation, unary minus, STR$ and ATN2 already normalise negative
REM zero, so a negative zero never reaches the constructor and no test can tell
REM the difference; it is defensive only and deliberately has no test.
REM ---------------------------------------------------------------------------

REM True when both parts of z are within 1e-9 of re and im. It is a boolean
REM function (not a helper that asserts) so that a failing
REM   ASSERT ComplexNear(z, 1, 2), "message"
REM is reported at the line of the test statement, not inside a helper.
FUNCTION ComplexNear(PARAMETER z AS Complex, PARAMETER re AS DOUBLE, PARAMETER im AS DOUBLE) AS BOOLEAN
  ComplexNear = (ABS(z.Real() - re) <= 0.000000001 AND ABS(z.Imaginary() - im) <= 0.000000001)
FEND

TEST ComplexNear_checksBothPartsAndTheTolerance
  DIM z AS Complex = NEW Complex(1, 2)
  ASSERT ComplexNear(z, 1, 2)
  ASSERT ComplexNear(z, 1.0000000001, 2)
  ASSERT NOT ComplexNear(z, 1.1, 2), "real part differs"
  ASSERT NOT ComplexNear(z, 1, 2.1), "imaginary part differs"
END TEST

TEST Constructor_keepsParts
  DIM z AS Complex = NEW Complex(3, -4)
  ASSERTEQ 3, z.Real()
  ASSERTEQ -4, z.Imaginary()
END TEST

TEST Constructor_zeroPrintsWithoutASign
  ASSERTEQ "0", NEW Complex(0, 0).ToString$()
  ASSERTEQ "0", ComplexZero().Negate().ToString$()
END TEST

TEST Add_basic
  DIM z AS Complex = NEW Complex(3, 4).Add(NEW Complex(1, 2))
  ASSERT ComplexNear(z, 4, 6), "got " + z.ToString$()
END TEST

TEST Negate_flipsBothPartsAndLeavesTheReceiver
  DIM a AS Complex = NEW Complex(3, -4)
  DIM z AS Complex = a.Negate()
  ASSERT ComplexNear(z, -3, 4), "got " + z.ToString$()
  ASSERT ComplexNear(NEW Complex(-1.5, 2).Negate(), 1.5, -2)
  ASSERTEQ "3 - 4i", a.ToString$()
END TEST

TEST Phase_isTheArgument
  ASSERTNEAR PI() / 2, NEW Complex(0, 2).Phase(), 0.000000001
  ASSERTNEAR -3 * PI() / 4, NEW Complex(-1, -1).Phase(), 0.000000001
  ASSERTNEAR PI(), NEW Complex(-2, 0).Phase(), 0.000000001
END TEST

TEST Norm_isTheSquaredMagnitude
  ASSERTNEAR 25, NEW Complex(3, -4).Norm(), 0.000000001
  ASSERTNEAR 5, NEW Complex(1, 2).Norm(), 0.000000001
  ASSERTNEAR 4, NEW Complex(0, -2).Norm(), 0.000000001
END TEST

TEST AddReal_changesOnlyTheRealPart
  DIM z AS Complex = NEW Complex(1, 2).AddReal(0.5)
  ASSERT ComplexNear(z, 1.5, 2), "got " + z.ToString$()
END TEST

TEST SubtractReal_changesOnlyTheRealPart
  DIM z AS Complex = NEW Complex(1, 2).SubtractReal(0.5)
  ASSERT ComplexNear(z, 0.5, 2), "got " + z.ToString$()
END TEST

TEST MultiplyReal_scalesBothParts
  DIM z AS Complex = NEW Complex(1, -2).MultiplyReal(3)
  ASSERT ComplexNear(z, 3, -6), "got " + z.ToString$()
  ASSERT ComplexNear(NEW Complex(1, -2).MultiplyReal(-0.5), -0.5, 1)
  ASSERT NEW Complex(4, 4).MultiplyReal(0).IsZero(), "zero factor"
END TEST

TEST IsImaginary_onlyForANonZeroImaginaryPartAndNoRealPart
  ASSERT NEW Complex(0, 2).IsImaginary()
  ASSERT NEW Complex(0, -2).IsImaginary()
  ASSERT NOT NEW Complex(1, 2).IsImaginary(), "has a real part"
  ASSERT NOT NEW Complex(1, 0).IsImaginary(), "is real"
  ASSERT NOT ComplexZero().IsImaginary(), "zero is not imaginary"
END TEST

TEST IsReal_ignoresTheRealPart
  ASSERT NEW Complex(5, 0).IsReal()
  ASSERT NEW Complex(0, 0).IsReal()
  ASSERT NOT NEW Complex(5, 1).IsReal()
  ASSERT NOT ComplexI().IsReal()
END TEST

TEST Add_doesNotChangeItsReceiver
  DIM a AS Complex = NEW Complex(1, 1)
  DIM b AS Complex = a.Add(NEW Complex(5, 5))
  ASSERTEQ "1 + 1i", a.ToString$()
  ASSERTEQ "6 + 6i", b.ToString$()
END TEST

TEST Subtract_basic
  DIM z AS Complex = NEW Complex(3, 4).Subtract(NEW Complex(1, 6))
  ASSERT ComplexNear(z, 2, -2), "got " + z.ToString$()
END TEST

TEST Multiply_basic
  REM (3 + 4i)(1 + 2i) = 3 + 6i + 4i - 8 = -5 + 10i
  DIM z AS Complex = NEW Complex(3, 4).Multiply(NEW Complex(1, 2))
  ASSERT ComplexNear(z, -5, 10), "got " + z.ToString$()
END TEST

TEST Multiply_iSquaredIsMinusOne
  DIM z AS Complex = ComplexI().Multiply(ComplexI())
  ASSERT ComplexNear(z, -1, 0), "got " + z.ToString$()
END TEST

TEST Divide_basic
  REM (3 + 4i) / (1 + 2i) = (11 - 2i) / 5
  DIM z AS Complex = NEW Complex(3, 4).Divide(NEW Complex(1, 2))
  ASSERT ComplexNear(z, 2.2, -0.4), "got " + z.ToString$()
END TEST

TEST Divide_byZeroThrows
  DIM caught AS BOOLEAN = FALSE
  TRY
    DIM z AS Complex = NEW Complex(1, 1).Divide(ComplexZero())
  CATCH e$
    caught = TRUE
    ASSERTEQ "complex division by zero", e$
  END TRY
  ASSERT caught, "dividing by zero must throw"
END TEST

TEST Conjugate_negatesTheImaginaryPart
  DIM z AS Complex = NEW Complex(3, 4).Conjugate()
  ASSERT ComplexNear(z, 3, -4), "got " + z.ToString$()
  ASSERT NEW Complex(5, 0).Conjugate().IsReal()
END TEST

TEST Magnitude_of3and4Is5
  ASSERTNEAR 5, NEW Complex(3, 4).Magnitude(), 0.000000001
  ASSERTNEAR 25, NEW Complex(3, -4).MagnitudeSquared(), 0.000000001
END TEST

TEST Argument_quadrants
  ASSERTNEAR 0, NEW Complex(1, 0).Argument(), 0.000000001
  ASSERTNEAR PI() / 2, ComplexI().Argument(), 0.000000001
  ASSERTNEAR PI(), NEW Complex(-1, 0).Argument(), 0.000000001
  ASSERTNEAR -PI() / 4, NEW Complex(1, -1).Argument(), 0.000000001
END TEST

TEST Sqrt_ofNegativeRealIsImaginary
  DIM z AS Complex = NEW Complex(-4, 0).Sqrt()
  ASSERT ComplexNear(z, 0, 2), "got " + z.ToString$()
END TEST

TEST Sqrt_ofPerfectSquare
  DIM z AS Complex = NEW Complex(3, 4).Sqrt()
  ASSERT ComplexNear(z, 2, 1), "got " + z.ToString$()
  ASSERT ComplexNear(ComplexZero().Sqrt(), 0, 0), "sqrt(0)"
END TEST

TEST Power_wholeExponents
  ASSERT ComplexNear(NEW Complex(2, 1).Power(3), 2, 11), "cube"
  ASSERT ComplexNear(NEW Complex(2, 1).Power(0), 1, 0), "zeroth power"
  ASSERT ComplexNear(NEW Complex(2, 1).Power(-2), 0.12, -0.16), "negative power"
END TEST

TEST Power_fractionalExponentThrows
  DIM message$ AS STRING = ""
  TRY
    DIM z AS Complex = NEW Complex(2, 1).Power(0.5)
  CATCH e$
    message$ = e$
  END TRY
  ASSERTEQ "complex Power: exponent must be a whole number", message$
END TEST

TEST Exp_ofIPiIsMinusOne
  DIM z AS Complex = ComplexFromReal(PI()).Multiply(ComplexI()).Exp()
  ASSERT ComplexNear(z, -1, 0), "got " + z.ToString$()
END TEST

TEST Log_inversesExp
  DIM z AS Complex = NEW Complex(1.5, -0.5)
  DIM back AS Complex = z.Log().Exp()
  ASSERT ComplexNear(back, 1.5, -0.5), "got " + back.ToString$()
END TEST

TEST Log_ofZeroThrows
  DIM caught AS BOOLEAN = FALSE
  TRY
    DIM z AS Complex = ComplexZero().Log()
  CATCH e$
    caught = TRUE
    ASSERTEQ "complex Log: logarithm of zero", e$
  END TRY
  ASSERT caught, "log of zero must throw"
END TEST

TEST FromPolar_unitCircle
  ASSERT ComplexNear(ComplexFromPolar(2, PI() / 2), 0, 2), "2 at 90 degrees"
  ASSERT ComplexNear(ComplexFromPolar(1, 0), 1, 0), "1 at 0 degrees"
END TEST

TEST FromString_acceptedForms
  ASSERT ComplexNear(ComplexFromString("3 + 4i"), 3, 4), "3 + 4i"
  ASSERT ComplexNear(ComplexFromString("2.5-1.5i"), 2.5, -1.5), "2.5-1.5i"
  ASSERT ComplexNear(ComplexFromString("-i"), 0, -1), "-i"
  ASSERT ComplexNear(ComplexFromString("i"), 0, 1), "i"
  ASSERT ComplexNear(ComplexFromString("7"), 7, 0), "7"
  ASSERT ComplexNear(ComplexFromString("  1e-3 + 2i "), 0.001, 2), "1e-3 + 2i"
END TEST

TEST FromString_exponentSignsAreNotSeparators
  ASSERT ComplexNear(ComplexFromString("1e-3"), 0.001, 0), "1e-3"
  ASSERT ComplexNear(ComplexFromString("2E+2"), 200, 0), "2E+2"
  ASSERT ComplexNear(ComplexFromString("1E+3i"), 0, 1000), "1E+3i"
  ASSERT ComplexNear(ComplexFromString("2e-2i"), 0, 0.02), "2e-2i"
  ASSERT ComplexNear(ComplexFromString("3 + 1e-3i"), 3, 0.001), "3 + 1e-3i"
  ASSERT ComplexNear(ComplexFromString("2E+2 - 1E-2i"), 200, -0.01), "2E+2 - 1E-2i"
END TEST

TEST FromString_signsAndSpacing
  ASSERT ComplexNear(ComplexFromString("-3 - 4i"), -3, -4), "-3 - 4i"
  ASSERT ComplexNear(ComplexFromString("1+i"), 1, 1), "1+i"
  ASSERT ComplexNear(ComplexFromString("1 - i"), 1, -1), "1 - i"
  ASSERT ComplexNear(ComplexFromString("+2i"), 0, 2), "+2i"
END TEST

TEST FromString_rejectsGarbage
  DIM message$ AS STRING = ""
  TRY
    DIM z AS Complex = ComplexFromString("1 + 2x")
  CATCH e$
    message$ = e$
  END TRY
  ASSERTEQ "invalid complex number: ""1 + 2x""", message$
END TEST

TEST ToString_forms
  ASSERTEQ "3 + 4i", NEW Complex(3, 4).ToString$()
  ASSERTEQ "3 - 4i", NEW Complex(3, -4).ToString$()
  ASSERTEQ "5", NEW Complex(5, 0).ToString$()
END TEST

TEST ToString_roundTripsThroughFromString
  DIM z AS Complex = NEW Complex(-1.25, 0.5)
  DIM back AS Complex = ComplexFromString(z.ToString$())
  ASSERT z.Equals(back), "round trip: " + back.ToString$()
END TEST

TEST IsZero_onlyForZero
  ASSERT ComplexZero().IsZero()
  ASSERT NOT NEW Complex(0, 1).IsZero()
  ASSERT NOT ComplexOne().IsZero()
END TEST

TEST Equals_usesTolerance
  DIM a AS Complex = NEW Complex(1, 1)
  DIM b AS Complex = NEW Complex(1.001, 1)
  ASSERT NOT a.Equals(b)
  ASSERT a.Equals(b, 0.01)
END TEST

TEST Equals_comparesBothParts
  DIM a AS Complex = NEW Complex(1, 1)
  ASSERT a.Equals(NEW Complex(1, 1)), "identical"
  ASSERT NOT a.Equals(NEW Complex(1, 2)), "imaginary part differs"
  ASSERT NOT a.Equals(NEW Complex(2, 1)), "real part differs"
  ASSERT NOT a.Equals(NEW Complex(1, 1.5), 0.25), "imaginary part outside tolerance"
  ASSERT NOT a.Equals(NEW Complex(1.5, 1), 0.25), "real part outside tolerance"
  ASSERT a.Equals(NEW Complex(1.5, 1.5), 0.5), "a difference equal to the tolerance is equal"
END TEST

TEST Equals_negativeToleranceThrows
  DIM message$ AS STRING = ""
  TRY
    DIM same AS Boolean = ComplexOne().Equals(ComplexOne(), -0.5)
  CATCH e$
    message$ = e$
  END TRY
  ASSERTEQ "complex Equals: tolerance must not be negative", message$
END TEST

TEST Pow_ofAWholeComplexExponent
  DIM z AS Complex = NEW Complex(1, 1).Pow(NEW Complex(2, 0))
  ASSERT ComplexNear(z, 0, 2), "(1 + i)^2, got " + z.ToString$()
  DIM root AS Complex = NEW Complex(4, 0).Pow(NEW Complex(0.5, 0))
  ASSERT ComplexNear(root, 2, 0), "4^0.5, got " + root.ToString$()
END TEST

TEST Pow_iToTheIIsReal
  REM i^i = e^(-pi/2)
  DIM z AS Complex = ComplexI().Pow(ComplexI())
  ASSERT ComplexNear(z, 0.20787957635076193, 0), "got " + z.ToString$()
END TEST

TEST Pow_ofZero
  ASSERT ComplexNear(ComplexZero().Pow(ComplexZero()), 1, 0), "0^0 is 1"
  ASSERT ComplexNear(ComplexZero().Pow(NEW Complex(2, 3)), 0, 0), "0^w is 0 for a positive real part"
  DIM failures AS INTEGER = 0
  TRY
    DIM a AS Complex = ComplexZero().Pow(NEW Complex(-1, 0))
  CATCH e$
    ASSERTEQ "complex Pow: zero needs an exponent with a positive real part", e$
    failures = failures + 1
  END TRY
  TRY
    DIM b AS Complex = ComplexZero().Pow(NEW Complex(0, 1))
  CATCH
    failures = failures + 1
  END TRY
  ASSERTEQ 2, failures
END TEST

TEST Sin_ofAComplexNumber
  REM sin(1 + 2i) = sin 1 cosh 2 + i cos 1 sinh 2
  DIM z AS Complex = NEW Complex(1, 2).Sin()
  ASSERT ComplexNear(z, 3.165778513216168, 1.959601041421606), "got " + z.ToString$()
  ASSERT ComplexNear(NEW Complex(1, -2).Sin(), 3.165778513216168, -1.959601041421606), "odd in the imaginary part"
  ASSERT ComplexNear(NEW Complex(PI() / 2, 0).Sin(), 1, 0), "sin(pi/2)"
END TEST

TEST Cos_ofAComplexNumber
  REM cos(1 + 2i) = cos 1 cosh 2 - i sin 1 sinh 2
  DIM z AS Complex = NEW Complex(1, 2).Cos()
  ASSERT ComplexNear(z, 2.0327230070196656, -3.0518977991517997), "got " + z.ToString$()
  ASSERT ComplexNear(NEW Complex(1, -2).Cos(), 2.0327230070196656, 3.0518977991517997), "even in the real part"
  ASSERT ComplexNear(ComplexZero().Cos(), 1, 0), "cos(0)"
END TEST

TEST SinAndCos_obeyPythagoras
  DIM z AS Complex = NEW Complex(0.7, -1.3)
  DIM sum AS Complex = z.Sin().Square().Add(z.Cos().Square())
  ASSERT ComplexNear(sum, 1, 0), "sin^2 + cos^2, got " + sum.ToString$()
END TEST

TEST ToPolarString_positiveAndNegativeAngles
  ASSERTEQ "1 * e^(i*0)", ComplexOne().ToPolarString$()
  ASSERTEQ "2 * e^(i*" + STR$(PI() / 2) + ")", NEW Complex(0, 2).ToPolarString$()
  ASSERTEQ "1 * e^(-i*" + STR$(PI() / 2) + ")", NEW Complex(0, -1).ToPolarString$()
  ASSERTEQ "5 * e^(i*" + STR$(PI()) + ")", NEW Complex(-5, 0).ToPolarString$()
END TEST

TEST Reciprocal_ofZeroThrows
  DIM caught AS BOOLEAN = FALSE
  TRY
    DIM z AS Complex = ComplexZero().Reciprocal()
  CATCH e$
    caught = TRUE
  END TRY
  ASSERT caught
END TEST

TEST NthRoot_cubeOfEight
  DIM z AS Complex = NEW Complex(8, 0).NthRoot(3)
  ASSERT ComplexNear(z, 2, 0), "got " + z.ToString$()
END TEST

TEST NthRoot_rejectsZeroAndFractions
  DIM failures AS INTEGER = 0
  TRY
    DIM a AS Complex = ComplexOne().NthRoot(0)
  CATCH
    failures = failures + 1
  END TRY
  TRY
    DIM b AS Complex = ComplexOne().NthRoot(2.5)
  CATCH
    failures = failures + 1
  END TRY
  ASSERTEQ 2, failures
END TEST
