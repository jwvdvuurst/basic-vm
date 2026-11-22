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
