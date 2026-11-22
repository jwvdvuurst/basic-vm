REM Complex numbers: the Complex class from lib/complexutil.bas.
OPTION EXPLICIT

IMPORT "lib/complexutil.bas" AS Cx

REM Rounds both parts so that floating point noise does not show up in the output.
FUNCTION Show$(PARAMETER z AS Cx.Complex) AS STRING
  Show$ = NEW Cx.Complex(ROUND(z.Real(), 6), ROUND(z.Imaginary(), 6)).ToString$()
FEND

DIM a AS Cx.Complex = NEW Cx.Complex(3, 4)
DIM b AS Cx.Complex = NEW Cx.Complex(1, 2)
PRINT "a = "; Show$(a); ", b = "; Show$(b)
PRINT "a + b = "; Show$(a.Add(b))
PRINT "a - b = "; Show$(a.Subtract(b))
PRINT "a * b = "; Show$(a.Multiply(b))
PRINT "a / b = "; Show$(a.Divide(b))
PRINT "conjugate of a = "; Show$(a.Conjugate())
PRINT "|a| = "; ROUND(a.Magnitude(), 6); ", arg(a) = "; ROUND(a.Argument(), 6)
PRINT "sqrt(-4) = "; Show$(NEW Cx.Complex(-4, 0).Sqrt())
PRINT "sqrt(a) = "; Show$(a.Sqrt())
PRINT "(2 + i)^3 = "; Show$(NEW Cx.Complex(2, 1).Power(3))
PRINT "(2 + i)^-2 = "; Show$(NEW Cx.Complex(2, 1).Power(-2))
PRINT "exp(i*pi) = "; Show$(Cx.ComplexFromReal(PI()).Multiply(Cx.ComplexI()).Exp())
PRINT "polar: "; Cx.ComplexFromString("3 + 4i").ToPolarString$()
PRINT "from polar: "; Show$(Cx.ComplexFromPolar(2, PI() / 2))
PRINT "parsed: "; Show$(Cx.ComplexFromString("-i")); ", "; Show$(Cx.ComplexFromString("2.5-1.5i"))

TRY
  PRINT Show$(a.Divide(Cx.ComplexZero()))
CATCH e$
  PRINT "caught: "; e$
END TRY
TRY
  PRINT Show$(Cx.ComplexFromString("1 + 2x"))
CATCH e$
  PRINT "caught: "; e$
END TRY
