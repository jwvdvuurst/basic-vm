# BASIC Interpreter

A personal implementation of extended BASIC, compiled to bytecode and executed
by a standalone Rust virtual machine (not the JVM).

Version 0.3.0 covers six feature areas: (1) core BASIC with collections,
loops, functions, typed values, a standard library and a tracing garbage
collector; (2) file I/O and structured errors; (3) records; (4) objects
(`CLASS`); (5) modules, named/optional parameters; (6) console control,
software-rendered graphics and events, with a desktop window backend.

Examples: `control`, `stdlib`, `fileio`, `types`, `records`, `modules`,
`objects`, `complex` (a `Complex` class library in `examples/lib/complexutil.bas`),
`graphics` and `paint` (plus `collections`, `functions`, `hello`,
`input`, `primes`) in `examples/`.

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
cargo run -- examples\stdlib.bas extra arguments
cargo run -- --dump-bytecode examples\hello.bas
cargo run -- --headless-graphics --screenshot scene.ppm examples\graphics.bas
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
- `--screenshot file.ppm|file.bmp`: implies headless and saves the last frame.
- `--auto-close-ms <n>` / `--max-frames <n>`: close the screen after n
  milliseconds / presented frames (bounded runs, e.g. `examples\paint.bas`).
- `--dump-bytecode`: print the instruction stream instead of running.

Colours below 16 are palette indexes, so `RGB(0, 0, n)` with `n < 16` is
drawn as the palette colour.

```powershell
cargo run -- --auto-close-ms 800 examples\paint.bas
```

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

The only third-party dependency is `minifb`, used by the optional `window`
feature (enabled by default); `cargo build --no-default-features` has none.

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
