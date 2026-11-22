REM ======================================================================
REM LUNAR LANDER - bring a descent module down on the Moon.
REM Every turn lasts 10 seconds. You choose a fuel burn rate for the whole
REM turn; the engine pushes against lunar gravity. Touch down gently, or
REM crash. Fuel is limited, and with none left you are in free fall.
REM
REM Original game concept: the text-mode Lunar Lander genre, commonly
REM attributed to Jim Storer (1969); this is an independent implementation,
REM not a copy of the original listing.
REM
REM Play:        basic-vm examples\games\lander.bas [seed]
REM Run tests:   basic-vm --test examples\games\lander.bas
REM
REM The optional seed (a number) fixes the random starting altitude and
REM speed so a game can be replayed; without it the clock is used and the
REM seed is printed. The game ends gracefully when the input ends.
REM Units: metres, metres per second (speed is positive when descending),
REM kilograms of fuel. Thrust is 0.5 m/s^2 per kg/s burned.
REM ======================================================================
OPTION EXPLICIT

CONST GRAVITY = 1.62
CONST ACCEL_PER_RATE = 0.5
CONST MAX_BURN = 10
CONST TURN_SECONDS = 10
CONST START_FUEL = 400
CONST MAX_TURNS = 100
CONST PERFECT_SPEED = 1
CONST GOOD_SPEED = 3
CONST HARD_SPEED = 6
CONST BAD_NUMBER = -999999999

DIM gEof AS INTEGER = 0

TYPE Craft
  altitude AS DOUBLE = 0
  speed AS DOUBLE = 0
  fuel AS DOUBLE = 0
  clock AS DOUBLE = 0
  landed AS DOUBLE = 0
  impact AS DOUBLE = 0
END TYPE

REM ---------------------------------------------------------------------
REM Pure physics and rules
REM ---------------------------------------------------------------------

REM Flies one stretch of constant burn. Returns LIST(altitude, speed,
REM seconds flown). If the ground is reached early the altitude is 0, the
REM speed is the touchdown speed and fewer than dt seconds were flown.
FUNCTION FlightStep(PARAMETER altitude, PARAMETER speed, PARAMETER rate, PARAMETER dt) AS LIST
  LOCAL accel
  LOCAL newAltitude
  LOCAL t
  LOCAL discriminant
  accel = GRAVITY - ACCEL_PER_RATE * rate
  newAltitude = altitude - speed * dt - 0.5 * accel * dt * dt
  IF newAltitude > 0 THEN
    FlightStep = LIST(newAltitude, speed + accel * dt, dt)
    EXIT FUNCTION
  END IF
  IF ABS(accel) < 0.000000001 THEN
    t = altitude / speed
  ELSE
    discriminant = MAX(0, speed * speed + 2 * accel * altitude)
    t = (SQRT(discriminant) - speed) / accel
  END IF
  FlightStep = LIST(0, speed + accel * t, t)
FEND

REM The largest burn rate allowed: the engine limit, or what the remaining
REM fuel can sustain for a whole turn.
FUNCTION MaxBurn(PARAMETER fuel)
  MaxBurn = MAX(0, MIN(MAX_BURN, fuel / TURN_SECONDS))
FEND

REM Returns "" when the burn rate is allowed, otherwise the reason it is not.
FUNCTION BurnError$(PARAMETER rate, PARAMETER fuel) AS STRING
  BurnError$ = ""
  IF rate < 0 THEN
    BurnError$ = "The engine cannot burn a negative amount."
  ELSEIF rate > MAX_BURN THEN
    BurnError$ = "The engine cannot burn more than " + STR$(MAX_BURN) + " kg/s."
  ELSEIF rate > MaxBurn(fuel) THEN
    BurnError$ = "You only have " + STR$(ROUND(fuel, 1)) + " kg of fuel - at most " + STR$(ROUND(MaxBurn(fuel), 2)) + " kg/s for a whole turn."
  END IF
FEND

REM Starting state from two rolls (0 <= roll < 1): 1200-2000 m up, falling
REM at 20-45 m/s, with a full tank.
FUNCTION NewCraft(PARAMETER altitudeRoll, PARAMETER speedRoll) AS Craft
  LOCAL c AS Craft
  c = NEW Craft()
  c.altitude = 1200 + 100 * INT(altitudeRoll * 9)
  c.speed = 20 + INT(speedRoll * 26)
  c.fuel = START_FUEL
  NewCraft = c
FEND

