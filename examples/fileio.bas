REM File I/O and error handling demo. It writes a scratch file in the
REM current directory and deletes it again before finishing.
NAME$ = "fileio-demo.tmp"

OPEN NAME$ FOR OUTPUT AS #1
PRINT #1, "alpha"
PRINT #1, "beta"; ","; 42
CLOSE #1

OPEN NAME$ FOR APPEND AS #1
PRINT #1, "gamma"
CLOSE #1

OPEN NAME$ FOR INPUT AS #1
DO UNTIL EOF(1)
  LINE INPUT #1, LINE$
  PRINT "read: "; LINE$
LOOP
CLOSE #1

REM Binary access: overwrite the first three bytes and read them back.
OPEN NAME$ FOR BINARY AS #2
PUT #2, LIST(0, 255, 65)
SEEK #2, 0
BYTES = GET(2, 3)
PRINT LEN(BYTES); " bytes: "; BYTES[1]; " "; BYTES[2]; " "; BYTES[3]
PRINT "size: "; LOF(2)
CLOSE #2

REM Errors are catchable; FINALLY always runs.
TRY
  OPEN "no-such-directory/missing.txt" FOR INPUT AS #3
  PRINT "not reached"
CATCH MESSAGE$
  PRINT "caught: "; INSTR(MESSAGE$, "file not found") > 0
FINALLY
  IF ISOPEN(3) THEN CLOSE #3
  PRINT "cleanup done"
END TRY

KILL NAME$
PRINT "scratch file exists: "; FILEEXISTS(NAME$)
