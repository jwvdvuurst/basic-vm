REM ======================================================================
REM HAMURABI - rule a growing city for ten years.
REM Buy and sell land, feed your people, plant grain, and survive plague,
REM rats and hunger. At the end of the term you receive a rating.
REM
REM Original game concept: "The Sumer Game" / Hamurabi, commonly attributed
REM to Doug Dyment (1968); this is an independent implementation, not a copy
REM of the original listing.
REM
REM Play:        basic-vm examples\games\hamurabi.bas [seed]
REM Run tests:   basic-vm --test examples\games\hamurabi.bas
REM
REM The optional seed (a number) makes a game reproducible. Without it the
REM clock is used and the seed is printed so the game can be replayed.
REM The game ends gracefully when the input ends.
REM ======================================================================
OPTION EXPLICIT

CONST YEARS = 10
CONST FOOD_PER_PERSON = 20
CONST ACRES_PER_FARMER = 10
CONST IMPEACH_PERCENT = 45
CONST RAT_CHANCE = 0.4
CONST PLAGUE_CHANCE = 0.15
CONST BAD_NUMBER = -2147483648

DIM gEof AS INTEGER = 0

TYPE Realm
  year AS DOUBLE = 1
  people AS DOUBLE = 100
  bushels AS DOUBLE = 2800
  acres AS DOUBLE = 1000
  price AS DOUBLE = 20
  starved AS DOUBLE = 0
  arrivals AS DOUBLE = 0
  yieldPerAcre AS DOUBLE = 0
  harvest AS DOUBLE = 0
  ratsAte AS DOUBLE = 0
  plague AS DOUBLE = 0
  totalStarved AS DOUBLE = 0
  percentSum AS DOUBLE = 0
  status AS DOUBLE = 0
END TYPE

REM ---------------------------------------------------------------------
REM Pure game logic (no input or output; randomness is passed in as rolls
REM in the range 0 <= roll < 1, so every function is deterministic).
REM ---------------------------------------------------------------------

FUNCTION NewRealm() AS Realm
  LOCAL r AS Realm
  r = NEW Realm()
  NewRealm = r
FEND

REM Land price in bushels per acre: 17 to 26.
FUNCTION PriceFromRoll(PARAMETER roll)
  PriceFromRoll = 17 + INT(roll * 10)
FEND

REM Harvest per planted acre: 1 to 6 bushels.
FUNCTION YieldFromRoll(PARAMETER roll)
  YieldFromRoll = 1 + INT(roll * 6)
FEND

REM Parses a whole number (optional leading minus, at most 9 digits).
REM Returns BAD_NUMBER for anything else.
FUNCTION ParseWhole(PARAMETER text$ AS STRING)
  LOCAL t$ AS STRING
  LOCAL i
  LOCAL c$ AS STRING
  ParseWhole = BAD_NUMBER
  t$ = TRIM$(text$)
  IF LEN(t$) = 0 THEN EXIT FUNCTION
  IF LEN(t$) > 9 THEN EXIT FUNCTION
  FOR i = 1 TO LEN(t$)
    c$ = MID$(t$, i, 1)
    IF INSTR("0123456789", c$) = 0 THEN
      IF NOT (i = 1 AND c$ = "-" AND LEN(t$) > 1) THEN EXIT FUNCTION
    END IF
  NEXT
  ParseWhole = VAL(t$)
FEND

REM Returns "" when the trade is possible, otherwise the reason it is not.
REM A positive trade buys acres, a negative one sells them.
FUNCTION TradeError$(PARAMETER trade, PARAMETER price, PARAMETER bushels, PARAMETER acres) AS STRING
  TradeError$ = ""
  IF trade > 0 ANDALSO trade * price > bushels THEN
    TradeError$ = "You only have " + STR$(bushels) + " bushels - enough for at most " + STR$(INT(bushels / price)) + " acres at " + STR$(price) + " bushels each."
  ELSEIF trade < 0 ANDALSO -trade > acres THEN
    TradeError$ = "You only own " + STR$(acres) + " acres, so you cannot sell " + STR$(-trade) + "."
  END IF
FEND

