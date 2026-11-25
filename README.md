# BASIC Interpreter

A personal implementation of extended BASIC, compiled to bytecode and executed
by a standalone Rust virtual machine (not the JVM).

Version 0.5.0 covers seven feature areas: (1) core BASIC with collections,
loops, functions, typed values, a standard library and a tracing garbage
collector; (2) file I/O and structured errors; (3) records; (4) objects
(`CLASS`); (5) modules, named/optional parameters; (6) console control,
software-rendered graphics and events, with a desktop window backend; (7)
built-in unit testing (`TEST` blocks, `ASSERT*` statements, `basic-vm --test`);
plus sound: `BEEP`, `SOUND`, `PLAY` music strings, `WAVE`, `VOLUME`, WAV file
playback (`PLAYWAV`, `WAVLENGTH`) and real audio output through the sound
device, with `--save-audio` to render to a WAV file (see [Sound](#sound)).

Examples: `control`, `stdlib`, `fileio`, `types`, `records`, `modules`,
`objects`, `complex` (a `Complex` class library in `examples/lib/complexutil.bas`,
with its own unit tests), `testing` (unit tests),
`graphics`, `paint` and `animation` (double-buffered animation), `sound` and
`wavdemo` (plus `collections`, `functions`, `hello`,
`input`, `primes`) in `examples/`, and the five text games in `examples/games/`
(see [Games](#games)).

<!-- ai-hub:start -->
## Shared AI guidance

This project uses [shared AI guidance](C:\Users\vdvuursj\Documents\workspace\private\AI-hub) maintained in
[jwvdvuurst/ai-hub](https://github.com/jwvdvuurst/ai-hub).

Before changing this project:

1. Follow this repository's local instructions and current code first.
2. Consult the shared [general rules](https://github.com/jwvdvuurst/ai-hub/blob/main/rules/general_rules.md).
3. Use the relevant [skills](https://github.com/jwvdvuurst/ai-hub/tree/main/skills) for the task.
4. If repository-local guidance conflicts with ai-hub, the repository-local guidance wins.
<!-- ai-hub:end -->

## Getting Started

Install a stable Rust toolchain and the native linker for your platform. On
Windows, use the Visual Studio C++ build tools.

```powershell
cargo run -- examples\collections.bas
cargo run -- examples\complex.bas
cargo run -- examples\control.bas
cargo run -- examples\fileio.bas
cargo run -- examples\functions.bas
cargo run -- examples\hello.bas
cargo run -- examples\input.bas
cargo run -- examples\games\wumpus.bas 42cargo run -- examples\stdlib.bas extra arguments
cargo run -- --dump-bytecode examples\hello.bas
cargo run -- --test examples\testing.bas
cargo run -- --test examples\lib\complexutil.bas
cargo run -- --test --test-imports examples\complex.bas
cargo run -- --headless-graphics --screenshot scene.ppm examples\graphics.bas
cargo run -- --headless-graphics --screenshot frame.png examples\animation.bas
cargo build --release
.\target\release\basic-vm.exe examples\collections.bas
```

If you use the workspace-local Rust installation, set these variables for the
current PowerShell session first (the installation does not alter system PATH):

```powershell
$env:RUSTUP_HOME = Join-Path $PWD '.toolchain\rustup'
$env:CARGO_HOME = Join-Path $PWD '.toolchain\cargo'
$env:PATH = "$env:CARGO_HOME\bin;$env:PATH"
```

### Console and graphics

CLS, COLOR, LOCATE, INKEY$(), SLEEP, SCREEN, PSET, LINE, CIRCLE, TEXT,
ON KEY|MOUSE|CLOSE CALL handlers and DOEVENTS are documented in the
[Console, graphics and events](docs/statements.md#console-graphics-and-events)
section. Graphics run on a GraphicsBackend. By default `SCREEN` opens a desktop
window (minifb, the default `window` Cargo feature; `--no-default-features`
builds without it). CLI flags, before the program path:

- `--headless-graphics`: draw to an in-memory canvas instead of a window.
- `--screenshot file.ppm|file.bmp|file.png`: implies headless and saves the last
  presented frame.
- `--auto-close-ms <n>` / `--max-frames <n>`: close the screen after n
  milliseconds / presented frames (bounded runs, e.g. `examples\paint.bas`).
- `--dump-bytecode`: print the instruction stream instead of running.

Colours below 16 are palette indexes, so `RGB(0, 0, n)` with `n < 16` is
drawn as the palette colour.

Double buffering: `SCREEN 160, 120, "Anim", 2` draws into a hidden back buffer
and only `FLIP` shows it (`BUFFER`, `COPYBUFFER`, `ACTIVEBUFFER()`,
`VISIBLEBUFFER()`, `BUFFERS()`, `FLIP CLEAR`). `SCREENSHOT "file.png" [, VISIBLE]`
saves a buffer from BASIC code as `.ppm`, `.bmp` or `.png`. Geometry helpers:
`WIDTH()`, `HEIGHT()`, `CENTERX()`, `CENTERY()`, `CENTER()` (a `[x, y]` list)
and `INSIDE(x, y)` (is the pixel on the screen?).

```powershell
cargo run -- --auto-close-ms 800 examples\paint.bas
cargo run -- --auto-close-ms 2000 examples\animation.bas
```

### Sound

`BEEP`, `SOUND freq, ms [, volume]`, `PLAY mml$`, `WAVE`, `VOLUME`, `STOPSOUND`,
`SOUNDWAIT`, `PLAYWAV file$` and the functions `SOUNDPLAYING()`, `SOUNDQUEUE()`,
`NOTEFREQ()`, `MUSICLENGTH()` and `WAVLENGTH()` are documented in the
[Sound](docs/statements.md#sound) section.
Sounds are rendered by a deterministic 44.1 kHz mono software synthesizer
(`src/audio.rs`; WAV files are decoded by `src/wav.rs`) and queued on an
`AudioBackend`. The command line plays them through the sound device (the
default `audio` Cargo feature, built on [cpal](https://crates.io/crates/cpal):
WASAPI, CoreAudio or ALSA) and waits for the last queued sound to finish before
exiting. Without a device it prints one warning line and carries on silently. The
library default is a silent virtual-clock backend (`NullAudio`), and tests use
`HeadlessAudio`, which records the exact PCM. CLI flags, before the program path:

- `--headless-audio`: do not open the device (virtual clock, nothing is heard,
  nothing waits).
- `--save-audio out.wav`: implies `--headless-audio` and writes everything the
  program played to a 16-bit mono 44.1 kHz WAV file.

```powershell
cargo run -- --save-audio out.wav examples\sound.bas   # render silently to a file
cargo run -- examples\wavdemo.bas out.wav              # play it back (VOLUME 30)
cargo run -- examples\sound.bas                        # play it through the speakers
```

On Linux the `audio` feature needs the ALSA development headers
(`libasound2-dev`); `cargo build --no-default-features` builds without sound
output and window support.

### Implemented language

See the [supported statements reference](docs/statements.md) for syntax,
examples and current limitations.

- One statement per physical line. Keywords and variable names are
  case-insensitive; string contents are preserved.
- Optional positive integer line numbers act as jump labels. Numbered and
  unnumbered lines may be mixed. Execution follows **source order**, not numeric
  label order. Labels are scoped to the main program or current function.
  Duplicate labels within a scope and unresolved jumps are compile errors.
- `LET X = expression` or `X = expression`. Variables without `$` accept finite
  `f64` numbers or collection references; string variables end in `$`.
  Variables must be assigned before use.
- Exact 64-bit integers (`123&`, `&HFF`, `INT64`, `CINT`; checked `+ - * \ MOD`),
  `DIM x AS INTEGER|DOUBLE|BOOLEAN|STRING|LIST|MAP|ARRAY|VARIANT`, typed
  `LOCAL`/`PARAMETER`/function results, `OPTION EXPLICIT`, `TYPEOF`,
  multi-dimensional `DIM A(3,4)` / `REDIM` / `UBOUND`, and
  `FOR EACH k$, v IN map`. Plain literals and arithmetic stay `f64`; see
  [typed values](docs/statements.md#typed-values-declarations-and-multi-dimensional-arrays).
- `PRINT expression`: a newline by default; `;` joins output without spacing
  and `,` inserts a tab. A trailing separator suppresses the newline.
- `INPUT X` reads a number; `INPUT X$` reads a line of text.
- `+`, `-`, `*`, `/`, `^` (right-associative, above unary minus), parentheses and unary signs. `+` also concatenates strings.
- `MOD(a, b)` or `a MOD b` computes a numeric remainder, including fractional
  operands. Nonzero results have the dividend's sign; a zero divisor is an error.
- `=`, `<>`, `<`, `<=`, `>`, `>=` compare two numbers or two strings and return
  `1` or `0`. Multiplication/division/`MOD` bind more tightly than addition/subtraction,
  which bind more tightly than comparisons.
- `GOTO 100`, `IF condition THEN 100`, or `IF condition THEN statement`, plus
  block `IF` / `ELSEIF` / `ELSE` / `END IF`. Conditions are numeric: zero is false.
- `TRUE`/`FALSE`, `NOT`, `AND`, `OR` (both sides evaluated) and short-circuit
  `ANDALSO`/`ORELSE`; precedence is comparisons, `NOT`, `AND`, `OR`.
- `SELECT CASE` with values, `a TO b` ranges, `IS <op> x` and `CASE ELSE`;
  `DO`/`LOOP` with `WHILE`/`UNTIL`; `EXIT FOR|WHILE|DO|FUNCTION`;
  `CONTINUE FOR|WHILE|DO`; and `CONST NAME = expression`.
- `END` / `STOP`, `REM` comments, apostrophe comments, and `GC` to collect now.
  Double quotes within a string are escaped as `""`.
- `ARRAY(...)` creates a fixed-length indexed array, `LIST(...)` creates a
  resizable list, and `MAP(key, value, ...)` creates a string-keyed associative
  array. Elements can be numbers, strings or nested collections.
- Arrays and lists use **one-based** indexing: `A[1]`. Maps use string keys:
  `M["name"]`. Indexed assignment mutates the shared collection.
- `APPEND L, value`, `REMOVE L, index`, `REMOVE M, key`, and `LEN(collection)`
  provide collection operations. `LEN(string)` counts Unicode characters.
- A standard library of math (`ABS`, `SQRT`, `MIN`, `MAX`, `ROUND`, `INT`, `FIX`,
  `SIN`, `COS`, `TAN`, `ATN`, `ATN2`, `PI`, `LOG`, `EXP`, `^`, `RND`/`RANDOMIZE`), string (`LEFT$`, `RIGHT$`,
  `MID$`, `INSTR`, `REPLACE$`, `TRIM$`, `UCASE$`, `LCASE$`, `STR$`, `VAL`,
  `SPLIT`, `JOIN$`), collection (`INSERT`, `POP`, `SORT`, `REVERSE`, `CONTAINS`,
  `KEYS`, `VALUES`, `HASKEY`) and utility (`TIMER`, `UNIXTIME`, `DATE$`, `TIME$`,
  `NOW$`, `ARGC`, `ARG$`, `ENV$`) routines. They are recognised only before `(`
  (or at statement start), so old variables with those names still work, and a
  user `FUNCTION` of the same name shadows the builtin. Extra command-line
  arguments after the program path feed `ARGC()`/`ARG$(n)`. See
  [Standard library](docs/statements.md#standard-library) and
  [the example](examples/stdlib.bas).
- `FOR I = start TO end [STEP step]` / `NEXT [I]` supports inclusive numeric
  ranges. `FOR EACH item IN collection` / `NEXT [item]` visits a shallow snapshot
  of values; maps preserve insertion order.
- `WHILE condition` / `WEND` reevaluates the numeric condition before each pass.
  Structured loops can be nested.
- `FUNCTION NAME` / `FEND`, optional `PARAMETER` declarations and `LOCAL VAR`
  support recursive functions with independent per-call variables. Assign the
  result to the function's own name and use `NAME(args)` in expressions, or
  `CALL NAME(args)` for side effects. Undeclared variables still use global scope.
- File I/O: `OPEN path FOR INPUT|OUTPUT|APPEND|BINARY AS #n`, `CLOSE`,
  `PRINT #n`, `INPUT #n`, `LINE INPUT #n`, `PUT`/`GET`/`SEEK`, `KILL`, and
  `EOF`/`LOF`/`LOC`/`FREEFILE`/`ISOPEN`/`FILEEXISTS`. Handles are closed and
  flushed deterministically, even after errors.
- `TRY` / `CATCH [var$]` / `FINALLY` / `END TRY` and `THROW` with correct
  unwinding; `ERR$()`, `ERRLINE()`, `ERRFUNC$()`. `FINALLY` also runs for
  `EXIT`/`CONTINUE`. See [File I/O](docs/statements.md#file-io) and
  [Structured errors](docs/statements.md#structured-errors).
- Records: `TYPE ... END TYPE`, `NEW`, `p.field`, `NOTHING`, `COPY`, `FIELDS`. See
  [Records](docs/statements.md#records).
- Objects: `CLASS ... END CLASS` = record + methods, implicit `SELF`, `FUNCTION NEW`
  constructors, `obj.Method(args)` / `CALL obj.Method`. See
  [Objects](docs/statements.md#objects-class) and `examples/objects.bas`. Objects
    have no inheritance or `PRIVATE`/`PUBLIC` access control; they can be used with
    the console and graphics statements like any other values.
- Parameters: `PARAMETER [BYVAL|BYREF] [OPTIONAL] name [AS type] [= default]` and
  named arguments `F(a, k := 5)`. See
  [Parameter passing](docs/statements.md#parameter-passing-optional-and-named-arguments).
- Modules: `IMPORT "lib/x.bas" AS X`, then `X.F()`, `X.var`, `X.CONST`, `X.Type`.
  Imports resolve relative to the importing file, load once, run before the
  importer, report errors as `path:line:`. Library entry points: `compile`,
  `compile_from(source, path)`, `compile_file(path)`. See
  [Modules](docs/statements.md#modules-import) and `examples/modules.bas`.
- Unit testing: top-level `TEST name` ... `END TEST` blocks (skipped in normal
  runs; `EXIT TEST` passes early) and `ASSERT cond [, msg$]`,
  `ASSERTEQ expected, actual [, msg$]` (deep equality), `ASSERTNEAR expected,
  actual, eps [, msg$]`; failures are catchable errors. See
  [Unit testing](#unit-testing-cli) and the
  [reference](docs/statements.md#unit-testing).

```basic
NAME$ = "BASIC"
PRINT "Hello, "; NAME$
COUNT = 1
100 PRINT COUNT
COUNT = COUNT + 1
IF COUNT <= 3 THEN 100
END
```

Errors include the physical source line and the CLI exits unsuccessfully.
Programs run until they halt or reach the end; infinite loops are not
automatically interrupted.

### Unit testing CLI

```powershell
cargo run -- --test examples\testing.bas
cargo run -- --test --test-filter clamp examples\testing.bas
cargo run -- --test --test-imports examples\complex.bas
```

`--test` (alias `--run-tests`) runs the program file's tests instead of the
program (the main body is not executed); each test runs on a fresh VM with
headless graphics, empty stdin and captured output (capped at 64 KiB per test);
`SLEEP` does not wait. A function that recurses without end stops with a
catchable `call depth limit exceeded (25000 frames)` error, in tests and normal
runs alike. Options, before the path:
`--test-filter text` (case-insensitive name match) and `--test-imports` (also
run tests of imported modules, reported as `Alias.name`). Output is one
`PASS name` / `FAIL name: message (file:line)` / `ERROR name: message (file:line)`
line per test, printed as each test finishes, and a summary `N tests: P passed, F failed, E errors`. Exit code
`0` when at least one test ran and all passed, `1` on failures, errors, compile
errors or `no tests found`, `2` for command-line misuse (for example combining
`--test` with `--screenshot` or `--dump-bytecode`). `END`/`STOP` inside a test is an
`ERROR`; use `EXIT TEST`. There is no per-test timeout. The library API is
`run_tests(&program, &TestOptions)` (or `run_tests_with` to stream results).

### Games

`examples/maze.bas` is a graphical maze-chase game with synthesized sound
effects. Collect every dot while avoiding the pursuer. Use the arrow keys or
WASD to steer, R to restart after a win or loss, and Esc to quit.

```powershell
cargo run -- examples\maze.bas
cargo run -- --test examples\maze.bas
cargo run -- --headless-graphics --headless-audio --max-frames 20 examples\maze.bas
```

Five complete text games live in `examples/games/`. They are independent,
clean-room implementations of well-known public game concepts (the rules are
common knowledge; no original listing was used - each file header credits the
concept's origin). They show the idioms of the dialect: `OPTION EXPLICIT`, pure
game logic in `FUNCTION`s kept apart from input/output, `TRY`/`CATCH` around
`INPUT` so the end of the input ends the game gracefully, and a `TEST` suite
per game.

```powershell
basic-vm examples\games\hamurabi.bas [seed]
basic-vm examples\games\wumpus.bas [seed] [reveal]
basic-vm examples\games\lander.bas [seed]
basic-vm examples\games\hangman.bas [seed]
basic-vm examples\games\mastermind.bas [seed] [distinct]
basic-vm --test examples\games\wumpus.bas        # likewise for the others
```

(With Cargo: `cargo run -- examples\games\wumpus.bas 42`.) The optional
numeric argument seeds the random generator, so a game can be replayed or
scripted; without it the clock is used and the seed is printed at the start.
Every game also stops cleanly (exit code 0, no error) when its input ends, so
they can be driven from a pipe: `"M 2","Q" | basic-vm examples\games\wumpus.bas 42`.

| Game | Controls | Tests |
| --- | --- | --- |
| `hamurabi.bas` - rule a city for ten years: 100 people, 2800 bushels, 1000 acres. | Each year enter acres to buy (negative sells), bushels to feed (20 per person) and acres to plant; amounts you do not have are refused with a message. Famine over 45% in a year ends your reign; a final rating follows year ten. | 11 |
| `wumpus.bas` - hunt the wumpus in a 20-room dodecahedron cave with two pits and two bat colonies; five arrows. | `M room` (or just a room number) walks, `S r1 r2 ...` shoots an arrow through up to five rooms (it strays if the next room is not connected), `H` help, `Q` quit. The extra argument `reveal` prints the hazard positions (a debugging aid). | 12 |
| `lander.bas` - land a module on the Moon: each 10 s turn choose a fuel burn rate. | Type a burn rate in kg/s (0 up to the limit shown); touch down at 3 m/s or less to walk away, 1 m/s or less is perfect. With the tank empty the lander free-falls. `y`/`n` flies again. | 12 |
| `hangman.bas` - guess a word (42 built in) before the gallows is complete. | Type a letter, or the whole word; repeated or invalid guesses are explained and cost nothing. Six misses lose. `y`/`n` plays again. | 10 |
| `mastermind.bas` - crack a hidden code of 4 letters from A-F in 10 attempts. | Type four letters (spaces optional); the reply counts `exact` and `misplaced` pegs. `Q` gives up. The extra argument `distinct` forbids repeated letters in the code. `y`/`n` plays again. | 10 |

`cargo test --test games` drives every game through the built binary with a
fixed seed and scripted input (with a 20-second watchdog) and runs each game's
`--test` suite.

### VM and memory

The lexer and compiler produce stack-machine instructions with resolved jump
addresses. The VM executes instructions, not source text. `--dump-bytecode`
shows the in-memory instruction stream; a portable bytecode file format is
not implemented yet.

Each VM owns a private managed arena initially sized at **1 MiB
(1,048,576 bytes)**. This is a heap reservation, not a total process-memory
limit or a security sandbox. Strings and collection payloads, including
iteration snapshots, reside in the arena. Bytecode, scalar numeric variables,
VM stacks, handle metadata and temporary host allocations are outside it.

The collector traces roots from global and per-call variables, the operand stack
and active or suspended iteration snapshots, follows collection elements and map keys, then sweeps
unreachable allocations and compacts live bytes. Unreachable cycles are
collected. Stable handles survive compaction; free metadata slots are reused.
Collection runs on allocation pressure, after 256 allocation/mutation events,
or `GC`. The arena grows to a power of two when
necessary and shrinks after collection when live data permits, retaining
roughly twice the live payload and never dropping below 1 MiB. Empty strings
and empty collections consume one arena byte. Host allocators determine when released capacity is
returned to the OS.

`Vm::heap_stats()` reports arena capacity, occupied bytes, allocated objects and
collection count. Occupied bytes can include garbage until the next collection.

## Development

The only third-party dependencies are optional, both enabled by default:
`minifb` for the `window` feature and `cpal` for the `audio` feature (sound
device output); `cargo build --no-default-features` has none.

```powershell
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Tests cover parsing, bytecode jumps, execution, errors, I/O, GC roots,
compaction, metadata reuse, per-VM isolation, heap growth/shrinkage,
collection mutations, cyclic graphs, nested/snapshot loops, function scoping,
recursion and GC during suspended calls, file I/O (text, binary, handle
release) and TRY/CATCH/FINALLY unwinding.
