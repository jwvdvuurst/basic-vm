REM Interactive paint demo (opens a real window).
REM   Hold the left mouse button to draw, right button or C to clear,
REM   keys 1-9 pick a colour, ESC or closing the window quits.
REM Bounded run for automated checks:
REM   basic-vm --auto-close-ms 800 examples/paint.bas
PX = 0
PY = 0
DRAWING = 0
INK = 14
QUIT = 0

FUNCTION OnMouse(PARAMETER X, PARAMETER Y, PARAMETER B)
    IF B MOD 2 = 1 THEN
        IF DRAWING = 1 THEN
            LINE PX, PY, X, Y, INK
        ELSE
            PSET X, Y, INK
        END IF
        DRAWING = 1
    ELSE
        DRAWING = 0
    END IF
    REM Bit 2 of the button mask is the right button.
    IF INT(B / 2) MOD 2 = 1 THEN GCLS
    PX = X
    PY = Y
FEND

FUNCTION OnKey(PARAMETER K$)
    IF K$ = "ESC" THEN QUIT = 1
    IF K$ = "C" THEN GCLS
    IF K$ >= "1" THEN
        IF K$ <= "9" THEN INK = VAL(K$)
    END IF
FEND

FUNCTION OnClose()
    QUIT = 1
FEND

ON MOUSE CALL OnMouse
ON KEY CALL OnKey
ON CLOSE CALL OnClose

SCREEN 320, 240, "Paint - ESC quits"
GCLS
TEXT 4, 4, "Draw with the mouse", 15
WHILE QUIT = 0
    DOEVENTS
    SLEEP 16
    FLIP
WEND
SCREEN 0
PRINT "Goodbye"
