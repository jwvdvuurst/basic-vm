REM ======================================================================
REM HANGMAN - guess the hidden word before the drawing is finished.
REM The computer picks a word from a built-in list. Guess one letter at a
REM time (or the whole word if you dare). Six wrong guesses complete the
REM gallows picture and you lose.
REM
REM Original game concept: the traditional pencil-and-paper word game;
REM this is an independent implementation, not a copy of any original
REM listing.
REM
REM Play:        basic-vm examples\games\hangman.bas [seed]
REM Run tests:   basic-vm --test examples\games\hangman.bas
REM
REM The optional seed (a number) fixes the word choice so a game can be
REM replayed; without it the clock is used and the seed is printed. The
REM game ends gracefully when the input ends.
REM ======================================================================
OPTION EXPLICIT

CONST MAX_WRONG = 6
CONST ALPHABET$ = "ABCDEFGHIJKLMNOPQRSTUVWXYZ"
CONST WORD_TEXT$ = "ALGORITHM BICYCLE BLANKET BREEZE BUTTERFLY CABINET CANYON CARAVAN CASTLE CHIMNEY COMPASS CRICKET DOLPHIN DRAGON ECLIPSE ELEPHANT FIREPLACE GARDEN GLACIER HARBOUR HORIZON ISLAND JIGSAW KEYBOARD LANTERN LIBRARY MAGNET MOUNTAIN NOTEBOOK ORCHARD PENGUIN PYRAMID QUARTZ RAINBOW SATELLITE TELESCOPE THUNDER UMBRELLA VOLCANO WATERFALL WINDMILL ZEPPELIN"

DIM gEof AS INTEGER = 0
DIM gWords AS LIST = SPLIT(WORD_TEXT$, " ")

TYPE Round
  word$ AS STRING = ""
  tried$ AS STRING = ""
  wrong AS INTEGER = 0
  solved AS INTEGER = 0
END TYPE

REM ---------------------------------------------------------------------
REM Pure game logic
REM ---------------------------------------------------------------------

REM The word for a roll 0 <= roll < 1.
FUNCTION PickWord$(PARAMETER roll) AS STRING
  PickWord$ = gWords[1 + INT(roll * LEN(gWords))]
FEND

REM The word with unguessed letters shown as underscores, e.g. "_ A _ _ A".
FUNCTION MaskWord$(PARAMETER word$ AS STRING, PARAMETER tried$ AS STRING) AS STRING
  LOCAL i
  LOCAL letter$ AS STRING
  LOCAL shown$ AS STRING
  shown$ = ""
  FOR i = 1 TO LEN(word$)
    letter$ = MID$(word$, i, 1)
    IF i > 1 THEN shown$ = shown$ + " "
    IF INSTR(tried$, letter$) > 0 THEN
      shown$ = shown$ + letter$
    ELSE
      shown$ = shown$ + "_"
    END IF
  NEXT
  MaskWord$ = shown$
FEND

FUNCTION IsSolved(PARAMETER word$ AS STRING, PARAMETER tried$ AS STRING)
  LOCAL i
  IsSolved = 1
  FOR i = 1 TO LEN(word$)
    IF INSTR(tried$, MID$(word$, i, 1)) = 0 THEN
      IsSolved = 0
      EXIT FUNCTION
    END IF
  NEXT
FEND

REM "" when the input is an acceptable new guess, otherwise the complaint.
REM A guess is a single letter not tried before, or a whole word of letters.
FUNCTION GuessError$(PARAMETER guess$ AS STRING, PARAMETER tried$ AS STRING) AS STRING
  LOCAL i
  GuessError$ = ""
  IF guess$ = "" THEN
    GuessError$ = "Please type a letter."
    EXIT FUNCTION
  END IF
  FOR i = 1 TO LEN(guess$)
    IF INSTR(ALPHABET$, MID$(guess$, i, 1)) = 0 THEN
      GuessError$ = "Only the letters A-Z are allowed."
      EXIT FUNCTION
    END IF
  NEXT
  IF LEN(guess$) = 1 AND INSTR(tried$, guess$) > 0 THEN
    GuessError$ = "You already tried " + guess$ + "."
  END IF
FEND

FUNCTION NewRound(PARAMETER word$ AS STRING) AS Round
  LOCAL r AS Round
  r = NEW Round()
  r.word$ = word$
  NewRound = r
FEND

