REM Draws a small scene. Run headless with:
REM   basic-vm --headless-graphics --screenshot scene.ppm examples/graphics.bas
SCREEN 160, 120, "Graphics demo"
GCLS RGB(0, 0, 64)
GCOLOR 14
CIRCLE 130, 25, 12, 14, FILL
GCOLOR 2
LINE 0, 100, 159, 119, , BF
LINE 20, 60, 70, 100, 4, BF
LINE 20, 60, 70, 100, 15, B
LINE 15, 60, 45, 40, 12
LINE 45, 40, 75, 60, 12
TEXT 4, 4, "HELLO", 15
PSET 80, 80, RGB(255, 0, 255)
FLIP
FOR I = 1 TO 5
    DOEVENTS
    SLEEP 16
    IF CLOSED() THEN EXIT FOR
NEXT