FUNCTION FeedError$(PARAMETER feed, PARAMETER bushels) AS STRING
  FeedError$ = ""
  IF feed < 0 THEN
    FeedError$ = "You cannot feed your people a negative amount."
  ELSEIF feed > bushels THEN
    FeedError$ = "You only have " + STR$(bushels) + " bushels to give out."
  END IF
FEND

REM The most acres that can be sown: land, farmers and seed grain all limit it.
FUNCTION MaxPlantable(PARAMETER acres, PARAMETER people, PARAMETER bushels)
  MaxPlantable = MAX(0, MIN(acres, people * ACRES_PER_FARMER, bushels))
FEND

FUNCTION PlantError$(PARAMETER plant, PARAMETER acres, PARAMETER people, PARAMETER bushels) AS STRING
  PlantError$ = ""
  IF plant < 0 THEN
    PlantError$ = "You cannot plant a negative number of acres."
  ELSEIF plant > acres THEN
    PlantError$ = "You only own " + STR$(acres) + " acres."
  ELSEIF plant > people * ACRES_PER_FARMER THEN
    PlantError$ = "Your " + STR$(people) + " people can only farm " + STR$(people * ACRES_PER_FARMER) + " acres."
  ELSEIF plant > bushels THEN
    PlantError$ = "You only have " + STR$(bushels) + " bushels of seed grain left."
  END IF
FEND

REM Plays out one year. Decisions: trade (acres bought, negative = sold),
REM feed (bushels), plant (acres). Rolls decide the weather and luck.
REM Returns the realm status: 0 = carry on, 1 = impeached, 2 = nobody left.
FUNCTION Resolve(PARAMETER r AS Realm, PARAMETER trade, PARAMETER feed, PARAMETER plant, PARAMETER yieldPerAcre, PARAMETER ratRoll, PARAMETER plagueRoll, PARAMETER arrivalRoll, PARAMETER priceRoll)
  LOCAL starvedPercent
  r.bushels = r.bushels - trade * r.price - feed - plant
  r.acres = r.acres + trade
  r.starved = MAX(0, r.people - INT(feed / FOOD_PER_PERSON))
  starvedPercent = r.starved * 100 / r.people
  r.percentSum = r.percentSum + starvedPercent
  r.totalStarved = r.totalStarved + r.starved
  r.people = r.people - r.starved
  r.arrivals = 0
  r.harvest = 0
  r.ratsAte = 0
  r.plague = 0
  r.yieldPerAcre = yieldPerAcre
  IF starvedPercent > IMPEACH_PERCENT THEN
    r.status = 1
    Resolve = r.status
    EXIT FUNCTION
  END IF
  r.harvest = plant * yieldPerAcre
  r.bushels = r.bushels + r.harvest
  IF ratRoll < RAT_CHANCE THEN
    r.ratsAte = ROUND(r.bushels * (0.1 + 0.5 * ratRoll))
    r.bushels = r.bushels - r.ratsAte
  END IF
  IF r.starved = 0 THEN
    r.arrivals = INT(arrivalRoll * 15) + INT(r.acres / 200)
    r.people = r.people + r.arrivals
  END IF
  IF plagueRoll < PLAGUE_CHANCE THEN
    r.plague = 1
    r.people = INT(r.people / 2)
  END IF
  r.year = r.year + 1
  r.price = PriceFromRoll(priceRoll)
  IF r.people < 1 THEN r.status = 2
  Resolve = r.status
FEND

REM Rating tier 1 (worst) to 5 (best) from the average percentage of the
REM population that starved per year and the land held per person.
FUNCTION Rating(PARAMETER averageStarvedPercent, PARAMETER acresPerPerson)
  IF averageStarvedPercent > 33 OR acresPerPerson < 7 THEN
    Rating = 1
  ELSEIF averageStarvedPercent > 10 OR acresPerPerson < 9 THEN
    Rating = 2
  ELSEIF averageStarvedPercent > 3 OR acresPerPerson < 10 THEN
    Rating = 3
  ELSEIF acresPerPerson < 12 THEN
    Rating = 4
  ELSE
    Rating = 5
  END IF
FEND