REM Applies a validated guess (upper case). Returns 1 for a hit, 0 for a miss.
REM A wrong whole word costs one miss; the right whole word solves the round.
FUNCTION ApplyGuess(PARAMETER r AS Round, PARAMETER guess$ AS STRING)
  ApplyGuess = 0
  IF LEN(guess$) > 1 THEN
    IF guess$ = r.word$ THEN
      r.tried$ = r.tried$ + r.word$
      r.solved = 1
      ApplyGuess = 1
    ELSE
      r.wrong = r.wrong + 1
    END IF
    EXIT FUNCTION
  END IF
  r.tried$ = r.tried$ + guess$
  IF INSTR(r.word$, guess$) > 0 THEN
    ApplyGuess = 1
    r.solved = IsSolved(r.word$, r.tried$)
  ELSE
    r.wrong = r.wrong + 1
  END IF
FEND

FUNCTION IsLost(PARAMETER r AS Round)
  IsLost = 0
  IF r.wrong >= MAX_WRONG THEN IsLost = 1
FEND

REM The gallows after `wrong` wrong guesses (0-6) as a list of text lines.
FUNCTION Gallows(PARAMETER wrong) AS LIST
  LOCAL head$ AS STRING
  LOCAL arms$ AS STRING
  LOCAL legs$ AS STRING
  head$ = "      |"
  arms$ = "      |"
  legs$ = "      |"
  IF wrong >= 1 THEN head$ = "  O   |"
  IF wrong = 2 THEN arms$ = "  |   |"
  IF wrong = 3 THEN arms$ = " /|   |"
  IF wrong >= 4 THEN arms$ = " /|\  |"
  IF wrong = 5 THEN legs$ = " /    |"
  IF wrong >= 6 THEN legs$ = " / \  |"
  Gallows = LIST("  +---+", "  |   |", head$, arms$, legs$, "      |", "=========")
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

FUNCTION ShowBoard(PARAMETER r AS Round)
  LOCAL line$ AS STRING
  PRINT
  FOR EACH line$ IN Gallows(r.wrong)
    PRINT line$
  NEXT
  PRINT "Word:  "; MaskWord$(r.word$, r.tried$)
  PRINT "Wrong guesses: "; r.wrong; " of "; MAX_WRONG
FEND

REM Plays one word. Returns 1 for a win, 0 for a loss or abandoned game.
FUNCTION PlayRound()
  LOCAL r AS Round
  LOCAL guess$ AS STRING
  LOCAL msg$ AS STRING
  LOCAL hit
  r = NewRound(PickWord$(RND()))
  PRINT
  PRINT "I have chosen a word of "; LEN(r.word$); " letters."
  PlayRound = 0
  DO WHILE r.solved = 0 AND IsLost(r) = 0
    CALL ShowBoard(r)
    guess$ = UCASE$(TRIM$(ReadLine$("Guess a letter or the word> ")))
    IF gEof THEN
      PRINT
      PRINT "The input ended; the game is abandoned. The word was "; r.word$; "."
      EXIT FUNCTION
    END IF
    msg$ = GuessError$(guess$, r.tried$)
    IF msg$ <> "" THEN
      PRINT msg$
    ELSE
      hit = ApplyGuess(r, guess$)
      IF hit THEN
        PRINT "Yes!"
      ELSE
        PRINT "No - that is wrong."
      END IF
    END IF
  LOOP
  CALL ShowBoard(r)
  IF r.solved THEN
    PRINT "You win! The word is "; r.word$; "."
    PlayRound = 1
  ELSE
    PRINT "You lose. The word was "; r.word$; "."
  END IF
FEND

REM ---------------------------------------------------------------------
REM Unit tests: basic-vm --test examples\games\hangman.bas
REM ---------------------------------------------------------------------

TEST word_list_has_enough_clean_words
  DIM w$ AS STRING
  DIM i AS INTEGER
  ASSERT LEN(gWords) >= 30, "at least 30 words"
  FOR EACH w$ IN gWords
    ASSERT LEN(w$) >= 5, "words are long enough: " + w$
    FOR i = 1 TO LEN(w$)
      ASSERT INSTR(ALPHABET$, MID$(w$, i, 1)) > 0, "only capital letters in " + w$
    NEXT
  NEXT
END TEST

TEST PickWord_covers_the_list_edges
  ASSERTEQ gWords[1], PickWord$(0)
  ASSERTEQ gWords[LEN(gWords)], PickWord$(0.9999999)
  ASSERTEQ gWords[1 + INT(0.5 * LEN(gWords))], PickWord$(0.5)
END TEST

TEST MaskWord_hides_untried_letters
  ASSERTEQ "_ _ _ _ _", MaskWord$("PANDA", "")
  ASSERTEQ "_ A _ _ A", MaskWord$("PANDA", "AXZ")
  ASSERTEQ "P A N D A", MaskWord$("PANDA", "ADNPQ")
