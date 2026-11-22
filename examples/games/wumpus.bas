REM ======================================================================
REM HUNT THE WUMPUS - stalk a wumpus through a cave of twenty rooms.
REM The cave is a dodecahedron: every room has three tunnels. Two rooms
REM hold bottomless pits, two hold giant bats that carry you off, and one
REM holds the wumpus. You can sense hazards in the next rooms. Walk with
REM M or shoot crooked arrows with S - five arrows, up to five rooms each.
REM
REM Original game concept: Hunt the Wumpus, commonly attributed to
REM Gregory Yob (1973); this is an independent implementation, not a copy
REM of the original listing.
REM
REM Play:        basic-vm examples\games\wumpus.bas [seed] [reveal]
REM Run tests:   basic-vm --test examples\games\wumpus.bas
REM
REM Arguments (any order): a number seeds the random generator (otherwise
REM the clock is used and the seed is printed); "reveal" is a debugging aid
REM that prints where every hazard is before each move.
REM
REM Room numbers: 1-5 form the top ring, 6-10 hang below them, 11-15 are the
REM lower middle ring and 16-20 the bottom ring. The game ends gracefully
REM when the input ends.
REM ======================================================================
OPTION EXPLICIT

CONST ROOMS = 20
CONST MAX_ARROWS = 5
CONST MAX_PATH = 5
CONST WAKE_MOVE_CHANCE = 0.75
CONST BAT_TRIP_LIMIT = 10

DIM gEof AS INTEGER = 0
DIM gReveal AS INTEGER = 0
DIM gSeed AS DOUBLE = 0
DIM gSeedGiven AS INTEGER = 0
DIM gRolls AS LIST = LIST()

TYPE Hunt
  player AS DOUBLE = 1
  wumpus AS DOUBLE = 2
  arrows AS DOUBLE = 5
  pits AS LIST
  bats AS LIST
  events AS LIST
  status AS DOUBLE = 0
END TYPE

REM ---------------------------------------------------------------------
REM The cave
REM ---------------------------------------------------------------------

REM Room number of position i (any whole number, wrapping) in the ring of
REM five rooms that starts at room "base".
FUNCTION RingRoom(PARAMETER base, PARAMETER i)
  RingRoom = base + ((i + 5) MOD 5)
FEND

REM Adjacency lists for the dodecahedron: element r is the ascending list
REM of the three rooms connected to room r.
FUNCTION BuildCave() AS LIST
  LOCAL cave AS LIST
  LOCAL neighbours AS LIST
  LOCAL room
  LOCAL ring
  LOCAL i
  cave = LIST()
  FOR room = 1 TO ROOMS
    ring = INT((room - 1) / 5)
    i = (room - 1) MOD 5
    SELECT CASE ring
      CASE 0
        neighbours = LIST(RingRoom(1, i - 1), RingRoom(1, i + 1), RingRoom(6, i))
      CASE 1
        neighbours = LIST(RingRoom(1, i), RingRoom(11, i), RingRoom(11, i - 1))
      CASE 2
        neighbours = LIST(RingRoom(6, i), RingRoom(6, i + 1), RingRoom(16, i))
      CASE ELSE
        neighbours = LIST(RingRoom(16, i - 1), RingRoom(16, i + 1), RingRoom(11, i))
    END SELECT
    SORT neighbours
    APPEND cave, neighbours
  NEXT
  BuildCave = cave
FEND

DIM gCave AS LIST = BuildCave()

REM ---------------------------------------------------------------------
REM Randomness. Tests can queue rolls in gRolls; they are used first, in
REM order. Otherwise RND() supplies them. Rolls are 0 <= roll < 1.
REM ---------------------------------------------------------------------

FUNCTION NextRoll()
  IF LEN(gRolls) > 0 THEN
    NextRoll = gRolls[1]
    REMOVE gRolls, 1
  ELSE
    NextRoll = RND()
  END IF
FEND

