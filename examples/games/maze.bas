REM MAZE MUNCH - an original maze-chase game.
REM Collect all the dots while avoiding the red pursuer.
REM Arrow keys or WASD steer; ESC or closing the window quits.
REM   basic-vm examples\maze.bas
REM   basic-vm --test examples\maze.bas
OPTION EXPLICIT

CONST COLS = 13
CONST ROWS = 11
CONST CELL = 20
CONST TOP = 32

DIM gMaze AS LIST = LIST()
DIM gPlayerX AS INTEGER
DIM gPlayerY AS INTEGER
DIM gEnemyX AS INTEGER
DIM gEnemyY AS INTEGER
DIM gPreviousX AS INTEGER
DIM gPreviousY AS INTEGER
DIM gDots AS INTEGER
DIM gScore AS INTEGER
DIM gState AS INTEGER

FUNCTION NewGame()
  LOCAL x
  LOCAL y
  gMaze = LIST("#############", "#...........#", "#.###.###.#.#", "#...#...#.#.#", "###.#.#.#.#.#", "#.....#.....#", "#.###.###.#.#", "#.#.......#.#", "#.#.###.###.#", "#...........#", "#############")
  gPlayerX = 2
  gPlayerY = 2
  gEnemyX = 12
  gEnemyY = 10
  gPreviousX = 0
  gPreviousY = 0
  gDots = 0
  gScore = 0
  gState = 0
  FOR y = 1 TO ROWS
    FOR x = 1 TO COLS
      IF MID$(gMaze[y], x, 1) = "." THEN gDots = gDots + 1
    NEXT
  NEXT
  REM The starting square is already visited.
  gMaze[2] = LEFT$(gMaze[2], 1) + " " + MID$(gMaze[2], 3)
  gDots = gDots - 1
FEND

FUNCTION OpenCell(PARAMETER x, PARAMETER y)
  OpenCell = 0
  IF x < 1 OR x > COLS OR y < 1 OR y > ROWS THEN EXIT FUNCTION
  IF MID$(gMaze[y], x, 1) <> "#" THEN OpenCell = 1
FEND

FUNCTION Caught()
  gState = 2
  SOUND 180, 180, 45
  SOUND 110, 250, 45
FEND

FUNCTION MovePlayer(PARAMETER dx, PARAMETER dy)
  LOCAL x
  LOCAL y
  IF gState <> 0 THEN EXIT FUNCTION
  IF dx = 0 AND dy = 0 THEN EXIT FUNCTION
  x = gPlayerX + dx
  y = gPlayerY + dy
  IF OpenCell(x, y) = 0 THEN EXIT FUNCTION
  gPlayerX = x
  gPlayerY = y
  IF x = gEnemyX AND y = gEnemyY THEN
    CALL Caught()
    EXIT FUNCTION
  END IF
  IF MID$(gMaze[y], x, 1) = "." THEN
    gMaze[y] = LEFT$(gMaze[y], x - 1) + " " + MID$(gMaze[y], x + 1)
    gDots = gDots - 1
    gScore = gScore + 10
    SOUND 700, 35, 25
    IF gDots = 0 THEN
      gState = 1
      SOUND 880, 100, 40
      SOUND 1175, 200, 40
    END IF
  END IF
FEND

REM Avoid reversing at junctions, unless the player is behind the pursuer.
FUNCTION MoveEnemy()
  LOCAL i
  LOCAL x
  LOCAL y
  LOCAL bestX
  LOCAL bestY
  LOCAL distance
  LOCAL bestDistance
  LOCAL dx AS LIST
  LOCAL dy AS LIST
  IF gState <> 0 THEN EXIT FUNCTION
  dx = LIST(1, 0, -1, 0)
  dy = LIST(0, 1, 0, -1)
  bestX = gEnemyX
  bestY = gEnemyY
  bestDistance = 999
  FOR i = 1 TO 4
    x = gEnemyX + dx[i]
    y = gEnemyY + dy[i]
    IF OpenCell(x, y) AND (x <> gPreviousX OR y <> gPreviousY OR (x = gPlayerX AND y = gPlayerY)) THEN
      distance = ABS(x - gPlayerX) + ABS(y - gPlayerY)
      IF distance < bestDistance THEN
        bestDistance = distance
        bestX = x
        bestY = y
      END IF
    END IF
  NEXT
  IF bestDistance = 999 AND OpenCell(gPreviousX, gPreviousY) THEN
    bestX = gPreviousX
    bestY = gPreviousY
  END IF
  gPreviousX = gEnemyX
  gPreviousY = gEnemyY
  gEnemyX = bestX
  gEnemyY = bestY
  IF gEnemyX = gPlayerX AND gEnemyY = gPlayerY THEN CALL Caught()
FEND