REM Advances the craft by one turn at the given burn rate (limited by the
REM fuel left). Returns 1 when the craft has touched down.
FUNCTION AdvanceTurn(PARAMETER c AS Craft, PARAMETER rate)
  LOCAL burn
  LOCAL result AS LIST
  burn = MAX(0, MIN(rate, MaxBurn(c.fuel)))
  result = FlightStep(c.altitude, c.speed, burn, TURN_SECONDS)
  c.altitude = result[1]
  c.speed = result[2]
  c.clock = c.clock + result[3]
  c.fuel = MAX(0, c.fuel - burn * result[3])
  IF c.altitude <= 0 THEN
    c.landed = 1
    c.impact = c.speed
  END IF
  AdvanceTurn = c.landed
FEND

REM Landing quality from the touchdown speed: 1 perfect, 2 good, 3 hard, 4 crash.
FUNCTION LandingTier(PARAMETER speed)
  IF speed <= PERFECT_SPEED THEN
    LandingTier = 1
  ELSEIF speed <= GOOD_SPEED THEN
    LandingTier = 2
  ELSEIF speed <= HARD_SPEED THEN
    LandingTier = 3
  ELSE
    LandingTier = 4
  END IF
FEND

FUNCTION LandingText$(PARAMETER tier) AS STRING
  SELECT CASE tier
    CASE 1
      LandingText$ = "Perfect landing! The module settles on the dust like a feather."
    CASE 2
      LandingText$ = "Good landing. A firm touchdown, and everyone walks away."
    CASE 3
      LandingText$ = "Hard landing. The legs buckle and the module is wrecked, but the crew survives."
    CASE ELSE
      LandingText$ = "CRASH. The module hits the surface far too fast and digs a new crater."
  END SELECT
FEND

REM ---------------------------------------------------------------------
REM Input and output
REM ---------------------------------------------------------------------

REM Reads one line. At the end of the input it sets gEof and returns "".
FUNCTION ReadLine$(PARAMETER prompt$ AS STRING) AS STRING
  LOCAL line$ AS STRING
  ReadLine$ = ""
  IF gEof THEN EXIT FUNCTION
  line$ = ""
  PRINT prompt$;
  TRY
    INPUT line$
  CATCH problem$
    gEof = 1
    line$ = ""
  END TRY
  ReadLine$ = line$
FEND

REM A finite number from text, or BAD_NUMBER.
FUNCTION ParseNumber(PARAMETER text$ AS STRING)
  ParseNumber = BAD_NUMBER
  TRY
    ParseNumber = VAL(text$)
  CATCH problem$
    ParseNumber = BAD_NUMBER
  END TRY
FEND

REM Seeds the random generator from the first program argument (a number)
REM or from the clock, and returns the seed in use.
FUNCTION SeedFromArguments()
  LOCAL seed
  seed = UNIXTIME()
  IF ARGC() >= 1 THEN
    TRY
      seed = VAL(ARG$(1))
    CATCH problem$
      PRINT "(Ignoring seed "; ARG$(1); ": it is not a number.)"
    END TRY
  END IF
  RANDOMIZE seed
  SeedFromArguments = seed
FEND

REM Asks a yes/no question; anything but yes (or end of input) is no.
FUNCTION AskYesNo(PARAMETER prompt$ AS STRING)
  LOCAL answer$ AS STRING
  LOCAL tries
  AskYesNo = 0
  FOR tries = 1 TO 5
    answer$ = UCASE$(TRIM$(ReadLine$(prompt$)))
    IF gEof THEN EXIT FUNCTION
    IF answer$ = "Y" OR answer$ = "YES" THEN
      AskYesNo = 1
      EXIT FUNCTION
    END IF
    IF answer$ = "N" OR answer$ = "NO" THEN EXIT FUNCTION
    PRINT "Please answer Y or N."
  NEXT
FEND

FUNCTION AskBurn(PARAMETER c AS Craft)
  LOCAL rate
  LOCAL msg$ AS STRING
  AskBurn = 0
  DO
    rate = ParseNumber(ReadLine$("Burn rate in kg/s (0 to " + STR$(ROUND(MaxBurn(c.fuel), 2)) + ")> "))
    IF gEof THEN EXIT FUNCTION
    IF rate = BAD_NUMBER THEN
      msg$ = "Please enter a number."
    ELSE
      msg$ = BurnError$(rate, c.fuel)
    END IF
    IF msg$ <> "" THEN PRINT msg$
  LOOP UNTIL msg$ = ""
  AskBurn = rate
FEND

FUNCTION ShowStatus(PARAMETER c AS Craft)
  PRINT
  PRINT "Time "; c.clock; " s   Altitude "; ROUND(c.altitude, 1); " m   Fuel "; ROUND(c.fuel, 1); " kg"
  IF c.speed >= 0 THEN
    PRINT "Descending at "; ROUND(c.speed, 1); " m/s"
  ELSE
    PRINT "Rising at "; ROUND(-c.speed, 1); " m/s"
  END IF
