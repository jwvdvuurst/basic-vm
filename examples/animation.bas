REM Flicker-free animation with a double-buffered screen (SCREEN w, h, title$, 2).
REM Each frame is drawn into the hidden back buffer; only FLIP shows it.
REM   basic-vm --headless-graphics --screenshot ball.png examples/animation.bas
REM   basic-vm --auto-close-ms 3000 examples/animation.bas      (real window)
FRAMES = 60
SCREEN 160, 120, "Animation", 2
FOR FRAME = 0 TO FRAMES - 1
    IF CLOSED() THEN EXIT FOR
    GCLS 1
    LINE 0, 110, 159, 119, 2, BF
    CIRCLE 10 + 2 * FRAME, 40 + FRAME, 8, 14, FILL
    TEXT 4, 4, "FRAME " + STR$(FRAME + 1), 15
    FLIP
    DOEVENTS
    SLEEP 16
NEXT
SCREEN 0
PRINT "Animation done"