FUNCTION RatingText$(PARAMETER tier) AS STRING
  SELECT CASE tier
    CASE 1
      RatingText$ = "Disaster. The city is poorer and hungrier than you found it; history will not be kind."
    CASE 2
      RatingText$ = "Poor. Too many people suffered under your rule, and the survivors resent it."
    CASE 3
      RatingText$ = "Fair. A tolerable reign - nobody will cheer, nobody will riot."
    CASE 4
      RatingText$ = "Good. The people ate, the fields grew and the city prospered."
    CASE ELSE
      RatingText$ = "Excellent. Your name will be carved in stone: a wise and generous ruler."
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

REM Reads a whole number; BAD_NUMBER when the text is not one.
FUNCTION ReadWhole(PARAMETER prompt$ AS STRING)
  LOCAL line$ AS STRING
  line$ = ReadLine$(prompt$)
  ReadWhole = ParseWhole(line$)
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

FUNCTION AskTrade(PARAMETER r AS Realm)
  LOCAL n
  LOCAL msg$ AS STRING
  AskTrade = 0
  DO
    n = ReadWhole("Acres to trade (positive buys, negative sells, 0 for none)> ")
    IF gEof THEN EXIT FUNCTION
    IF n = BAD_NUMBER THEN
      msg$ = "Please enter a whole number."
    ELSE
      msg$ = TradeError$(n, r.price, r.bushels, r.acres)
    END IF
    IF msg$ <> "" THEN PRINT msg$
  LOOP UNTIL msg$ = ""
  AskTrade = n
FEND

FUNCTION AskFeed(PARAMETER r AS Realm, PARAMETER bushels)
  LOCAL n
  LOCAL msg$ AS STRING
  AskFeed = 0
  PRINT "Feeding everyone takes "; r.people * FOOD_PER_PERSON; " bushels ("; FOOD_PER_PERSON; " per person)."
  DO
    n = ReadWhole("Bushels to feed your people> ")
    IF gEof THEN EXIT FUNCTION
    IF n = BAD_NUMBER THEN
      msg$ = "Please enter a whole number."
    ELSE
      msg$ = FeedError$(n, bushels)
    END IF
    IF msg$ <> "" THEN PRINT msg$
  LOOP UNTIL msg$ = ""
  AskFeed = n
FEND

FUNCTION AskPlant(PARAMETER acres, PARAMETER people, PARAMETER bushels)
  LOCAL n
  LOCAL msg$ AS STRING
  AskPlant = 0
  PRINT "Each acre needs 1 bushel of seed; you can sow at most "; MaxPlantable(acres, people, bushels); " acres."
  DO
    n = ReadWhole("Acres to plant> ")
    IF gEof THEN EXIT FUNCTION
    IF n = BAD_NUMBER THEN
      msg$ = "Please enter a whole number."
    ELSE
      msg$ = PlantError$(n, acres, people, bushels)
    END IF
    IF msg$ <> "" THEN PRINT msg$
  LOOP UNTIL msg$ = ""
  AskPlant = n
FEND

FUNCTION ShowYear(PARAMETER r AS Realm)
  PRINT
  PRINT "=== Year "; r.year; " of "; YEARS; " ==="
  IF r.year > 1 THEN
    IF r.starved > 0 THEN PRINT r.starved; " people starved."
    IF r.arrivals > 0 THEN PRINT r.arrivals; " newcomers settled in the city."
    IF r.plague THEN PRINT "A plague swept through the city and half the people died."
    PRINT "The harvest gave "; r.yieldPerAcre; " bushels per acre ("; r.harvest; " in all)."
    IF r.ratsAte > 0 THEN PRINT "Rats ate "; r.ratsAte; " bushels from the granary."
  END IF
  PRINT "Population: "; r.people
  PRINT "Granary:    "; r.bushels; " bushels"
  PRINT "Land:       "; r.acres; " acres"
  PRINT "Land costs "; r.price; " bushels per acre."
FEND

FUNCTION ShowVerdict(PARAMETER r AS Realm)
  LOCAL averagePercent
  LOCAL perPerson
  PRINT
  IF r.status = 1 THEN
    PRINT "In year "; r.year; " "; r.starved; " of your people starved - more than "; IMPEACH_PERCENT; "% of the city."
    PRINT "The council throws you out of office. Your reign ends in disgrace."
  ELSEIF r.status = 2 THEN
    PRINT "Nobody is left in the city. The ruler of an empty city rules nothing."
  ELSE
    averagePercent = r.percentSum / YEARS
    perPerson = r.acres / r.people
    PRINT "Your ten years are over."
    PRINT "Average starvation per year: "; ROUND(averagePercent, 1); "% ("; r.totalStarved; " people in all)."
    PRINT "You leave "; r.people; " people and "; r.acres; " acres: "; ROUND(perPerson, 1); " acres per person."
    PRINT RatingText$(Rating(averagePercent, perPerson))
  END IF