FEND

FUNCTION ShowLanding(PARAMETER c AS Craft)
  PRINT
  PRINT "Touchdown after "; ROUND(c.clock, 1); " s at "; ROUND(c.impact, 2); " m/s with "; ROUND(c.fuel, 1); " kg of fuel left."
  PRINT LandingText$(LandingTier(c.impact))
FEND

REM Flies one landing. Returns the landing tier (1-4), or 0 if the game was
REM abandoned (end of input or too many turns).
FUNCTION PlayRound()
  LOCAL c AS Craft
  LOCAL rate
  LOCAL turn
  c = NewCraft(RND(), RND())
  PlayRound = 0
  PRINT
  PRINT "Gravity pulls at "; GRAVITY; " m/s^2. Each kg/s of burn pushes back with "; ACCEL_PER_RATE; " m/s^2."
  PRINT "A burn of "; GRAVITY / ACCEL_PER_RATE; " kg/s hovers. Land at "; GOOD_SPEED; " m/s or less to walk away."
  turn = 0
  DO WHILE c.landed = 0 AND turn < MAX_TURNS
    turn = turn + 1
    CALL ShowStatus(c)
    IF c.fuel > 0 THEN
      rate = AskBurn(c)
      IF gEof THEN EXIT DO
    ELSE
      rate = 0
      PRINT "Out of fuel - the engine is dead and the module is in free fall."
    END IF
    CALL AdvanceTurn(c, rate)
  LOOP
  IF c.landed THEN
    CALL ShowLanding(c)
    PlayRound = LandingTier(c.impact)
  ELSEIF gEof THEN
    PRINT
    PRINT "The input ended; the mission is abandoned."
  ELSE
    PRINT
    PRINT "Mission control calls it off after "; MAX_TURNS; " turns."
  END IF
FEND

REM ---------------------------------------------------------------------
REM Unit tests: basic-vm --test examples\games\lander.bas
REM ---------------------------------------------------------------------

TEST FlightStep_free_fall
  DIM r AS LIST = FlightStep(1000, 0, 0, 10)
  ASSERTNEAR 919, r[1], 0.000001, "1000 - 0.5 * 1.62 * 100"
  ASSERTNEAR 16.2, r[2], 0.000001
  ASSERTEQ 10, r[3]
END TEST

TEST FlightStep_hover_keeps_the_speed
  DIM r AS LIST = FlightStep(500, 5, 3.24, 10)
  ASSERTNEAR 450, r[1], 0.000001
  ASSERTNEAR 5, r[2], 0.000001
END TEST

TEST FlightStep_full_burn_brakes_and_rising_craft_gain_height
  DIM r AS LIST = FlightStep(1000, 40, MAX_BURN, 10)
  ASSERTNEAR 769, r[1], 0.000001, "net deceleration 3.38 m/s^2"
  ASSERTNEAR 6.2, r[2], 0.000001
  DIM s AS LIST = FlightStep(100, -5, 3.24, 10)
  ASSERTNEAR 150, s[1], 0.000001
  ASSERTNEAR -5, s[2], 0.000001
END TEST

TEST FlightStep_touchdown_part_way_through_a_turn
  DIM r AS LIST = FlightStep(10, 10, 0, 10)
  ASSERTEQ 0, r[1]
  ASSERT r[3] > 0 AND r[3] < 10, "ground reached before the turn ends"
  ASSERTNEAR 10, 10 * r[3] + 0.81 * r[3] * r[3], 0.000000001, "the fall covers exactly 10 m"
  ASSERTNEAR 10 + 1.62 * r[3], r[2], 0.000000001, "touchdown speed"
  DIM h AS LIST = FlightStep(20, 4, 3.24, 10)
  ASSERTNEAR 5, h[3], 0.000001, "no acceleration: 20 m at 4 m/s"
  ASSERTNEAR 4, h[2], 0.000001
END TEST

TEST MaxBurn_and_BurnError
  ASSERTEQ 10, MaxBurn(400)
  ASSERTNEAR 2.5, MaxBurn(25), 0.000001
  ASSERTEQ 0, MaxBurn(0)
  ASSERTEQ "", BurnError$(0, 400)
  ASSERTEQ "", BurnError$(10, 400)
  ASSERT BurnError$(-1, 400) <> ""
  ASSERT INSTR(BurnError$(11, 400), "more than 10 kg/s") > 0, "engine limit"
  ASSERT INSTR(BurnError$(4, 30), "only have 30 kg of fuel") > 0, "fuel limit"
  ASSERTEQ "", BurnError$(3, 30)
END TEST