FUNCTION DrawGame()
  LOCAL x
  LOCAL y
  LOCAL px
  LOCAL py
  LOCAL tile$ AS STRING
  GCLS 0
  TEXT 4, 4, "MAZE MUNCH  SCORE " + STR$(gScore), 15
  TEXT 4, 16, "ARROWS/WASD  ESC QUITS", 7
  FOR y = 1 TO ROWS
    FOR x = 1 TO COLS
      px = (x - 1) * CELL
      py = TOP + (y - 1) * CELL
      tile$ = MID$(gMaze[y], x, 1)
      IF tile$ = "#" THEN LINE px + 1, py + 1, px + CELL - 2, py + CELL - 2, 1, BF
      IF tile$ = "." THEN CIRCLE px + 10, py + 10, 2, 15, FILL
    NEXT
  NEXT
  CIRCLE (gPlayerX - 1) * CELL + 10, TOP + (gPlayerY - 1) * CELL + 10, 7, 14, FILL
  CIRCLE (gEnemyX - 1) * CELL + 10, TOP + (gEnemyY - 1) * CELL + 10, 7, 12, FILL
  IF gState = 1 THEN TEXT 20, 120, "ALL DOTS! YOU WIN", 10
  IF gState = 2 THEN TEXT 20, 120, "CAUGHT! GAME OVER", 12
  IF gState <> 0 THEN TEXT 20, 132, "R: RESTART  ESC: QUIT", 15
  FLIP
FEND

TEST Walls_and_bounds
  CALL NewGame()
  ASSERTEQ 0, OpenCell(0, 2)
  ASSERTEQ 0, OpenCell(1, 1)
  ASSERTEQ 1, OpenCell(2, 2)
  CALL MovePlayer(-1, 0)
  ASSERTEQ 2, gPlayerX
  ASSERTEQ 0, gScore
END TEST

TEST Collect_once_and_win
  CALL NewGame()
  ASSERTEQ 65, gDots
  CALL MovePlayer(1, 0)
  ASSERTEQ 10, gScore
  ASSERTEQ 64, gDots
  CALL MovePlayer(-1, 0)
  CALL MovePlayer(1, 0)
  ASSERTEQ 10, gScore
  gDots = 1
  CALL MovePlayer(1, 0)
  ASSERTEQ 1, gState
  ASSERTEQ 0, gDots
  CALL MovePlayer(1, 0)
  ASSERTEQ 4, gPlayerX
END TEST

TEST Collision_in_both_directions
  CALL NewGame()
  gEnemyX = 3
  gEnemyY = 2
  CALL MovePlayer(1, 0)
  ASSERTEQ 2, gState
  ASSERTEQ 0, gScore
  CALL NewGame()
  gEnemyX = 3
  gEnemyY = 2
  CALL MoveEnemy()
  ASSERTEQ 2, gState
END TEST

TEST Pursuer_does_not_reverse_at_a_junction
  CALL NewGame()
  gEnemyX = 3
  gEnemyY = 2
  gPreviousX = 4
  gPreviousY = 2
  gPlayerX = 12
  gPlayerY = 2
  CALL MoveEnemy()
  ASSERTEQ 2, gEnemyX
  ASSERTEQ 2, gEnemyY
  CALL MoveEnemy()
  ASSERTEQ 2, gEnemyX
  ASSERTEQ 3, gEnemyY
END TEST

TEST Pursuer_reverses_at_dead_end_and_restart_resets_game
  CALL NewGame()
  gEnemyX = 3
  gEnemyY = 2
  gPreviousX = 2
  gPreviousY = 2
  gPlayerX = 12
  gPlayerY = 2
  gMaze[2] = LEFT$(gMaze[2], 3) + "#" + MID$(gMaze[2], 5)
  CALL MoveEnemy()
  ASSERTEQ 2, gEnemyX
  ASSERTEQ 2, gEnemyY
  gScore = 10
  gState = 2
  CALL NewGame()
  ASSERTEQ 0, gState
  ASSERTEQ 0, gScore
  ASSERTEQ 65, gDots
  ASSERTEQ 12, gEnemyX
  ASSERTEQ 0, gPreviousX
END TEST

REM Main loop: movement every eight frames, pursuer every third move.
DIM key$ AS STRING
DIM dx AS INTEGER = 0
DIM dy AS INTEGER = 0
DIM frame AS INTEGER = 0
DIM turns AS INTEGER = 0
CALL NewGame()
SCREEN COLS * CELL, TOP + ROWS * CELL, "Maze Munch", 2
SOUND 520, 80, 35
DO
  CALL DrawGame()
  IF CLOSED() THEN EXIT DO
  key$ = GETKEY$()
  IF key$ = "ESC" THEN EXIT DO
  IF key$ = "R" AND gState <> 0 THEN
    CALL NewGame()
    dx = 0
    dy = 0
    turns = 0
    frame = 0
  END IF
  IF key$ = "LEFT" OR key$ = "A" THEN
    dx = -1
    dy = 0
  END IF
  IF key$ = "RIGHT" OR key$ = "D" THEN
    dx = 1
    dy = 0
  END IF
  IF key$ = "UP" OR key$ = "W" THEN
    dx = 0
    dy = -1
  END IF
  IF key$ = "DOWN" OR key$ = "S" THEN
    dx = 0
    dy = 1
  END IF
  frame = frame + 1
  IF frame >= 8 THEN
    frame = 0
    IF dx <> 0 OR dy <> 0 THEN
      CALL MovePlayer(dx, dy)
      turns = turns + 1
      IF turns MOD 3 = 0 THEN CALL MoveEnemy()
    END IF
  END IF
  SLEEP 16
LOOP
SCREEN 0
