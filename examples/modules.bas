REM Modules: IMPORT, aliases, qualified names, BYREF and named arguments.
OPTION EXPLICIT

PRINT "main starts"
IMPORT "lib/mathutil.bas" AS Math

PRINT "Square(7) = "; Math.Square(7)
PRINT "Scaled(2) = "; Math.Scaled(2)
PRINT "Scaled(2, factor := 5) = "; Math.Scaled(2, factor := 5)
PRINT "SCALE = "; Math.SCALE

DIM a AS INTEGER = 1
DIM b AS INTEGER = 2
CALL Math.Swap(a, b)
PRINT "after swap: "; a; " "; b

DIM p AS Math.Point = NEW Math.Point(x = 3, y = 4)
PRINT "point: "; p.x; ","; p.y
DIM o AS Math.Point = Math.Origin()
PRINT "origin: "; o.x; ","; o.y

PRINT "calls so far: "; Math.CallCount()
PRINT "calls (global): "; Math.calls