TEST NewCraft_start_values
  DIM low AS Craft = NewCraft(0, 0)
  DIM high AS Craft = NewCraft(0.9999, 0.9999)
  ASSERTEQ 1200, low.altitude
  ASSERTEQ 20, low.speed
  ASSERTEQ 2000, high.altitude
  ASSERTEQ 45, high.speed
  ASSERTEQ START_FUEL, high.fuel
  ASSERTEQ 0, high.landed
END TEST

TEST AdvanceTurn_uses_fuel_and_time
  DIM c AS Craft = NewCraft(0, 0)
  DIM landed AS INTEGER
  landed = AdvanceTurn(c, 4)
  ASSERTEQ 0, landed
  ASSERTNEAR 360, c.fuel, 0.000001, "4 kg/s for 10 s"
  ASSERTEQ 10, c.clock
  ASSERTNEAR 1200 - 200 - 0.5 * (1.62 - 2) * 100, c.altitude, 0.000001
  ASSERTNEAR 20 + (1.62 - 2) * 10, c.speed, 0.000001
END TEST

TEST AdvanceTurn_with_no_fuel_is_free_fall
  DIM c AS Craft = NewCraft(0, 0)
  DIM free AS Craft = NewCraft(0, 0)
  DIM landed AS INTEGER
  c.fuel = 0
  landed = AdvanceTurn(c, 10)
  landed = AdvanceTurn(free, 0)
  ASSERTEQ 0, c.fuel
  ASSERTNEAR free.altitude, c.altitude, 0.000001, "a burn without fuel changes nothing"
  ASSERTNEAR free.speed, c.speed, 0.000001
  c.fuel = 15
  landed = AdvanceTurn(c, 10)
  ASSERTNEAR 0, c.fuel, 0.000001, "the burn is capped at what the tank holds"
END TEST

TEST AdvanceTurn_a_planned_burn_schedule_lands_softly
  DIM c AS Craft = NewCraft(0.5, 0.48)
  DIM plan AS LIST = LIST(0, 0, 10, 10, 1.8, 4)
  DIM landed AS INTEGER = 0
  DIM rate AS DOUBLE
  ASSERTEQ 1600, c.altitude
  ASSERTEQ 32, c.speed
  FOR EACH rate IN plan
    landed = AdvanceTurn(c, rate)
  NEXT
  ASSERTEQ 1, landed
  ASSERTEQ 1, LandingTier(c.impact), "perfect landing"
  ASSERTNEAR 0.89, c.impact, 0.01
  ASSERTNEAR 149.3, c.fuel, 0.1
END TEST

TEST AdvanceTurn_zero_burn_crashes
  DIM crash AS Craft = NewCraft(0, 0)
  DIM landed AS INTEGER = 0
  DIM turns AS INTEGER = 0
  DO WHILE landed = 0 AND turns < 100
    landed = AdvanceTurn(crash, 0)
    turns = turns + 1
  LOOP
  ASSERTEQ 1, landed
  ASSERTEQ 4, LandingTier(crash.impact), "free fall from 1200 m is a crash"
  ASSERT crash.impact > 50, "impact speed"
  ASSERTEQ START_FUEL, crash.fuel
END TEST

TEST LandingTier_boundaries
  ASSERTEQ 1, LandingTier(0)
  ASSERTEQ 1, LandingTier(1)
  ASSERTEQ 2, LandingTier(1.01)
  ASSERTEQ 2, LandingTier(3)
  ASSERTEQ 3, LandingTier(3.01)
  ASSERTEQ 3, LandingTier(6)
  ASSERTEQ 4, LandingTier(6.01)
  ASSERT INSTR(LandingText$(1), "Perfect") > 0
  ASSERT INSTR(LandingText$(4), "CRASH") > 0
END TEST

TEST ParseNumber_and_end_of_input
  ASSERTEQ 3.5, ParseNumber(" 3.5 ")
  ASSERTEQ 0, ParseNumber("0")
  ASSERTEQ BAD_NUMBER, ParseNumber("fast")
  ASSERTEQ BAD_NUMBER, ParseNumber("")
  ASSERTEQ "", ReadLine$("> ")
  ASSERTEQ 1, gEof
END TEST

REM ---------------------------------------------------------------------
REM Main program
REM ---------------------------------------------------------------------

DIM gSeed
DIM gRounds
DIM gOutcome
gSeed = SeedFromArguments()
IF ARGC() = 0 THEN PRINT "(Seed "; gSeed; " - pass it as an argument to replay this game.)"
PRINT "LUNAR LANDER"
gRounds = 0
DO
  gRounds = gRounds + 1
  gOutcome = PlayRound()
  IF gEof THEN EXIT DO
LOOP UNTIL AskYesNo("Fly again? (y/n)> ") = 0 OR gRounds >= 50
PRINT
PRINT "Thanks for flying. Rounds played: "; gRounds
