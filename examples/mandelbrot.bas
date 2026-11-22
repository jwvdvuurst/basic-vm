REM Mandelbrot set example in BASIC

OPTION EXPLICIT

IMPORT "lib/complexutil.bas" AS Cx

CONST MAX_ITER = 100








SCREEN 800, 800, "mandelbrot"



REM Number of iterations before |z| exceeds 2 (MAX_ITER when it never does).
FUNCTION CalculateMandelbrot(PARAMETER Z AS Cx.Complex)
    LOCAL C
    LOCAL N
    C = Z
    N = 0
    DO WHILE N < MAX_ITER AND C.Magnitude() <= 2
        C = C.Multiply(C).Add(Z)
        N = N + 1
    LOOP
    CalculateMandelbrot = N
FEND

DIM X
DIM Y
DIM N
DIM P

GCLS
FOR Y = 0 TO HEIGHT() - 1
    FOR X = 0 TO WIDTH() - 1
        REM Map the pixel to the complex plane: roughly -2..2 on both axes.
        P = NEW Cx.Complex((X - WIDTH() / 2) / (WIDTH() / 4), (Y - HEIGHT() / 2) / (HEIGHT() / 4))
        N = CalculateMandelbrot(P)
        REM Points inside the set stay black; the others use palette colours 1..15.
        IF N < MAX_ITER THEN PSET X, Y, 1 + MOD(N, 15)
    NEXT
    DOEVENTS
NEXT