FEND

FUNCTION PlayGame()
  LOCAL r AS Realm
  LOCAL trade
  LOCAL feed
  LOCAL plant
  LOCAL bushelsLeft
  r = NewRealm()
  r.price = PriceFromRoll(RND())
  PRINT "HAMURABI - you rule the city for "; YEARS; " years."
  PRINT "You start with "; r.people; " people, "; r.bushels; " bushels and "; r.acres; " acres."
  DO WHILE r.year <= YEARS AND r.status = 0
    CALL ShowYear(r)
    trade = AskTrade(r)
    IF gEof THEN EXIT DO
    bushelsLeft = r.bushels - trade * r.price
    feed = AskFeed(r, bushelsLeft)
    IF gEof THEN EXIT DO
    bushelsLeft = bushelsLeft - feed
    plant = AskPlant(r.acres + trade, r.people, bushelsLeft)
    IF gEof THEN EXIT DO
    CALL Resolve(r, trade, feed, plant, YieldFromRoll(RND()), RND(), RND(), RND(), RND())
  LOOP
  IF gEof THEN
    PRINT
    PRINT "The input ended; the reign is abandoned."
  ELSE
    CALL ShowVerdict(r)
  END IF
  PlayGame = r.status
FEND

REM ---------------------------------------------------------------------
REM Unit tests: basic-vm --test examples\games\hamurabi.bas
REM ---------------------------------------------------------------------

TEST Price_and_yield_rolls_stay_in_range
  ASSERTEQ 17, PriceFromRoll(0)
  ASSERTEQ 22, PriceFromRoll(0.5)
  ASSERTEQ 26, PriceFromRoll(0.9999)
  ASSERTEQ 1, YieldFromRoll(0)
  ASSERTEQ 4, YieldFromRoll(0.5)
  ASSERTEQ 6, YieldFromRoll(0.9999)
END TEST

TEST ParseWhole_accepts_whole_numbers_only
  ASSERTEQ 42, ParseWhole("42")
  ASSERTEQ -7, ParseWhole("  -7 ")
  ASSERTEQ 0, ParseWhole("0")
  ASSERTEQ BAD_NUMBER, ParseWhole("")
  ASSERTEQ BAD_NUMBER, ParseWhole("abc")
  ASSERTEQ BAD_NUMBER, ParseWhole("1.5")
  ASSERTEQ BAD_NUMBER, ParseWhole("1e3")
  ASSERTEQ BAD_NUMBER, ParseWhole("-")
  ASSERTEQ BAD_NUMBER, ParseWhole("12345678901")
END TEST

TEST TradeError_explains_what_is_missing
  ASSERTEQ "", TradeError$(10, 20, 200, 1000)
  ASSERTEQ "", TradeError$(-1000, 20, 0, 1000)
  ASSERT INSTR(TradeError$(11, 20, 200, 1000), "only have 200 bushels") > 0, "buy too much"
  ASSERT INSTR(TradeError$(-1001, 20, 200, 1000), "only own 1000 acres") > 0, "sell too much"
END TEST

TEST FeedError_and_PlantError_validate_amounts
  ASSERTEQ "", FeedError$(2000, 2000)
  ASSERT INSTR(FeedError$(2001, 2000), "only have 2000 bushels") > 0
  ASSERT FeedError$(-1, 2000) <> ""
  ASSERTEQ "", PlantError$(500, 1000, 100, 800)
  ASSERT INSTR(PlantError$(900, 1000, 100, 800), "only have 800 bushels") > 0, "seed grain"
  ASSERT INSTR(PlantError$(1001, 1000, 500, 5000), "only own 1000 acres") > 0, "land"
  ASSERT INSTR(PlantError$(600, 1000, 50, 5000), "only farm 500 acres") > 0, "farmers"
  ASSERTEQ 500, MaxPlantable(1000, 50, 5000)
END TEST