REM Uniformly picks a room number 1..ROOMS.
FUNCTION RandomRoom()
  RandomRoom = MIN(ROOMS, 1 + INT(NextRoll() * ROOMS))
FEND

REM Removes the first occurrence of value from the list, if it is there.
FUNCTION RemoveValue(PARAMETER items AS LIST, PARAMETER value)
  LOCAL i
  FOR i = 1 TO LEN(items)
    IF items[i] = value THEN
      REMOVE items, i
      EXIT FOR
    END IF
  NEXT
FEND

REM Removes and returns the element of the list chosen by the roll.
FUNCTION TakeChoice(PARAMETER items AS LIST, PARAMETER roll)
  LOCAL index
  index = MIN(LEN(items), 1 + INT(roll * LEN(items)))
  TakeChoice = items[index]
  REMOVE items, index
FEND

REM Chooses distinct rooms for LIST(player, wumpus, pit, pit, bat, bat).
REM Hazards never share the player's room or sit next to it.
FUNCTION PlaceEntities() AS LIST
  LOCAL available AS LIST
  LOCAL placed AS LIST
  LOCAL room
  LOCAL k
  available = LIST()
  FOR room = 1 TO ROOMS
    APPEND available, room
  NEXT
  placed = LIST()
  APPEND placed, TakeChoice(available, NextRoll())
  FOR EACH room IN gCave[placed[1]]
    CALL RemoveValue(available, room)
  NEXT
  FOR k = 1 TO 5
    APPEND placed, TakeChoice(available, NextRoll())
  NEXT
  PlaceEntities = placed
FEND

