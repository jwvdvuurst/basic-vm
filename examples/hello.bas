REM Numbered and unnumbered lines can be mixed.
LET NAME$ = "BASIC"
PRINT "Hello from "; NAME$; " on a bytecode VM!"
COUNT = 1
100 PRINT "Count: "; COUNT
COUNT = COUNT + 1
IF COUNT <= 3000000 THEN 100
GC
END
