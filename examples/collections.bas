REM Indexed collections use one-based indices.
SCORES = ARRAY(10, 20, 30)
SCORES[2] = 25
FOR EACH SCORE IN SCORES
PRINT "Score: "; SCORE
NEXT SCORE

NAMES = LIST("Ada", "Grace")
APPEND NAMES, "Linus"
REMOVE NAMES, 2
FOR EACH NAME$ IN NAMES
PRINT "Name: "; NAME$
NEXT

COUNTS = MAP("apples", 3, "pears", 2)
COUNTS["pears"] = 4
PRINT "Pears: "; COUNTS["pears"]
FOR EACH COUNT IN COUNTS
PRINT "Count: "; COUNT
NEXT

FOR I = 1 TO LEN(SCORES)
PRINT "Indexed score: "; SCORES[I]
NEXT I

I = 3
WHILE I > 0
PRINT "Countdown: "; I
I = I - 1
WEND
GC
END
