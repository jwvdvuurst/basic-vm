REM ======================================================================
REM MASTERMIND - break the secret colour code.
REM The computer hides a code of four pegs, each one of the letters A-F.
REM After every guess you are told how many pegs are exactly right (right
REM letter in the right place) and how many are misplaced (right letter in
REM the wrong place). You have ten attempts.
REM
REM Original game concept: Mordecai Meirowitz, 1970 (the board game), itself
REM a descendant of the older paper game Bulls and Cows; this is an
REM independent implementation, not a copy of any original listing.
REM
REM Play:        basic-vm examples\games\mastermind.bas [seed] [distinct]
REM Run tests:   basic-vm --test examples\games\mastermind.bas
REM
REM Arguments (in any order): a number is the random seed, which makes a
REM game replayable (without it the clock is used and the seed is printed);
REM the word "distinct" forbids repeated letters in the secret code.
REM Guesses are four letters A-F, with or without spaces. Q gives up.
REM The game ends gracefully when the input ends.
REM ======================================================================
OPTION EXPLICIT

CONST CODE_LENGTH = 4
CONST LETTERS$ = "ABCDEF"
CONST MAX_ATTEMPTS = 10

DIM gEof AS INTEGER = 0

REM ---------------------------------------------------------------------
REM Pure game logic
REM ---------------------------------------------------------------------

REM Builds a secret code from CODE_LENGTH rolls (0 <= roll < 1). With
REM `distinct` set, each roll picks among the letters not yet used.
FUNCTION CodeFromRolls$(PARAMETER rolls AS LIST, PARAMETER distinct) AS STRING
  LOCAL pool$ AS STRING
  LOCAL code$ AS STRING
  LOCAL pick
  LOCAL i
  pool$ = LETTERS$
  code$ = ""
  FOR i = 1 TO CODE_LENGTH
    pick = 1 + INT(rolls[i] * LEN(pool$))
    code$ = code$ + MID$(pool$, pick, 1)
    IF distinct THEN pool$ = LEFT$(pool$, pick - 1) + MID$(pool$, pick + 1)
  NEXT
  CodeFromRolls$ = code$
FEND

FUNCTION RandomCode$(PARAMETER distinct) AS STRING
  LOCAL rolls AS LIST
  LOCAL i
  rolls = LIST()
  FOR i = 1 TO CODE_LENGTH
    APPEND rolls, RND()
  NEXT
  RandomCode$ = CodeFromRolls$(rolls, distinct)
FEND

REM Scores a guess against the secret. Returns LIST(exact, misplaced).
REM Each secret peg can be matched only once, so repeated letters are
REM counted correctly: secret ABBA vs guess BBBB is 2 exact, 0 misplaced.
FUNCTION Score(PARAMETER secret$ AS STRING, PARAMETER guess$ AS STRING) AS LIST
  LOCAL exact
  LOCAL common
  LOCAL i
  LOCAL letter$ AS STRING
  LOCAL inSecret
  LOCAL inGuess
  exact = 0
  FOR i = 1 TO CODE_LENGTH
    IF MID$(secret$, i, 1) = MID$(guess$, i, 1) THEN exact = exact + 1
  NEXT
  common = 0
  FOR i = 1 TO LEN(LETTERS$)
    letter$ = MID$(LETTERS$, i, 1)
    inSecret = CountLetter(secret$, letter$)
    inGuess = CountLetter(guess$, letter$)
    common = common + MIN(inSecret, inGuess)
  NEXT
  Score = LIST(exact, common - exact)
FEND

FUNCTION CountLetter(PARAMETER text$ AS STRING, PARAMETER letter$ AS STRING)
  LOCAL i
  CountLetter = 0
  FOR i = 1 TO LEN(text$)
    IF MID$(text$, i, 1) = letter$ THEN CountLetter = CountLetter + 1
  NEXT
FEND

REM The guess in canonical form: upper case, spaces removed.
FUNCTION NormalizeGuess$(PARAMETER text$ AS STRING) AS STRING
  NormalizeGuess$ = REPLACE$(UCASE$(TRIM$(text$)), " ", "")
FEND