END TEST

TEST IsSolved_needs_every_letter
  ASSERTEQ 0, IsSolved("PANDA", "")
  ASSERTEQ 0, IsSolved("PANDA", "PAN")
  ASSERTEQ 1, IsSolved("PANDA", "NDAP")
  ASSERTEQ 1, IsSolved("PANDA", "PANDA")
END TEST

TEST GuessError_checks_input
  ASSERTEQ "", GuessError$("A", "")
  ASSERTEQ "", GuessError$("PANDA", "PANDA")
  ASSERT INSTR(GuessError$("", ""), "type a letter") > 0
  ASSERT INSTR(GuessError$("7", ""), "A-Z") > 0
  ASSERT INSTR(GuessError$("A B", ""), "A-Z") > 0
  ASSERT INSTR(GuessError$("E", "XEQ"), "already tried E") > 0
END TEST

TEST ApplyGuess_hits_and_misses
  DIM r AS Round = NewRound("PANDA")
  DIM hit AS INTEGER
  hit = ApplyGuess(r, "A")
  ASSERTEQ 1, hit
  ASSERTEQ 0, r.wrong
  ASSERTEQ "A", r.tried$
  hit = ApplyGuess(r, "Z")
  ASSERTEQ 0, hit
  ASSERTEQ 1, r.wrong
  ASSERTEQ 0, r.solved
  hit = ApplyGuess(r, "P")
  hit = ApplyGuess(r, "N")
  hit = ApplyGuess(r, "D")
  ASSERTEQ 1, r.solved, "all letters found"
  ASSERTEQ 1, r.wrong
END TEST

TEST ApplyGuess_whole_words
  DIM r AS Round = NewRound("PANDA")
  DIM hit AS INTEGER
  hit = ApplyGuess(r, "PENGUIN")
  ASSERTEQ 0, hit
  ASSERTEQ 1, r.wrong
  ASSERTEQ 0, r.solved
  hit = ApplyGuess(r, "PANDA")
  ASSERTEQ 1, hit
  ASSERTEQ 1, r.solved
  ASSERTEQ 1, r.wrong
END TEST

TEST IsLost_after_six_misses
  DIM r AS Round = NewRound("PANDA")
  DIM miss AS INTEGER
  DIM letter$ AS STRING = "QWXZJK"
  DIM i AS INTEGER
  FOR i = 1 TO 6
    ASSERTEQ 0, IsLost(r), "still alive before miss " + STR$(i)
    miss = ApplyGuess(r, MID$(letter$, i, 1))
  NEXT
  ASSERTEQ 6, r.wrong
  ASSERTEQ 1, IsLost(r)
  ASSERTEQ 0, r.solved
END TEST

TEST Gallows_stages_grow_and_differ
  DIM stage AS INTEGER
  DIM picture AS LIST
  DIM previous$ AS STRING = ""
  DIM current$ AS STRING
  FOR stage = 0 TO MAX_WRONG
    picture = Gallows(stage)
    ASSERTEQ 7, LEN(picture), "seven lines at stage " + STR$(stage)
    current$ = JOIN$(picture, "|")
    ASSERT current$ <> previous$, "stage " + STR$(stage) + " differs from the one before"
    previous$ = current$
  NEXT
  ASSERTEQ "      |", Gallows(0)[3], "an empty gallows has no head"
  ASSERTEQ "  O   |", Gallows(1)[3]
  ASSERTEQ " /|\  |", Gallows(4)[4]
  ASSERTEQ " / \  |", Gallows(6)[5]
END TEST

TEST ReadLine_reports_end_of_input
  ASSERTEQ "", ReadLine$("> ")
  ASSERTEQ 1, gEof
END TEST

REM ---------------------------------------------------------------------
REM Main program
REM ---------------------------------------------------------------------

DIM gSeed
DIM gRounds
DIM gWins
DIM gResult
gSeed = SeedFromArguments()
IF ARGC() = 0 THEN PRINT "(Seed "; gSeed; " - pass it as an argument to replay this game.)"
PRINT "HANGMAN"
gRounds = 0
gWins = 0
DO
  gRounds = gRounds + 1
  gResult = PlayRound()
  gWins = gWins + gResult
  IF gEof THEN EXIT DO
LOOP UNTIL AskYesNo("Play again? (y/n)> ") = 0 OR gRounds >= 50
PRINT
PRINT "Games won: "; gWins; " of "; gRounds
