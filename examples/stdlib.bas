REM Standard library tour with deterministic output.
PRINT "Math: "; ABS(-3); " "; SQRT(16); " "; MAX(2, 9, 4); " "; ROUND(2.567, 2); " "; INT(-2.5); " "; FIX(-2.5); " "; 2 ^ 10
S$ = "  Hello, World  "
T$ = TRIM$(S$)
PRINT UCASE$(T$); "|"; LCASE$(T$); "|"; LEFT$(T$, 5); "|"; RIGHT$(T$, 5); "|"; MID$(T$, 8, 5)
PRINT INSTR(T$, "World"); " "; REPLACE$(T$, "l", "L")
PARTS = SPLIT("pear,apple,fig", ",")
SORT PARTS
PRINT JOIN$(PARTS, "-")
INSERT PARTS, 1, "kiwi"
REVERSE PARTS
PRINT JOIN$(PARTS, " "); " / popped "; POP(PARTS); " / left "; LEN(PARTS)
M = MAP("x", 1, "y", 2)
PRINT JOIN$(KEYS(M), "+"); " "; HASKEY(M, "y"); " "; HASKEY(M, "z"); " "; CONTAINS(VALUES(M), 2)
RANDOMIZE 42
FIRST = RND()
RANDOMIZE 42
IF FIRST = RND() THEN PRINT "same seed, same number"
PRINT VAL(" 12.5 ") + 1; " "; STR$(7) + "!"
PRINT "Arguments: "; ARGC()