TEST Resolve_plain_year_without_trouble
  DIM r AS Realm = NewRealm()
  DIM status AS INTEGER
  status = Resolve(r, 0, 2000, 800, 3, 0.9, 0.5, 0.5, 0.5)
  ASSERTEQ 0, status
  ASSERTEQ 0, r.starved
  ASSERTEQ 12, r.arrivals
  ASSERTEQ 112, r.people
  ASSERTEQ 2400, r.bushels
  ASSERTEQ 2400, r.harvest
  ASSERTEQ 2, r.year
  ASSERTEQ 22, r.price
END TEST

TEST Resolve_buying_and_selling_land
  DIM r AS Realm = NewRealm()
  DIM q AS Realm = NewRealm()
  DIM status AS INTEGER
  status = Resolve(r, 10, 2000, 0, 1, 0.9, 0.5, 0, 0)
  ASSERTEQ 1010, r.acres
  ASSERTEQ 600, r.bushels, "2800 - 10 acres at 20 - 2000 fed"
  status = Resolve(q, -50, 2000, 0, 1, 0.9, 0.5, 0, 0)
  ASSERTEQ 950, q.acres
  ASSERTEQ 1800, q.bushels, "2800 + 50 acres at 20 - 2000 fed"
END TEST

TEST Resolve_starvation_below_the_impeachment_limit
  DIM r AS Realm = NewRealm()
  DIM status AS INTEGER
  status = Resolve(r, 0, 1200, 0, 2, 0.9, 0.5, 0.9, 0)
  ASSERTEQ 0, status
  ASSERTEQ 40, r.starved
  ASSERTEQ 0, r.arrivals, "no newcomers after a famine"
  ASSERTEQ 60, r.people
  ASSERTEQ 40, r.percentSum
END TEST

TEST Resolve_impeaches_when_more_than_45_percent_starve
  DIM r AS Realm = NewRealm()
  DIM q AS Realm = NewRealm()
  DIM status AS INTEGER
  status = Resolve(r, 0, 1000, 0, 2, 0.9, 0.5, 0.9, 0)
  ASSERTEQ 1, status, "50 percent starved"
  ASSERTEQ 50, r.starved
  ASSERTEQ 1, r.year, "the year does not advance"
  status = Resolve(q, 0, 1100, 0, 2, 0.9, 0.5, 0.9, 0)
  ASSERTEQ 0, status, "exactly 45 percent is survivable"
END TEST

TEST Resolve_rats_and_plague
  DIM r AS Realm = NewRealm()
  DIM q AS Realm = NewRealm()
  DIM status AS INTEGER
  status = Resolve(r, 0, 2000, 800, 3, 0.2, 0.1, 0, 0)
  ASSERTEQ 480, r.ratsAte, "20 percent of 2400"
  ASSERTEQ 1920, r.bushels
  ASSERTEQ 1, r.plague
  ASSERTEQ 52, r.people, "100 + 5 settlers from the land, halved"
  q.people = 1
  q.acres = 100
  status = Resolve(q, 0, 20, 0, 1, 0.9, 0, 0, 0)
  ASSERTEQ 2, status, "a plague that kills the last person"
END TEST

TEST Rating_tiers
  DIM tier AS INTEGER
  ASSERTEQ 1, Rating(40, 15)
  ASSERTEQ 1, Rating(0, 5)
  ASSERTEQ 2, Rating(20, 15)
  ASSERTEQ 2, Rating(0, 8)
  ASSERTEQ 3, Rating(5, 15)
  ASSERTEQ 3, Rating(0, 9.5)
  ASSERTEQ 4, Rating(0, 10)
  ASSERTEQ 5, Rating(0, 12)
  FOR tier = 1 TO 5
    ASSERT LEN(RatingText$(tier)) > 10
  NEXT
END TEST

TEST ReadLine_reports_end_of_input
  ASSERTEQ "", ReadLine$("> ")
  ASSERTEQ 1, gEof
  ASSERTEQ BAD_NUMBER, ReadWhole("> ")
END TEST

REM ---------------------------------------------------------------------
REM Main program
REM ---------------------------------------------------------------------

DIM gSeed
gSeed = SeedFromArguments()
IF ARGC() = 0 THEN PRINT "(Seed "; gSeed; " - pass it as an argument to replay this game.)"
CALL PlayGame()