REM "" if the (normalized) guess is acceptable, otherwise the complaint.
FUNCTION GuessError$(PARAMETER guess$ AS STRING, PARAMETER distinct) AS STRING
  LOCAL i
  GuessError$ = ""
  IF LEN(guess$) <> CODE_LENGTH THEN
    GuessError$ = "A guess is exactly " + STR$(CODE_LENGTH) + " letters from " + LEFT$(LETTERS$, 1) + " to " + RIGHT$(LETTERS$, 1) + "."
    EXIT FUNCTION
  END IF
  FOR i = 1 TO CODE_LENGTH
    IF INSTR(LETTERS$, MID$(guess$, i, 1)) = 0 THEN
      GuessError$ = "'" + MID$(guess$, i, 1) + "' is not one of the letters " + LETTERS$ + "."
      EXIT FUNCTION
    END IF
  NEXT
  IF distinct THEN
    FOR i = 1 TO CODE_LENGTH
      IF CountLetter(guess$, MID$(guess$, i, 1)) > 1 THEN
        GuessError$ = "In this game every letter appears only once."
        EXIT FUNCTION
      END IF
    NEXT
  END IF
FEND

REM The feedback line for a score list, e.g. "2 exact, 1 misplaced".
FUNCTION Feedback$(PARAMETER result AS LIST) AS STRING
  Feedback$ = STR$(result[1]) + " exact, " + STR$(result[2]) + " misplaced"
FEND

FUNCTION IsWin(PARAMETER result AS LIST)
  IsWin = 0
  IF result[1] = CODE_LENGTH THEN IsWin = 1
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

REM Scans the program arguments in any order: a number is the seed (the
REM clock is used without one). Sets gSeedGiven when a seed was supplied.
DIM gSeedGiven AS INTEGER = 0
DIM gDistinct AS INTEGER = 0

FUNCTION ReadArguments()
  LOCAL seed
  LOCAL i
  LOCAL arg$ AS STRING
  seed = UNIXTIME()
  FOR i = 1 TO ARGC()
    arg$ = LCASE$(ARG$(i))
    IF arg$ = "distinct" THEN
      gDistinct = 1
    ELSE
      TRY
        seed = VAL(arg$)
        gSeedGiven = 1
      CATCH problem$
        PRINT "(Ignoring argument "; arg$; ".)"
      END TRY
    END IF
  NEXT
  RANDOMIZE seed
  ReadArguments = seed
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

REM Plays one game against the given secret. Returns 1 for a win, 0
REM otherwise (out of attempts, gave up, or the input ended).
FUNCTION PlayRound(PARAMETER secret$ AS STRING)
  LOCAL attempt
  LOCAL guess$ AS STRING
  LOCAL msg$ AS STRING
  LOCAL result AS LIST
  PlayRound = 0
  attempt = 1
  DO WHILE attempt <= MAX_ATTEMPTS
    guess$ = NormalizeGuess$(ReadLine$("Attempt " + STR$(attempt) + " of " + STR$(MAX_ATTEMPTS) + "> "))
    IF gEof THEN
      PRINT
      PRINT "The input ended; the game is abandoned. The code was "; secret$; "."
      EXIT FUNCTION
    END IF
    IF guess$ = "Q" THEN
      PRINT "You give up. The code was "; secret$; "."
      EXIT FUNCTION
    END IF
    msg$ = GuessError$(guess$, gDistinct)
    IF msg$ <> "" THEN
      PRINT msg$
    ELSE
      result = Score(secret$, guess$)
      PRINT guess$; ": "; Feedback$(result)
      IF IsWin(result) THEN
        PRINT "You cracked the code in "; attempt; " attempts!"
        PlayRound = 1
        EXIT FUNCTION
      END IF
      attempt = attempt + 1
    END IF
  LOOP
  PRINT "Out of attempts. The code was "; secret$; "."
FEND

REM ---------------------------------------------------------------------
REM Unit tests: basic-vm --test examples\games\mastermind.bas
REM ---------------------------------------------------------------------

TEST Score_all_cases
  ASSERTEQ LIST(4, 0), Score("ABCD", "ABCD")
  ASSERTEQ LIST(0, 4), Score("ABCD", "DCBA")
  ASSERTEQ LIST(0, 0), Score("AAAA", "BBBB")
  ASSERTEQ LIST(1, 1), Score("ABCD", "AEBF"), "one in place, one misplaced"
  ASSERTEQ LIST(2, 0), Score("ABCD", "ABFF")
END TEST

TEST Score_counts_repeated_letters_once_per_secret_peg
  ASSERTEQ LIST(2, 0), Score("ABBA", "BBBB"), "only two Bs in the secret"
  ASSERTEQ LIST(1, 2), Score("AABB", "ABAF"), "A exact; A, B misplaced"
  ASSERTEQ LIST(0, 1), Score("ABCD", "EEEA"), "a single A is misplaced once"
  ASSERTEQ LIST(1, 0), Score("AEEE", "AAAF"), "extra guessed As earn nothing"
  ASSERTEQ LIST(1, 2), Score("AABC", "CAFA"), "pos 2 exact; A and C misplaced"