REM ---------------------------------------------------------------------
REM Game rules (no input or output; messages go into the hunt's event list)
REM ---------------------------------------------------------------------

FUNCTION MakeHunt(PARAMETER player, PARAMETER wumpus, PARAMETER pit1, PARAMETER pit2, PARAMETER bat1, PARAMETER bat2) AS Hunt
  LOCAL h AS Hunt
  h = NEW Hunt()
  h.player = player
  h.wumpus = wumpus
  h.arrows = MAX_ARROWS
  h.pits = LIST(pit1, pit2)
  h.bats = LIST(bat1, bat2)
  h.events = LIST()
  h.status = 0
  MakeHunt = h
FEND

FUNCTION NewHunt(PARAMETER placed AS LIST) AS Hunt
  NewHunt = MakeHunt(placed[1], placed[2], placed[3], placed[4], placed[5], placed[6])
FEND

FUNCTION AddEvent(PARAMETER h AS Hunt, PARAMETER text$ AS STRING)
  LOCAL events AS LIST
  events = h.events
  APPEND events, text$
FEND

REM Status 0 = hunting, 1 = the wumpus is dead, 2 = the hunter is dead.
FUNCTION LoseHunt(PARAMETER h AS Hunt, PARAMETER text$ AS STRING)
  CALL AddEvent(h, text$)
  h.status = 2
FEND

REM The wumpus wakes. With probability WAKE_MOVE_CHANCE it wanders into a
REM neighbouring room (chosen by the roll); otherwise it stays put. It eats
REM the hunter if it ends up in the hunter's room.
FUNCTION WakeWumpus(PARAMETER h AS Hunt, PARAMETER roll)
  LOCAL exits AS LIST
  LOCAL moved
  moved = 0
  IF roll < WAKE_MOVE_CHANCE THEN
    exits = gCave[h.wumpus]
    h.wumpus = exits[1 + INT(roll / (WAKE_MOVE_CHANCE / 3))]
    moved = 1
  END IF
  IF h.wumpus = h.player THEN
    IF moved THEN
      CALL LoseHunt(h, "The wumpus lumbers into your room and swallows you whole.")
    ELSE
      CALL LoseHunt(h, "The wumpus opens one eye, sees you, and swallows you whole.")
    END IF
  ELSEIF moved THEN
    CALL AddEvent(h, "You hear the wumpus lumber away through the tunnels.")
  ELSE
    CALL AddEvent(h, "The wumpus stirs, grunts, and goes back to sleep.")
  END IF
FEND

REM The hunter arrives in a room: the wumpus may wake, pits are fatal and
REM bats carry the hunter to another room (which is then entered in turn).
FUNCTION EnterRoom(PARAMETER h AS Hunt, PARAMETER room)
  LOCAL trips
  LOCAL again
  h.player = room
  trips = 0
  DO
    again = 0
    IF h.player = h.wumpus THEN
      CALL AddEvent(h, "You have stumbled into the wumpus's den!")
      CALL WakeWumpus(h, NextRoll())
      IF h.status <> 0 THEN EXIT FUNCTION
    END IF
    IF CONTAINS(h.pits, h.player) THEN
      CALL LoseHunt(h, "The floor vanishes - you tumble into a bottomless pit.")
      EXIT FUNCTION
    END IF
    IF CONTAINS(h.bats, h.player) ANDALSO trips < BAT_TRIP_LIMIT THEN
      trips = trips + 1
      h.player = RandomRoom()
      CALL AddEvent(h, "Giant bats seize you and drop you in room " + STR$(h.player) + ".")
      again = 1
    END IF
  LOOP WHILE again
FEND

REM Walks to an adjacent room. Returns "" or the reason the move is refused.
FUNCTION MovePlayer$(PARAMETER h AS Hunt, PARAMETER room) AS STRING
  MovePlayer$ = ""
  IF NOT CONTAINS(gCave[h.player], room) THEN
    MovePlayer$ = "Room " + STR$(room) + " is not connected to room " + STR$(h.player) + "."
    EXIT FUNCTION
  END IF
  CALL EnterRoom(h, room)
FEND

REM Fires one arrow along the route the hunter named. At every step, if the
REM next named room is not adjacent to the arrow's room, the arrow flies into
REM a random adjacent room instead. It can kill the wumpus - or the hunter.
FUNCTION ShootArrow(PARAMETER h AS Hunt, PARAMETER route AS LIST)
  LOCAL here
  LOCAL i
  LOCAL target
  h.arrows = h.arrows - 1
  here = h.player
  FOR i = 1 TO LEN(route)
    target = route[i]
    IF CONTAINS(gCave[here], target) THEN
      here = target
    ELSE
      here = gCave[here][MIN(3, 1 + INT(NextRoll() * 3))]
      CALL AddEvent(h, "The arrow veers off course in the dark tunnels.")
    END IF
    IF here = h.wumpus THEN
      CALL AddEvent(h, "Thwack! Your arrow finds the wumpus in room " + STR$(here) + ".")
      h.status = 1
      EXIT FUNCTION
    END IF
    IF here = h.player THEN
      CALL LoseHunt(h, "Your arrow curves back into your own room and strikes you down.")
      EXIT FUNCTION
    END IF
  NEXT
  CALL AddEvent(h, "Your arrow clatters to a stop and is lost.")
  CALL WakeWumpus(h, NextRoll())
  IF h.status = 0 ANDALSO h.arrows = 0 THEN
    CALL LoseHunt(h, "You have no arrows left, and the wumpus is still out there.")
  END IF
FEND

REM Sensory warnings about hazards in the rooms next to the hunter.
FUNCTION Warnings(PARAMETER h AS Hunt) AS LIST
  LOCAL notes AS LIST
  LOCAL near
  LOCAL smell
  LOCAL draft
  LOCAL wings
  notes = LIST()
  smell = 0
  draft = 0
  wings = 0
  FOR EACH near IN gCave[h.player]
    IF near = h.wumpus THEN smell = 1
    IF CONTAINS(h.pits, near) THEN draft = 1
    IF CONTAINS(h.bats, near) THEN wings = 1
  NEXT
  IF smell THEN APPEND notes, "A foul stench hangs in the air - the wumpus is close."
  IF draft THEN APPEND notes, "A cold draught blows from somewhere near: a pit."
  IF wings THEN APPEND notes, "You hear leathery wings rustling nearby: bats."
  Warnings = notes
FEND

REM ---------------------------------------------------------------------
REM Commands
REM ---------------------------------------------------------------------

REM Commas become spaces; runs of spaces collapse; the ends are trimmed.
FUNCTION Squeeze$(PARAMETER text$ AS STRING) AS STRING
  LOCAL t$ AS STRING
  t$ = REPLACE$(text$, ",", " ")
  DO WHILE INSTR(t$, "  ") > 0
    t$ = REPLACE$(t$, "  ", " ")
  LOOP
  Squeeze$ = TRIM$(t$)
FEND

REM A room number 1..ROOMS written in digits, or 0 when text is not one.
FUNCTION ParseRoom(PARAMETER text$ AS STRING)
  LOCAL i
  ParseRoom = 0
  IF LEN(text$) = 0 ORELSE LEN(text$) > 2 THEN EXIT FUNCTION
  FOR i = 1 TO LEN(text$)
    IF INSTR("0123456789", MID$(text$, i, 1)) = 0 THEN EXIT FUNCTION
  NEXT
  IF VAL(text$) >= 1 ANDALSO VAL(text$) <= ROOMS THEN ParseRoom = VAL(text$)
FEND

FUNCTION MakeCommand(PARAMETER kind$ AS STRING, PARAMETER roomList AS LIST, PARAMETER message$ AS STRING) AS MAP
  MakeCommand = MAP("kind", kind$, "rooms", roomList, "message", message$)
FEND

REM Turns a typed line into a command map with the keys "kind" (move, shoot,
REM help, quit or error), "rooms" (a list) and "message" (for errors).
FUNCTION ParseCommand(PARAMETER line$ AS STRING) AS MAP
  LOCAL parts AS LIST
  LOCAL roomList AS LIST
  LOCAL word$ AS STRING
  LOCAL kind$ AS STRING
  LOCAL first
  LOCAL i
  LOCAL room
  parts = SPLIT(UCASE$(Squeeze$(line$)), " ")
  word$ = parts[1]
  IF word$ = "" THEN
    ParseCommand = MakeCommand("error", LIST(), "Type M and a room to walk, S and up to 5 rooms to shoot, H for help.")
    EXIT FUNCTION
  END IF
  first = 2
  SELECT CASE word$
    CASE "M", "MOVE", "GO", "W", "WALK"
      kind$ = "move"
    CASE "S", "SHOOT", "F", "FIRE"
      kind$ = "shoot"
    CASE "H", "HELP", "?"
      ParseCommand = MakeCommand("help", LIST(), "")
      EXIT FUNCTION
    CASE "Q", "QUIT", "EXIT"
      ParseCommand = MakeCommand("quit", LIST(), "")
      EXIT FUNCTION
    CASE ELSE
      IF ParseRoom(word$) > 0 THEN
        kind$ = "move"
        first = 1
      ELSE
        ParseCommand = MakeCommand("error", LIST(), "I do not understand '" + word$ + "'. Type H for help.")
        EXIT FUNCTION
      END IF
  END SELECT
  roomList = LIST()
  FOR i = first TO LEN(parts)
    room = ParseRoom(parts[i])
    IF room = 0 THEN
      ParseCommand = MakeCommand("error", LIST(), "'" + parts[i] + "' is not a room number from 1 to " + STR$(ROOMS) + ".")
      EXIT FUNCTION
    END IF
    APPEND roomList, room
  NEXT
  IF LEN(roomList) = 0 THEN
    ParseCommand = MakeCommand("error", LIST(), "Which room? Give a room number after the command.")
  ELSEIF kind$ = "move" ANDALSO LEN(roomList) > 1 THEN
    ParseCommand = MakeCommand("error", LIST(), "You can only walk to one room at a time.")
  ELSEIF LEN(roomList) > MAX_PATH THEN
    ParseCommand = MakeCommand("error", LIST(), "An arrow can only travel through " + STR$(MAX_PATH) + " rooms.")
  ELSE
    ParseCommand = MakeCommand(kind$, roomList, "")
  END IF
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

REM Reads the program arguments: a number is the seed, "reveal" turns on the
REM debugging aid. Seeds the random generator.
FUNCTION ReadArguments()
  LOCAL i
  LOCAL word$ AS STRING
  gSeed = UNIXTIME()
  FOR i = 1 TO ARGC()
    word$ = LCASE$(ARG$(i))
    IF word$ = "reveal" THEN
      gReveal = 1
    ELSE
      TRY
        gSeed = VAL(word$)
        gSeedGiven = 1
      CATCH problem$
        PRINT "(Ignoring argument '"; ARG$(i); "': it is neither a seed nor 'reveal'.)"
      END TRY
    END IF
  NEXT
  RANDOMIZE gSeed
FEND

FUNCTION JoinRooms$(PARAMETER roomList AS LIST) AS STRING
  LOCAL text$ AS STRING
  LOCAL i
  text$ = ""
  FOR i = 1 TO LEN(roomList)
    IF i > 1 THEN text$ = text$ + ", "
    text$ = text$ + STR$(roomList[i])
  NEXT
  JoinRooms$ = text$
FEND

FUNCTION ShowHelp()
  PRINT "M <room>         walk to a connected room (a bare room number works too)"
  PRINT "S <rooms...>     shoot an arrow through up to "; MAX_PATH; " rooms, e.g. S 4 9 14"
  PRINT "                 if the next room is not connected, the arrow flies at random"
  PRINT "H                this help"
  PRINT "Q                give up the hunt"
  PRINT "Pits kill you, bats carry you elsewhere, and the wumpus wakes if you enter"
  PRINT "its room or miss it with an arrow. Hit it with an arrow to win."
FEND

FUNCTION ShowIntro()
  PRINT "HUNT THE WUMPUS"
  PRINT "Twenty rooms, three tunnels each. Two pits, two bat colonies, one wumpus."
  PRINT "You carry "; MAX_ARROWS; " arrows. Type H for help."
FEND

FUNCTION Describe(PARAMETER h AS Hunt)
  LOCAL note$ AS STRING
  LOCAL pits AS LIST
  LOCAL bats AS LIST
  pits = h.pits
  bats = h.bats
  PRINT
  PRINT "You are in room "; h.player; ". Arrows left: "; h.arrows; "."
  PRINT "Tunnels lead to rooms "; JoinRooms$(gCave[h.player]); "."
  FOR EACH note$ IN Warnings(h)
    PRINT note$
  NEXT
  IF gReveal THEN PRINT "[reveal] wumpus="; h.wumpus; " pits="; JoinRooms$(pits); " bats="; JoinRooms$(bats)
FEND

FUNCTION ShowEvents(PARAMETER h AS Hunt)
  LOCAL note$ AS STRING
  FOR EACH note$ IN h.events
    PRINT note$
  NEXT
  h.events = LIST()
FEND

FUNCTION PlayGame()
  LOCAL h AS Hunt
  LOCAL command AS MAP
  LOCAL roomList AS LIST
  LOCAL kind$ AS STRING
  LOCAL line$ AS STRING
  LOCAL refusal$ AS STRING
  LOCAL quit
  quit = 0
  h = NewHunt(PlaceEntities())
  CALL ShowIntro()
  DO WHILE h.status = 0 AND quit = 0
    CALL Describe(h)
    line$ = ReadLine$("Command (M room, S rooms, H help, Q quit)> ")
    IF gEof THEN EXIT DO
    command = ParseCommand(line$)
    kind$ = command["kind"]
    roomList = command["rooms"]
    SELECT CASE kind$
      CASE "move"
        refusal$ = MovePlayer$(h, roomList[1])
        IF refusal$ <> "" THEN PRINT refusal$
      CASE "shoot"
        CALL ShootArrow(h, roomList)
      CASE "help"
        CALL ShowHelp()
      CASE "quit"
        quit = 1
      CASE ELSE
        PRINT command["message"]
    END SELECT
    CALL ShowEvents(h)
  LOOP
  PRINT
  IF h.status = 1 THEN
    PRINT "You win! The wumpus will terrorise no one again."
  ELSEIF h.status = 2 THEN
    PRINT "You lose. The wumpus lurked in room "; h.wumpus; "."
  ELSEIF quit THEN
    PRINT "You abandon the hunt. The wumpus lurked in room "; h.wumpus; "."
  ELSE
    PRINT "The input ended; you leave the cave."
  END IF
  PlayGame = h.status
FEND

REM ---------------------------------------------------------------------
REM Unit tests: basic-vm --test examples\games\wumpus.bas
REM ---------------------------------------------------------------------

REM Breadth-first distances from a room to every room (-1 = unreachable).
FUNCTION DistancesFrom(PARAMETER start) AS LIST
  LOCAL dist AS LIST
  LOCAL queue AS LIST
  LOCAL room
  LOCAL near
  LOCAL head
  dist = LIST()
  FOR room = 1 TO ROOMS
    APPEND dist, -1
  NEXT
  dist[start] = 0
  queue = LIST(start)
  head = 1
  DO WHILE head <= LEN(queue)
    room = queue[head]
    head = head + 1
    FOR EACH near IN gCave[room]
      IF dist[near] < 0 THEN
        dist[near] = dist[room] + 1
        APPEND queue, near
      END IF
    NEXT
  LOOP
  DistancesFrom = dist
FEND

TEST Cave_is_a_valid_3_regular_symmetric_graph
  DIM room AS INTEGER
  DIM k AS INTEGER
  DIM edges AS INTEGER = 0
  DIM near AS INTEGER
  ASSERTEQ 20, LEN(gCave)
  FOR room = 1 TO ROOMS
    ASSERTEQ 3, LEN(gCave[room]), "three tunnels"
    FOR k = 1 TO 3
      near = gCave[room][k]
      ASSERT near >= 1 AND near <= ROOMS, "room number in range"
      ASSERT near <> room, "no tunnel to itself"
      ASSERT CONTAINS(gCave[near], room), "tunnels are two-way"
      edges = edges + 1
    NEXT
    ASSERT gCave[room][1] < gCave[room][2] AND gCave[room][2] < gCave[room][3], "distinct, ascending"
  NEXT
  ASSERTEQ 60, edges, "30 tunnels counted from both ends"
END TEST

TEST Cave_is_connected_with_diameter_five
  DIM room AS INTEGER
  DIM other AS INTEGER
  DIM dist AS LIST
  DIM longest AS INTEGER = 0
  FOR room = 1 TO ROOMS
    dist = DistancesFrom(room)
    FOR other = 1 TO ROOMS
      ASSERT dist[other] >= 0, "every room is reachable"
      longest = MAX(longest, dist[other])
    NEXT
  NEXT
  ASSERTEQ 5, longest, "a dodecahedron has diameter 5"
  ASSERTEQ LIST(2, 5, 6), gCave[1]
  ASSERTEQ LIST(15, 16, 19), gCave[20], "bottom ring"
END TEST

TEST PlaceEntities_with_scripted_rolls_is_exact
  DIM placed AS LIST
  gRolls = LIST(0, 0, 0, 0, 0, 0)
  placed = PlaceEntities()
  ASSERTEQ LIST(1, 3, 4, 7, 8, 9), placed, "room 1 and its neighbours 2, 5, 6 are skipped"
  ASSERTEQ 0, LEN(gRolls), "six rolls consumed"
END TEST

TEST PlaceEntities_random_setups_are_distinct_and_reproducible
  DIM trial AS INTEGER
  DIM a AS INTEGER
  DIM b AS INTEGER
  DIM placed AS LIST
  DIM first AS LIST
  DIM second AS LIST
  RANDOMIZE 2024
  FOR trial = 1 TO 200
    placed = PlaceEntities()
    ASSERTEQ 6, LEN(placed)
    FOR a = 1 TO 6
      ASSERT placed[a] >= 1 AND placed[a] <= ROOMS, "valid room"
      FOR b = a + 1 TO 6
        ASSERT placed[a] <> placed[b], "distinct rooms"
      NEXT
      IF a > 1 THEN
        ASSERT NOT CONTAINS(gCave[placed[1]], placed[a]), "hazard next to the player"
      END IF
    NEXT
  NEXT
  RANDOMIZE 77
  first = PlaceEntities()
  RANDOMIZE 77
  second = PlaceEntities()
  ASSERTEQ first, second, "the same seed gives the same setup"
END TEST

TEST Walking_to_a_pit_or_a_wall_of_rock
  DIM h AS Hunt = MakeHunt(1, 13, 6, 17, 9, 19)
  ASSERT INSTR(MovePlayer$(h, 7), "not connected") > 0, "room 7 is not next to room 1"
  ASSERTEQ 1, h.player
  ASSERTEQ 0, h.status
  ASSERTEQ "", MovePlayer$(h, 2)
  ASSERTEQ 2, h.player
  ASSERTEQ "", MovePlayer$(h, 1)
  ASSERTEQ "", MovePlayer$(h, 6)
  ASSERTEQ 2, h.status, "fell into the pit"
  ASSERT INSTR(h.events[1], "pit") > 0
END TEST

TEST Bats_carry_the_hunter_and_can_chain
  DIM h AS Hunt = MakeHunt(1, 20, 8, 17, 6, 9)
  gRolls = LIST(0.42, 0.5)
  ASSERTEQ "", MovePlayer$(h, 6)
  ASSERTEQ 11, h.player, "bats at 6 drop you in 9, bats at 9 drop you in 11"
  ASSERTEQ 0, h.status
  ASSERTEQ 2, LEN(h.events)
  DIM g AS Hunt = MakeHunt(1, 20, 8, 17, 6, 19)
  gRolls = LIST(0.37)
  ASSERTEQ "", MovePlayer$(g, 6)
  ASSERTEQ 2, g.status, "bats drop you in room 8, a pit"
END TEST

TEST Entering_the_wumpus_room_wakes_it
  DIM eaten AS Hunt = MakeHunt(1, 2, 8, 17, 9, 19)
  DIM spared AS Hunt = MakeHunt(1, 2, 8, 17, 9, 19)
  DIM wanders AS Hunt = MakeHunt(1, 2, 8, 17, 9, 19)
  gRolls = LIST(0.9)
  ASSERTEQ "", MovePlayer$(eaten, 2)
  ASSERTEQ 2, eaten.status, "a sleepy wumpus that stays put eats you"
  gRolls = LIST(0.0)
  ASSERTEQ "", MovePlayer$(spared, 2)
  ASSERTEQ 0, spared.status
  ASSERTEQ 1, spared.wumpus, "it moved to the first neighbour, room 1"
  gRolls = LIST(0.5)
  ASSERTEQ "", MovePlayer$(wanders, 2)
  ASSERTEQ 7, wanders.wumpus, "roll 0.5 picks the third neighbour"
END TEST

TEST Arrows_straight_crooked_and_self_inflicted
  DIM hit AS Hunt = MakeHunt(1, 2, 8, 17, 9, 19)
  DIM crooked AS Hunt = MakeHunt(1, 5, 12, 17, 9, 19)
  DIM astray AS Hunt = MakeHunt(1, 5, 12, 17, 9, 19)
  DIM suicide AS Hunt = MakeHunt(1, 13, 8, 17, 9, 19)
  CALL ShootArrow(hit, LIST(2))
  ASSERTEQ 1, hit.status, "adjacent wumpus is hit"
  ASSERTEQ 4, hit.arrows
  gRolls = LIST(0.4)
  CALL ShootArrow(crooked, LIST(10))
  ASSERTEQ 1, crooked.status, "room 10 is not adjacent: a roll of 0.4 sends the arrow to room 5"
  gRolls = LIST(0.1, 0.9)
  CALL ShootArrow(astray, LIST(10))
  ASSERTEQ 0, astray.status, "a roll of 0.1 sends the arrow to room 2 instead"
  ASSERTEQ 5, astray.wumpus, "the wumpus stays asleep when the wake roll is 0.9"
  CALL ShootArrow(suicide, LIST(2, 1))
  ASSERTEQ 2, suicide.status, "the arrow comes back to room 1"
END TEST

TEST A_missed_arrow_wakes_the_wumpus_and_counts_arrows
  DIM moves AS Hunt = MakeHunt(1, 13, 8, 17, 9, 19)
  DIM eats AS Hunt = MakeHunt(1, 2, 8, 17, 9, 19)
  DIM last AS Hunt = MakeHunt(1, 13, 8, 17, 9, 19)
  gRolls = LIST(0.0)
  CALL ShootArrow(moves, LIST(2))
  ASSERTEQ 0, moves.status
  ASSERTEQ 8, moves.wumpus, "woken wumpus took the first tunnel from room 13"
  ASSERTEQ 4, moves.arrows
  gRolls = LIST(0.0)
  CALL ShootArrow(eats, LIST(5))
  ASSERTEQ 2, eats.status, "it wandered from room 2 into room 1, where you stand"
  last.arrows = 1
  gRolls = LIST(0.9)
  CALL ShootArrow(last, LIST(2))
  ASSERTEQ 2, last.status, "no arrows left"
  ASSERT INSTR(last.events[LEN(last.events)], "no arrows") > 0
END TEST

TEST Warnings_report_nearby_hazards
  DIM h AS Hunt = MakeHunt(1, 2, 6, 17, 5, 19)
  DIM quiet AS Hunt = MakeHunt(1, 13, 8, 17, 9, 19)
  ASSERTEQ 3, LEN(Warnings(h))
  ASSERT INSTR(Warnings(h)[1], "stench") > 0
  ASSERT INSTR(Warnings(h)[2], "draught") > 0
  ASSERT INSTR(Warnings(h)[3], "wings") > 0
  ASSERTEQ 0, LEN(Warnings(quiet))
END TEST

TEST ParseCommand_understands_commands_and_rejects_nonsense
  ASSERTEQ "move", ParseCommand("m 5")["kind"]
  ASSERTEQ LIST(5), ParseCommand("  M   5 ")["rooms"]
  ASSERTEQ "move", ParseCommand("17")["kind"]
  ASSERTEQ "shoot", ParseCommand("S 2, 3 4")["kind"]
  ASSERTEQ LIST(2, 3, 4), ParseCommand("s 2,3 4")["rooms"]
  ASSERTEQ "quit", ParseCommand("q")["kind"]
  ASSERTEQ "help", ParseCommand("?")["kind"]
  ASSERTEQ "error", ParseCommand("")["kind"]
  ASSERTEQ "error", ParseCommand("m")["kind"]
  ASSERTEQ "error", ParseCommand("m 21")["kind"]
  ASSERTEQ "error", ParseCommand("m 0")["kind"]
  ASSERTEQ "error", ParseCommand("m 2 3")["kind"]
  ASSERTEQ "error", ParseCommand("s 1 2 3 4 5 6")["kind"]
  ASSERTEQ "error", ParseCommand("m x")["kind"]
  ASSERTEQ "error", ParseCommand("dance")["kind"]
END TEST

TEST ReadLine_reports_end_of_input
  ASSERTEQ "", ReadLine$("> ")
  ASSERTEQ 1, gEof
END TEST

REM ---------------------------------------------------------------------
REM Main program
REM ---------------------------------------------------------------------

CALL ReadArguments()
IF gSeedGiven = 0 THEN PRINT "(Seed "; gSeed; " - pass it as an argument to replay this game.)"
CALL PlayGame()
