REM Structured control flow tour; output is short and bounded.
CONST LIMIT = 10
FOR I = 1 TO LIMIT
  IF I MOD 2 = 0 THEN
    CONTINUE FOR
  ELSEIF I > 7 THEN
    EXIT FOR
  END IF
  SELECT CASE I
    CASE 1
      PRINT "I = one"
    CASE 3 TO 5
      PRINT "I = three to five: "; I
    CASE IS >= 7
      PRINT "I = seven or more"
    CASE ELSE
      PRINT "I = other"
  END SELECT
NEXT
N = 0
DO
  N = N + 1
LOOP UNTIL N >= 3
PRINT "N = "; N
IF N > 0 ANDALSO 10 / N > 3 THEN PRINT "short-circuit ok"
IF NOT (N = 3) OR TRUE THEN PRINT "logic ok"
PRINT "First multiple of 7 above 20: "; FIRST_MULTIPLE(7, 20)
END
FUNCTION FIRST_MULTIPLE(PARAMETER STEP_SIZE, PARAMETER ABOVE)
  LOCAL K
  FIRST_MULTIPLE = 0
  FOR K = 1 TO 100
    IF STEP_SIZE * K > ABOVE THEN
      FIRST_MULTIPLE = STEP_SIZE * K
      EXIT FUNCTION
    END IF
  NEXT
FEND
