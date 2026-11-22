REM Unit testing: TEST blocks and ASSERT statements.
REM
REM Run the program normally (the TEST blocks are skipped):
REM   basic-vm examples/testing.bas
REM Run the tests (the main program body is not run):
REM   basic-vm --test examples/testing.bas
REM   basic-vm --test --test-filter clamp examples/testing.bas
OPTION EXPLICIT

CONST MAX_ITEMS = 3

FUNCTION Clamp(PARAMETER value, PARAMETER low, PARAMETER high)
  IF value < low THEN
    Clamp = low
  ELSEIF value > high THEN
    Clamp = high
  ELSE
    Clamp = value
  END IF
FEND

FUNCTION Reverse$(PARAMETER text$ AS STRING) AS STRING
  DIM result$ AS STRING = ""
  DIM i AS INTEGER
  FOR i = LEN(text$) TO 1 STEP -1
    result$ = result$ + MID$(text$, i, 1)
  NEXT
  Reverse$ = result$
FEND

FUNCTION SafeDivide(PARAMETER a, PARAMETER b)
  IF b = 0 THEN
    THROW "cannot divide by zero"
  END IF
  SafeDivide = a / b
FEND

FUNCTION FirstItems(PARAMETER items)
  DIM result = LIST()
  DIM i AS INTEGER
  FOR i = 1 TO MIN(LEN(items), MAX_ITEMS)
    APPEND result, items[i]
  NEXT
  FirstItems = result
FEND

REM A tiny program: this is what a normal run does.
PRINT "Clamp(15, 0, 10) = "; Clamp(15, 0, 10)
PRINT "Reverse$(""stressed"") = "; Reverse$("stressed")

REM A test is a hidden function without parameters. It passes when it ends
REM without an error; a failed ASSERT* fails it.
TEST Clamp_limitsBothEnds
  ASSERTEQ 0, Clamp(-5, 0, 10)
  ASSERTEQ 10, Clamp(15, 0, 10)
  ASSERTEQ 7, Clamp(7, 0, 10), "values inside the range are unchanged"
END TEST

TEST Reverse_basic
  ASSERTEQ "desserts", Reverse$("stressed")
  ASSERTEQ "", Reverse$("")
END TEST

REM ASSERT checks any condition; ASSERTNEAR compares numbers within a tolerance.
TEST Clamp_acceptsFractions
  ASSERT Clamp(0.25, 0, 1) = 0.25
  ASSERTNEAR 0.3, Clamp(0.1 + 0.2, 0, 1), 0.000000001, "0.1 + 0.2 is not exactly 0.3"
END TEST

REM Collections are compared deeply.
TEST FirstItems_keepsAtMostThree
  ASSERTEQ LIST(1, 2, 3), FirstItems(LIST(1, 2, 3, 4, 5))
  ASSERTEQ LIST("a"), FirstItems(LIST("a"))
END TEST

REM An error is checked with TRY ... CATCH; ASSERT inside TRY is also catchable.
TEST SafeDivide_byZeroThrows
  DIM message$ AS STRING = ""
  TRY
    DIM x = SafeDivide(1, 0)
    ASSERT 0, "SafeDivide(1, 0) should have thrown"
  CATCH e$
    message$ = e$
  END TRY
  ASSERTEQ "cannot divide by zero", message$
END TEST

REM EXIT TEST ends a test early; it counts as a pass.
TEST Reverse_skipsTheRestWhenEmpty
  IF Reverse$("") = "" THEN
    EXIT TEST
  END IF
  ASSERT 0, "not reached"
END TEST

REM Tests are isolated: each runs on a fresh VM, so this global is 0 again here.
DIM counter AS INTEGER = 0

TEST Isolation_firstSetsTheCounter
  counter = counter + 1
  ASSERTEQ 1, counter
END TEST

TEST Isolation_secondStartsAgain
  ASSERTEQ 0, counter
END TEST