END TEST

TEST Score_is_symmetric_in_its_total
  DIM a AS LIST = Score("ABCA", "CABB")
  DIM b AS LIST = Score("CABB", "ABCA")
  ASSERTEQ a, b
  ASSERTEQ 3, a[1] + a[2], "three letters in common"
END TEST

TEST CodeFromRolls_repeats_allowed
  ASSERTEQ "AAAA", CodeFromRolls$(LIST(0, 0, 0, 0), 0)
  ASSERTEQ "FFFF", CodeFromRolls$(LIST(0.99, 0.99, 0.99, 0.99), 0)
  ASSERTEQ "ABCF", CodeFromRolls$(LIST(0, 0.2, 0.4, 0.99), 0)
END TEST

TEST CodeFromRolls_distinct_never_repeats
  ASSERTEQ "ABCD", CodeFromRolls$(LIST(0, 0, 0, 0), 1)
  ASSERTEQ "FEDC", CodeFromRolls$(LIST(0.99, 0.99, 0.99, 0.99), 1)
  DIM trial AS INTEGER
  DIM code$ AS STRING
  DIM i AS INTEGER
  RANDOMIZE 2024
  FOR trial = 1 TO 50
    code$ = RandomCode$(1)
    ASSERTEQ 4, LEN(code$)
    FOR i = 1 TO 4
      ASSERTEQ 1, CountLetter(code$, MID$(code$, i, 1)), "no repeats in " + code$
    NEXT
  NEXT
END TEST

TEST RandomCode_is_reproducible_and_uses_the_alphabet
  DIM first$ AS STRING
  DIM second$ AS STRING
  DIM i AS INTEGER
  RANDOMIZE 99
  first$ = RandomCode$(0)
  RANDOMIZE 99
  second$ = RandomCode$(0)
  ASSERTEQ first$, second$
  FOR i = 1 TO 4
    ASSERT INSTR(LETTERS$, MID$(first$, i, 1)) > 0, "letter from A-F"
  NEXT
END TEST

TEST NormalizeGuess_and_GuessError
  ASSERTEQ "ABCD", NormalizeGuess$("  a b c d ")
  ASSERTEQ "", GuessError$("ABCD", 0)
  ASSERTEQ "", GuessError$("AABB", 0)
  ASSERT INSTR(GuessError$("ABC", 0), "exactly 4 letters") > 0
  ASSERT INSTR(GuessError$("ABCDE", 0), "exactly 4 letters") > 0
  ASSERT INSTR(GuessError$("", 0), "exactly 4 letters") > 0
  ASSERT INSTR(GuessError$("ABCG", 0), "'G' is not one of") > 0
  ASSERT INSTR(GuessError$("AB1D", 0), "'1' is not one of") > 0
END TEST

TEST GuessError_distinct_mode
  ASSERTEQ "", GuessError$("ABCD", 1)
  ASSERT INSTR(GuessError$("AABC", 1), "only once") > 0
  ASSERT INSTR(GuessError$("ABCA", 1), "only once") > 0
END TEST

TEST Feedback_and_IsWin
  ASSERTEQ "2 exact, 1 misplaced", Feedback$(LIST(2, 1))
  ASSERTEQ "0 exact, 0 misplaced", Feedback$(LIST(0, 0))
  ASSERTEQ 1, IsWin(Score("ABCD", "ABCD"))
  ASSERTEQ 0, IsWin(Score("ABCD", "ABCE"))
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
DIM gSecret$ AS STRING
gSeed = ReadArguments()
IF gSeedGiven = 0 THEN PRINT "(Seed "; gSeed; " - pass it as an argument to replay this game.)"
PRINT "MASTERMIND"
PRINT "I have hidden a code of "; CODE_LENGTH; " letters from "; LETTERS$; "."
IF gDistinct THEN
  PRINT "No letter appears twice in the code."
ELSE
  PRINT "Letters may repeat. (Start with the argument 'distinct' to forbid that.)"
END IF
PRINT "'exact' = right letter, right place; 'misplaced' = right letter, wrong place."
PRINT "Type Q to give up."
gRounds = 0
gWins = 0
DO
  gRounds = gRounds + 1
  gSecret$ = RandomCode$(gDistinct)
  PRINT
  gWins = gWins + PlayRound(gSecret$)
  IF gEof THEN EXIT DO
LOOP UNTIL AskYesNo("Play again? (y/n)> ") = 0 OR gRounds >= 50
PRINT
PRINT "Codes cracked: "; gWins; " of "; gRounds
