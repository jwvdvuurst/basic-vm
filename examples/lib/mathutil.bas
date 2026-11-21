REM A small module: constants, a record type, a counter global and functions.
OPTION EXPLICIT

CONST SCALE = 10
DIM calls AS INTEGER

TYPE Point
  x AS INTEGER
  y AS INTEGER
END TYPE

PRINT "mathutil loaded"

FUNCTION Square(PARAMETER n AS INTEGER) AS INTEGER
  calls = calls + 1
  Square = n * n
FEND

FUNCTION Scaled(PARAMETER n AS INTEGER, PARAMETER factor AS INTEGER = 3) AS INTEGER
  calls = calls + 1
  Scaled = n * factor * SCALE
FEND

FUNCTION Swap(PARAMETER BYREF a AS INTEGER, PARAMETER BYREF b AS INTEGER)
  DIM t AS INTEGER = a
  a = b
  b = t
FEND

FUNCTION Origin() AS Point
  Origin = NEW Point(x = 0, y = 0)
FEND

FUNCTION CallCount() AS INTEGER
  CallCount = calls
FEND
