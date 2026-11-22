# Supported BASIC Statements

This reference describes the language: text I/O, collections, records, objects,
modules, console control, graphics and events.

## General rules

- Use one statement per physical line. Blank lines are allowed.
- Keywords and variable names are case-insensitive.
- Line numbers are optional jump labels, from `1` to `4294967295`.
  Numbered and unnumbered lines can be mixed. Execution follows source order,
  not numeric line order. Labels are scoped separately to the main program
  and each function.
- Variables without `$` hold finite double-precision numbers or collection
  references. String variable names end in `$`. Assign a variable before reading it.
- Strings use double quotes; write `""` inside a string for a literal quote.

## Statements

| Statement | Syntax / example | Meaning |
| --- | --- | --- |
| Assignment | `LET X = 10` or `X = 10` | Assign an expression to a variable. `LET` is optional. |
| String assignment | `NAME$ = "Ada"` | Assign text to a string variable. |
| Indexed assignment | `A[1] = 10` or `M["name"] = "Ada"` | Replace an element, or insert/update a map entry. |
| `PRINT` | `PRINT "Answer: "; X` | Write expressions to text output. |
| `INPUT` | `INPUT X` or `INPUT NAME$` | Read one line as a number or string, respectively. |
| `GOTO` | `GOTO 100` | Jump to an existing numbered line. |
| `IF ... THEN` | `IF X > 0 THEN PRINT X` | Execute the inline statement when the condition is nonzero. |
| `IF ... THEN` | `IF X < 10 THEN 100` | Jump to a numbered line when the condition is nonzero. |
| Block `IF` | `IF C THEN` / `ELSEIF C THEN` / `ELSE` / `END IF` | Multi-line conditional; nestable. See [Control flow](#structured-control-flow). |
| `SELECT CASE` | `SELECT CASE X` / `CASE ...` / `END SELECT` | Multi-way branch on numbers or strings. |
| `DO` / `LOOP` | `DO WHILE C` ... `LOOP` | Loop with a leading or trailing `WHILE`/`UNTIL` test. |
| `EXIT` | `EXIT FOR`, `EXIT WHILE`, `EXIT DO`, `EXIT FUNCTION`, `EXIT TEST` | Leave the innermost matching loop, the function, or the current `TEST` (as a pass). |
| `CONTINUE` | `CONTINUE FOR`, `CONTINUE WHILE`, `CONTINUE DO` | Start the next pass of the innermost matching loop. |
| `CONST` | `CONST LIMIT = 10` | Declare a constant (see below). |
| Numeric `FOR` | `FOR I = 1 TO 10 STEP 2` | Start an inclusive numeric loop; `STEP` defaults to `1`. |
| Collection `FOR` | `FOR EACH ITEM IN A` | Visit a shallow snapshot of collection values. |
| `NEXT` | `NEXT` or `NEXT I` | Advance the innermost `FOR`; an optional variable must match. |
| `WHILE` | `WHILE X > 0` | Repeat while the numeric condition is nonzero. |
| `WEND` | `WEND` | End the innermost `WHILE` and recheck its condition. |
| `APPEND` | `APPEND L, 30` | Add an element to the end of a list. |
| `REMOVE` | `REMOVE L, 1` or `REMOVE M, "name"` | Remove a list element or map entry. |
| `INSERT` | `INSERT L, 2, "x"` | Insert into a list before a position (see [Standard library](#standard-library)). |
| `SORT` / `REVERSE` | `SORT L`, `REVERSE A` | Sort or reverse an array or list in place. |
| `RANDOMIZE` | `RANDOMIZE 42` or `RANDOMIZE` | Seed the `RND()` generator; no argument seeds from the clock. |
| `FUNCTION` | `FUNCTION ADD(PARAMETER X, PARAMETER Y)` | Declare a function; the parameter list is optional. |
| `FEND` | `FEND` | End the declaration and return from a function call. |
| `LOCAL` | `LOCAL RESULT` | Declare a function-local variable. |
| `CALL` | `CALL HELLO` or `CALL UPDATE(A)` | Call a function for its side effects, discarding any result. |
| `CLASS` | `CLASS Name` ... `END CLASS` | Declare an object type: fields plus methods (`FUNCTION ... FEND`). See [Objects](#objects-class). |
| `CALL` (method) | `CALL obj.Method(args)` | Invoke a method, discarding any result. |
| `TEST` | `TEST Add_basic` ... `END TEST` | Declare a unit test (skipped in normal runs; run with `--test`). See [Unit testing](#unit-testing). |
| `ASSERT` | `ASSERT X > 0 [, "message"]` | Raise a catchable assertion failure unless the condition is nonzero. |
| `ASSERTEQ` | `ASSERTEQ expected, actual [, "message"]` | Fail unless the two values are equal (deep comparison). |
| `ASSERTNEAR` | `ASSERTNEAR expected, actual, epsilon [, "message"]` | Fail unless two numbers differ by at most `epsilon`. |
| `END` | `END` | Terminate the program. |
| `STOP` | `STOP` | Same as `END`; there is no resume operation. |
| `GC` | `GC` | Collect unreachable strings and collections, including cycles, and compact the heap. |
| `REM` | `REM This is a comment` | Ignore the remainder of the line. |
| Apostrophe comment | `X = 10 ' Initial value` | Ignore the remainder of the line outside a string. |

### Output and input details

`PRINT` normally ends with a newline. Between expressions, `;` adds no spacing
and `,` inserts a tab. A trailing separator suppresses the newline.
`PRINT` alone writes a newline.

```basic
PRINT "Hello, "; NAME$; "!"
PRINT X, X * 2
PRINT "Enter a number: ";
INPUT X
```

`INPUT` does not generate a prompt automatically. Numeric input must be a finite
number; string input preserves spaces but removes the line ending.
Invalid input or end-of-input produces an error.

An interactive program that must survive a closed stdin catches that error and
reads whole lines as strings, parsing them itself (a numeric `INPUT` would also
fail on bad text). A prompt printed with a trailing `;` is flushed before
`INPUT` waits. The games in `examples/games/` use this pattern:

```basic
FUNCTION ReadLine$(PARAMETER prompt$ AS STRING) AS STRING
  LOCAL line$ AS STRING
  line$ = ""
  PRINT prompt$;
  TRY
    INPUT line$
  CATCH problem$
    gEof = 1          ' global flag: the input has ended
  END TRY
  ReadLine$ = line$
FEND
```

(`--test` re-runs only top-level `CONST` and `DIM` statements before each test,
so globals that functions use, like `gEof`, must be declared with `DIM`.)

### Expressions and conditions

- Arithmetic: `+`, `-`, `*`, `/`, `^`, `MOD`, unary `+` and `-`, and parentheses.
- Remainder: `MOD(a, b)` or `a MOD b`, as described below.
- String concatenation: `"Hello " + NAME$`.
- Collection constructors: `ARRAY(...)`, `LIST(...)`, and `MAP(key, value, ...)`.
- Element access: `A[1]`, `M["name"]`, and nested access such as `A[1][2]`.
- `LEN(A)` returns the element count; `LEN(M)` returns the entry count.
  `LEN("text")` counts Unicode characters.
- User-defined functions are called as `NAME(arguments)`, including `NAME()`
  for a function without parameters.
- Comparisons: `=`, `<>`, `<`, `<=`, `>`, `>=`, using two numbers or two strings.
  String comparisons are case-sensitive and lexicographic.
- Comparisons return numeric `1` (true) or `0` (false).
  `IF` accepts any numeric expression: zero is false; nonzero is true.
- Precedence, highest first: parentheses, `^` (right-associative), unary signs,
  multiplication/division/`MOD`,
  addition/subtraction, comparisons, `NOT`, `AND`/`ANDALSO`, `OR`/`ORELSE`.
  `-2 ^ 2` is `-4` (the sign applies to the power) while `2 ^ -2` is `0.25`
  (an exponent may carry a sign) and `2 ^ 3 ^ 2` is `2 ^ 9`.
  Binary operators at the same precedence are evaluated left to right.
  `NOT` is a prefix operator whose operand extends over a whole comparison or
  arithmetic expression: `NOT A = B` means `NOT (A = B)`.
- `TRUE` and `FALSE` are the numeric constants `1` and `0`.
- `AND`, `OR` and `NOT` are logical, not bitwise: operands must be numbers, are
  treated as true when nonzero, and the result is always `1` or `0`
  (`5 AND 2` is `1`; `NOT 5` is `0`). `AND` and `OR` **always evaluate both
  operands** (left first), so side effects and errors on the right still happen.
- `ANDALSO` and `ORELSE` are short-circuit forms with the same precedence and
  results. `ANDALSO` skips its right operand when the left is zero; `ORELSE`
  skips it when the left is nonzero. Skipped operands cause no side effects and
  no errors, e.g. `IF D <> 0 ANDALSO 10 / D > 2 THEN`.
- `TRUE`, `FALSE`, `AND`, `OR`, `NOT`, `ANDALSO`, `ORELSE` and the control-flow
  words below are reserved and cannot be variable names.

### MOD: numeric remainder

`MOD(a, b)` returns the remainder after division. The equivalent infix form,
`a MOD b`, has the same precedence as `*` and `/`.

```basic
PRINT MOD(10, 3)    ' 1
PRINT 10 MOD 3     ' 1
PRINT -10 MOD 3    ' -1
PRINT 10 MOD -3    ' 1
PRINT MOD(5.5, 2)  ' 1.5
```

Both operands must be numbers; fractional operands are supported without
integer conversion. A nonzero remainder has the sign of the dividend (the
first operand), not the divisor. A zero divisor, including negative zero,
produces a runtime error. The function form requires exactly two arguments.
`MOD` is an expression, not a standalone statement; use it in assignments,
`PRINT`, conditions, or other expressions.

## Arrays, associative arrays and lists

```basic
A = ARRAY(10, 20, 30)
L = LIST("Ada", "Grace")
M = MAP("apples", 3, "pears", 2)
A[2] = 25
APPEND L, "Linus"
REMOVE L, 1
M["pears"] = 4
M["oranges"] = 5
PRINT A[2], L[1], M["pears"], LEN(M)
```

- Arrays have fixed length; lists can grow and shrink. Both use **one-based**
  integer indices. `ARRAY(10)` creates one element containing `10`, not ten
  empty elements. Empty constructors are allowed.
- Elements may be numbers, strings or nested collections.
- Maps are associative arrays with case-sensitive string keys. New keys are
  inserted on assignment. Updating a key preserves its position; duplicate
  constructor keys keep the last value at the first insertion position.
  Removing and reinserting a key moves it to the end.
- Collection assignment shares a reference: after `B = A`, changing `B[1]`
  also changes `A[1]`.
- Invalid indices, missing map keys on reads/removal, and non-string map keys
  are errors. Arrays cannot be appended to or have elements removed.
- `PRINT` requires scalar elements, not entire collections.

## Structured loops

```basic
FOR I = 1 TO LEN(A)
PRINT A[I]
NEXT I

FOR EACH NAME$ IN L
PRINT NAME$
NEXT

WHILE LEN(L) > 0
REMOVE L, 1
WEND
```

Numeric `FOR` evaluates its start, limit and step once before iteration.
Bounds are inclusive. Negative steps count down; zero steps are errors.
A range pointing away from its limit skips the body. The counter is assigned
the start value even for an empty range, and normally finishes at the first
value beyond the limit. Non-finite or non-advancing increments are errors.
With integer bounds at the 64-bit limit (e.g. `FOR I = MAXINT - 1 TO MAXINT`)
there is no overflow: the loop stops and the counter is left at the final value
(the limit) instead of one beyond it.

`FOR EACH` visits array/list elements in index order and map **values** in
insertion order. It takes a shallow snapshot at entry: changing, appending,
removing or reassigning the source does not change which values are visited.
Nested collections are still shared, not deep-copied. Use a `$` loop variable
for strings and a variable without `$` for numbers or nested collections.
An incompatible element type is an error; mixed string/numeric elements cannot
all be visited using a single typed loop variable. An empty collection skips
the body and leaves the variable unchanged, possibly undefined.

`WHILE` checks its condition before every pass; zero skips the body.
Loops can nest, but `FOR`, `NEXT`, `WHILE`, `WEND`, `DO` and `LOOP` must each be on their own
physical lines, not inside an inline `IF`. Closers must match the innermost loop.
Jumping out of a `FOR` discards its state; jumping back to its header restarts it.
Jumping into its body without entering its header causes an error at `NEXT`.

See the runnable [collection example](../examples/collections.bas).

## Functions and local variables

```basic
FUNCTION ADD(PARAMETER X, PARAMETER Y)
    LOCAL RESULT
    RESULT = X + Y
    ADD = RESULT
FEND

PRINT ADD(2, 3)
```

This prints `5`. Assign the result to the function's own name, following
traditional BASIC style. `FEND` returns that value to the caller.
A function whose name ends in `$` returns a string; other function names return
a number or collection reference. The result slot starts unassigned on every
call. Using a function in an expression without assigning its result produces
an error at `FEND`; there is no implicit zero or empty-string result.

Functions without parameters may omit parentheses in their declarations:

```basic
FUNCTION HELLO
    PRINT "Hello!"
FEND

CALL HELLO
CALL HELLO()
```

`CALL` discards any assigned result and also permits a function to finish
without assigning one. Expression calls always require parentheses.

- Every declared parameter uses the keyword `PARAMETER`, for example
  `FUNCTION SCALE(PARAMETER VALUE, PARAMETER FACTOR)`. Calls supply positional
  expressions: `SCALE(10, 2)`. `PARAMER` is not an alias.
- Parameters and variables declared with `LOCAL` belong to the current call.
  All other variable reads and assignments use the global scope. The function's
  own name is an implicit local result slot, not a global assignment.
- A `LOCAL` declaration applies throughout the function, even before its source
  line. Locals must be assigned before reading them; an unassigned local never
  falls back to a global variable with the same name.
- Declare one variable per `LOCAL` statement. Parameter names, local names and
  the function's result name must not overlap. Existing `$` typing rules apply.
- Numbers and strings are passed by value. Collections are shared references:
  element mutations affect the caller's collection, but reassigning a parameter
  does not reassign the caller's variable (unless the parameter is `BYREF`, see
  [Parameter passing](#parameter-passing-optional-and-named-arguments)).
- Recursion and calls before the function declaration are supported.
  Each call has independent locals, parameters, result and loop state.
  A function cannot read another call's locals.
- **Call depth limit**: at most **25,000** calls may be active at once
  (`basic_vm::MAX_CALL_DEPTH`). Functions, methods, constructors (`NEW`) and
  event handlers all count. Exceeding it raises an ordinary runtime error that
  `TRY`/`CATCH` can catch: `call depth limit exceeded (25000 frames) calling NAME;
  is a function calling itself without end?` (`NAME` is the upper-case,
  module-qualified function). A runaway recursion therefore fails in about
  0.1-0.3 s using roughly 6-35 MB instead of exhausting memory; legitimate
  recursion up to 25,000 nested calls still works.
- Definitions are skipped during normal main-program execution. They must be
  at global scope, outside loops; nested function definitions are not supported.
  `FUNCTION`, `FEND`, `LOCAL` and `CONST` must be on their own physical lines, not inside
  an inline `IF`. Loops cannot cross a function boundary.
- Function names must be unique and cannot use reserved keywords or the
  keyword-style built-ins (`LEN`, `MOD`, `ARRAY`, `LIST`, `MAP`). A function may
  reuse a [standard-library](#standard-library) name (e.g. `MAX`): the user
  function then **shadows** the builtin for every call in the program, wherever
  it is declared. Unknown functions and incorrect argument counts are compile
  errors; incompatible argument types are runtime errors.
- `GOTO` targets must be in the current function or, outside a function, in the
  main program. A label on `FEND` can be used to skip to the return point.
  There is no `RETURN` statement. `END` and `STOP` still halt the entire program.
- All calls share their VM's managed heap. GC traces locals, parameters,
  results, temporary operands and loop snapshots across suspended calls.

See the runnable [function example](../examples/functions.bas).

## Structured control flow

```basic
CONST LIMIT = 10
FOR I = 1 TO LIMIT
    IF I MOD 2 = 0 THEN
        CONTINUE FOR
    ELSEIF I > 7 THEN
        EXIT FOR
    END IF
    SELECT CASE I
        CASE 1
            PRINT "one"
        CASE 3 TO 5, 7
            PRINT "three to five, or seven"
        CASE IS >= 9
            PRINT "big"
        CASE ELSE
            PRINT "other"
    END SELECT
NEXT
```

**Block `IF`.** `IF cond THEN` with nothing after `THEN` starts a block closed by
`END IF`. It may contain any number of `ELSEIF cond THEN` clauses and at most
one final `ELSE`. Blocks nest. The inline form `IF cond THEN statement` and
`IF cond THEN 100` still work; inline `IF` bodies may use `EXIT`/`CONTINUE`
but not blocks or other delimiters. `ELSE IF` (two words) is not accepted;
write `ELSEIF`.

**`SELECT CASE expr`.** The selector is evaluated once. Clauses are tried in
order and only the first matching one runs (no fallthrough). A clause is
`CASE item, item, ...` where an item is a value (`CASE 3`, `CASE "a"`),
a range (`CASE 1 TO 5`, inclusive), or a comparison (`CASE IS < 0`; operators
`= <> < <= > >=`). `CASE ELSE` must be last. Selector and items must have
matching types (numbers with numbers, strings with strings), otherwise a
runtime error occurs. If nothing matches and there is no `CASE ELSE`, nothing
runs. Only `CASE` lines may appear between `SELECT CASE` and the first `CASE`.
Item expressions use `+ - * /` and `MOD` but not comparison or logical operators
(use parentheses to include them).

**`DO` loops.** `DO ... LOOP`, `DO WHILE cond ... LOOP`, `DO UNTIL cond ... LOOP`,
`DO ... LOOP WHILE cond` and `DO ... LOOP UNTIL cond`. A leading test may run
zero times; a trailing test runs at least once. One loop cannot have both a
leading and a trailing condition. `DO` without a condition repeats until
`EXIT DO`, `GOTO` or `END`.

**`EXIT` and `CONTINUE`.** `EXIT FOR|WHILE|DO` leaves the innermost enclosing loop of
that kind (even through inner loops of other kinds, `IF` or `SELECT` blocks).
`CONTINUE FOR` runs the `NEXT` step (advancing the counter or next element);
`CONTINUE WHILE` and `CONTINUE DO` re-test the condition (for `DO ... LOOP WHILE/UNTIL`
that is the trailing test; for a plain `DO` it jumps to the top). Leaving a
`FOR EACH` discards its snapshot so the garbage collector can reclaim it. After
`EXIT FOR` the counter keeps its current value. `EXIT FUNCTION` returns the
result already assigned to the function's name; loops of that call are discarded
and the caller's loops resume. As before, using such a function in an expression
when no result was assigned is a runtime error; `CALL` does not require one.
`EXIT`/`CONTINUE` with no matching enclosing construct (including `EXIT FUNCTION`
outside a function) is a compile error reported with its line number.

**`CONST name = expression`.** The expression is evaluated once when the
statement runs, and the name cannot be reassigned (`LET`, `INPUT`, `FOR`,
`FOR EACH` and plain assignment are compile errors). Constants follow the usual
type suffix rules (`CONST NAME$ = "x"`). A constant declared in the main program
is visible everywhere; a constant declared inside a function is local to that
call (like `LOCAL`) and may shadow a global one. Redeclaring a constant in the
same scope, or reusing a parameter/local name, is a compile error. `CONST` must be
on its own line at the top level of the main program or function, not inside a
loop, block or inline `IF`. Constant checks are made in source order, so declare
a global constant before code that could assign it. Only the binding is
constant: elements of a constant list or map can still be changed.

**Delimiter errors.** Unmatched or misplaced delimiters (`END IF` without `IF`,
`ELSE` after `ELSE`, `CASE` outside `SELECT CASE`, `CASE` after `CASE ELSE`,
`LOOP` without `DO`, unterminated blocks, closers that do not match the innermost
block, and `FEND` inside an open block) are compile errors with the source line.
All block keywords must be on their own physical line.

See the runnable [control-flow example](../examples/control.bas).

## Standard library

Builtins come in two forms. **Function builtins** are used in expressions
(`ABS(X)`) and are only recognised when the name is followed by `(`; zero-argument
ones still need parentheses (`RND()`). Elsewhere the same names stay ordinary
variables, so legacy programs with `MAX = 5` or `TIMER = 0` keep working.
**Statement builtins** (`INSERT`, `SORT`, `REVERSE`, `RANDOMIZE`) are recognised
at the start of a statement unless the line is an assignment (`SORT = 1`,
`REVERSE[1] = 2` still assign). Precedence: a user `FUNCTION` of the same name
shadows a function builtin; statement builtins are not shadowed (a user function
of that name is reachable via `CALL`/expressions). Builtins are never reserved
words. `CALL POP(L)` discards a builtin result. Argument counts are checked at
compile time; types and values at run time. All results are finite numbers
(negative zero is normalised to `0`).

### Math

| Function | Meaning |
| --- | --- |
| `ABS(x)` | Absolute value. |
| `SQRT(x)`, `SQR(x)` | Square root; negative `x` is an error. |
| `MIN(a, b, ...)`, `MAX(a, b, ...)` | Smallest/largest of two or more numbers (numbers only). |
| `ROUND(x [, digits])` | Round half away from zero to `digits` decimals (default 0; negative rounds to tens, hundreds, ...; whole number from -308 to 308). Uses binary floating point, so `ROUND(1.005, 2)` is `1`. |
| `INT(x)` / `FIX(x)` | Floor (`INT(-2.5)` is `-3`) / truncate toward zero (`FIX(-2.5)` is `-2`). |
| `SIN(x)`, `COS(x)` | Radians. |
| `TAN(x)` | Tangent, radians. A non-finite result is an error. |
| `ATN(x)` | Arc tangent in radians, in `(-pi/2, pi/2)`. |
| `ATN2(y, x)` | Angle of the point `(x, y)` in radians, in `(-pi, pi]`; **`y` comes first**. `ATN2(0, 0)` is `0` (negative zeros count as `0`, so `ATN2(0, -1)` is `pi` and never `-pi`). |
| `PI()` | The constant pi (`3.141592653589793`). Needs the parentheses like other zero-argument builtins, so a variable named `PI` still works. |
| `LOG(x)`, `EXP(x)` | Natural logarithm (`x <= 0` is an error) and `e^x`. |
| `a ^ b` | Power; see precedence above. `0 ^ 0` is `1`. A negative base with a fractional exponent, `0 ^ negative`, or overflow is an error. |
| `RND()` | Pseudo-random number with `0 <= x < 1`. |
| `RANDOMIZE [seed]` | Reseeds `RND()`. The same numeric seed always gives the same sequence. Without a seed the clock is used. Before any `RANDOMIZE` the sequence is fixed and deterministic. |

The generator is SplitMix64 (state advanced by `0x9E3779B97F4A7C15`, standard
finalizer, top 53 bits divided by 2^53); a numeric seed is the IEEE-754 bit pattern
of the seed as the initial state. It is not cryptographically secure.
`MOD` is unchanged. Domain errors and any non-finite result stop the program
with a line-numbered error.

### Strings

Positions and lengths are **one-based and count Unicode scalar values**, exactly
like `LEN`; combining marks and emoji sequences are several scalars.
Counts and lengths must be non-negative whole numbers and start positions whole
numbers `>= 1`; anything else is an error. Lengths or positions beyond the end
are **clamped** (never an error).

| Function | Meaning |
| --- | --- |
| `LEFT$(s, n)`, `RIGHT$(s, n)` | First/last `n` characters (all of `s` if shorter). |
| `MID$(s, start [, n])` | `n` characters from `start`, or to the end; `""` when `start` is past the end. |
| `INSTR(s, find [, start])` | One-based position of the first `find` at or after `start`, or `0` if absent. An empty `find` matches at `start` (or `0` when `start` is past `LEN(s) + 1`). |
| `REPLACE$(s, find, with)` | Replace all non-overlapping occurrences left to right; empty `find` is an error. |
| `TRIM$(s)` | Remove leading/trailing Unicode whitespace. |
| `UCASE$(s)`, `LCASE$(s)` | Full Unicode case mapping, so the length can change (`"straße"` upper-cases to `STRASSE`). |
| `STR$(x)` | Number as text exactly as `PRINT` writes it (no leading space). |
| `VAL(s)` | Parse a number after trimming whitespace: optional sign, digits, optional fraction and `e` exponent (`.5`, `1e3`, `+7`). Anything else, including empty text, `inf`, `NaN`, hex or an overflowing value, is an **error** (not `0`). |
| `SPLIT(s, sep)` | New list of the pieces between non-overlapping occurrences of `sep`; adjacent separators give empty strings and `SPLIT("", ",")` is a list with one `""`. Empty `sep` is an error. |
| `JOIN$(list, sep)` | Concatenate an array's or list's strings with `sep`; any non-string element (or a map) is an error. |

### Collections

Mutators change the collection **in place**, so all aliases see the change
(`B = L : SORT L` sorts `B` too). Positions are one-based.

| Routine | Meaning |
| --- | --- |
| `INSERT list, i, value` | Insert before position `i`, `1 <= i <= LEN + 1`; lists only (arrays are fixed-length); otherwise an error. |
| `POP(list)` | Remove and return the last element; an empty list or a non-list is an error. |
| `SORT c` | Sort an array or list ascending in place. Elements must be all numbers or all strings (an empty collection is fine); mixed or nested elements are an error. The sort is stable; strings compare by Unicode code point (so `"B" < "a" < "é"`). Maps are an error. |
| `REVERSE c` | Reverse an array or list in place. |
| `CONTAINS(c, v)` | `1`/`0`: some array/list element equals `v` (numbers by value, strings by content, collections by identity; a different type simply does not match). For a map it tests the **values**, not the keys. |
| `HASKEY(m, key)` | `1`/`0`; `m` must be a map and `key` a string (otherwise an error). |
| `KEYS(m)`, `VALUES(m)` | New list of a map's keys / values in insertion order. A snapshot: later changes to the map do not affect it, but nested collections are shared, not copied. |

New lists are allocated in the managed heap and rooted while they are built, so
they are safe under garbage collection.

### Date, time, arguments and environment

| Function | Meaning |
| --- | --- |
| `TIMER()` | Seconds (fractional) elapsed since the VM was created; monotonic. |
| `UNIXTIME()` | Whole seconds since 1970-01-01 UTC. |
| `DATE$()`, `TIME$()`, `NOW$()` | UTC `YYYY-MM-DD`, `HH:MM:SS` and `YYYY-MM-DD HH:MM:SS`. Time zones are not supported. |
| `ARGC()` | Number of program arguments. |
| `ARG$(n)` | The `n`th argument, `1 <= n <= ARGC()`; out of range is an error. |
| `ENV$(name)` | Environment variable value, or `""` when unset. An empty name or one containing `=` or NUL, or a value that is not valid Unicode, is an error. |

On the command line everything after the program path is passed through:
`basic-vm prog.bas first second` gives `ARGC() = 2`, `ARG$(1) = "first"`.
Arguments must be valid Unicode. `--dump-bytecode` accepts no program arguments.
Embedders call `Vm::set_arguments`.

See the runnable [standard library example](../examples/stdlib.bas).

## File I/O

File numbers are integers 1 to 255, written `#n` in statements and plain `n`
in functions. Opening a number that is already open, using an unopened number,
using a mode the operation does not allow, a missing file, a permission
problem, invalid UTF-8 in text input and reading past the end of a file are
all runtime errors (catchable with `TRY`).

```basic
OPEN "data.txt" FOR OUTPUT AS #1   ' INPUT | OUTPUT (truncates) | APPEND | BINARY
PRINT #1, "alpha"; ","; 42
CLOSE #1                           ' CLOSE with no arguments closes every file
OPEN "data.txt" FOR INPUT AS #1
WHILE EOF(1) = 0
  LINE INPUT #1, L$                ' strips \n or \r\n
  PRINT L$
WEND
CLOSE #1
KILL "data.txt"
```

- `INPUT #n, var` reads one whole line: a numeric variable parses it as a
  number, a string variable behaves like `LINE INPUT`. Note: a `$` variable is
  required for `LINE INPUT`.
- `PRINT #n, ...` follows the console PRINT rules and works in OUTPUT, APPEND
  and BINARY modes.
- BINARY opens read/write, creating the file but not truncating it. `PUT #n,
  list` writes a list or array of whole numbers 0 to 255; `GET(n, count)`
  returns a list of at most `count` (maximum 65536) numbers, shorter or empty
  at the end of the file. `SEEK #n, offset`, `LOC(n)` and `LOF(n)` use
  zero-based byte offsets/sizes; `PUT`, `GET`, `SEEK` and `LOC` are BINARY only
  (`LOF` works in every mode; `EOF` only in INPUT and BINARY).
- `FREEFILE()` returns the lowest unused number, or raises an error if all 255
  are used. `ISOPEN(n)` is 1 or 0. `FILEEXISTS(path)` is 1 or 0. `KILL path`
  deletes a file and refuses to delete one this VM has open.
- Handles are external resources, independent of the GC. They are flushed and
  closed by `CLOSE`, and automatically when the program ends normally, at
  `END`/`STOP`, after an uncaught error (after any `FINALLY` blocks have run),
  and when the VM is dropped. Inside `FINALLY` use `IF ISOPEN(1) THEN CLOSE #1`,
  because closing an unopened number is an error.
- A path that is already open in this VM (compared by canonical path, falling
  back to the raw path when it cannot be canonicalized, e.g. `NUL`) cannot be
  opened again; this is an error, so a second `OUTPUT` open can never truncate
  a file that is still open. `CLOSE #a, #b` validates every number first (all
  must be open and distinct), so it is never partially applied. `SEEK` to an
  offset above 2^63-1 is an error.
- If BINARY cannot open a file read/write because of permission denied (for
  example a read-only file), it falls back to read-only: `GET`, `SEEK`, `LOC`
  and `LOF` work, and `PUT` raises a runtime error.
- Text files: a UTF-8 byte-order mark is kept as part of the first line's text
  (it is not stripped), and only `\n` or `\r\n` end a line; a lone `\r` is not
  a line break.
- Not provided: record/random access, `INPUT #` with several fields per line,
  text encodings other than UTF-8.

## Structured errors

```basic
TRY
  PRINT 1 / 0
CATCH E$          ' catch variable must be a string variable; optional
  PRINT "caught: "; E$
FINALLY
  PRINT "always"
END TRY
```

At least one of `CATCH` and `FINALLY` is required. `THROW expression` raises
an error whose message is the string value. A bare `THROW` inside a `CATCH`
rethrows the current error with its original line and function; elsewhere it is
a compile error.

- Every runtime error (division by zero, type errors, bounds, file errors,
  `THROW`, ...) is catchable. Unwinding drops called frames and their locals,
  discards loop and `FOR EACH` state, and truncates the operand stack to the
  `TRY` depth, so GC roots stay correct.
- `ERR$()` is the message, `ERRLINE()` the source line and `ERRFUNC$()` the
  function name (empty at top level) of the error being handled. They work in
  `CATCH`, in functions called from it, and in a `FINALLY` running because of an
  error; elsewhere they raise "no error is being handled".
- `FINALLY` runs after normal completion, after a caught error, when an
  error is uncaught (which then continues with its original message), and when
  `EXIT FOR/WHILE/DO/FUNCTION` or `CONTINUE` leaves the block. An error raised
  inside `CATCH` or `FINALLY` replaces the current one; `EXIT`/`CONTINUE`
  inside `FINALLY` abandons a pending error or exit.
- `END` and `STOP` halt immediately without running `FINALLY` (open files are
  still closed).
- `GOTO`/`IF ... THEN line` inside a `TRY` block, or to a label inside one, is
  a compile error; use structured control flow.
- Uncaught errors terminate with `line N: message (in function F)` on stderr
  and a nonzero exit code. There is no full stack trace and no
  `ON ERROR RESUME NEXT`.

Runnable example: [fileio.bas](../examples/fileio.bas).

## Label-based loops

```basic
COUNT = 1
100 PRINT "Count: "; COUNT
COUNT = COUNT + 1
IF COUNT <= 3 THEN 100
GC
END
```

This prints counts `1`, `2`, and `3`. Labels and jumps remain supported.

## Typed values, declarations and multi-dimensional arrays

Runnable example: [types.bas](../examples/types.bas). Programs that use none of
the syntax below behave exactly as before.

### Runtime types and the legacy rule

A value is a **double** (`f64`), an exact 64-bit **integer** (`i64`), a
**string**, or a collection (`LIST`, `MAP`, `ARRAY`). There is no separate
boolean value: comparisons, `TRUE` and `FALSE` stay the doubles `1` and `0`
(`TYPEOF` reports `"DOUBLE"`), and `AS BOOLEAN` stores `1`/`0` (any nonzero
number becomes `1`) and prints as `1`/`0`.

**Plain numeric literals and arithmetic stay doubles.** Integers only arise from:

- `123&` literals (`-9223372036854775808&` is accepted; a fraction or exponent
  before `&` is a compile error), and `&H1F` hex literals (1 to 16 digits,
  two's complement, so `&HFFFFFFFFFFFFFFFF` is `-1`);
- `INT64(x)`, `CINT(x)`, values stored in an `AS INTEGER` variable, `FOR`
  loops whose start is an integer, `INPUT` into an `INTEGER` variable, and
  `VAL` of a plain integer text whose magnitude exceeds 2^53.

Integer rules:

- `+`, `-`, `*`, `\`, `MOD` and unary `-` on two integers are exact; overflow is
  a runtime error (`integer overflow in +`). `MOD` by zero and `\` by zero are
  errors. `MOD` has the dividend's sign; `\` truncates toward zero.
  `MIN MOD -1` is `0`; `MIN \ -1` and `-MIN` and `ABS(MIN)` are overflow errors.
- `\` accepts integers or whole-valued doubles (`7 \ 2` is `3`); other operands
  are errors. `/` and `^` always produce doubles.
- An integer combined with a double is converted to a double first (lossy
  above 2^53, as for any double).
- Comparisons (and `MIN`, `MAX`, `SORT`, `CONTAINS`) between integers and
  doubles are **exact**: `9007199254740993& > 9007199254740992` is `1`.
- `PRINT`, `STR$` and `VAL` round-trip integers exactly.
- `CINT(x)` rounds half away from zero; `INT64(x)` requires an exact whole
  number or an exact decimal string; `CDBL`, `CBOOL` (`TRUE`/`FALSE` strings
  accepted) and `CSTR` convert as named. Conversion failures are errors.

### Declarations

```basic
OPTION EXPLICIT                       REM optional; must be the first statement
DIM N AS INTEGER = 5&                 REM scalar; AS INTEGER|LONG|DOUBLE|BOOLEAN|STRING|LIST|MAP|ARRAY|VARIANT
DIM S$ AS STRING                      REM STRING variables must end in $; the others must not
DIM V                                 REM no AS: VARIANT (STRING if the name ends in $)
DIM GRID(3, 4) AS INTEGER             REM multi-dimensional array, 1..3 by 1..4
REDIM [PRESERVE] GRID(5, 4)           REM resize
LOCAL X AS INTEGER = 1&               REM in a function
FUNCTION F(PARAMETER A AS INTEGER, PARAMETER B AS DOUBLE) AS INTEGER
FUNCTION G AS STRING                  REM result type without parameters
```

- `INTEGER` and `LONG` are both 64-bit. Defaults: `0`, `0.0`, `0`, `""`, empty
  collection, `0.0` (VARIANT). Without `= initial`, typed `LOCAL`/`DIM` in a
  function are initialised each time the statement runs. A global must be declared
  once; `CONST` names cannot be redeclared.
- Assignments, parameters, `INPUT` and function results are checked against the
  declared type: a double converts to `INTEGER` only if it is whole and in range;
  an integer converts to `DOUBLE`; a string never converts to a number; a
  collection must match its kind. Violations are runtime errors naming the variable.
- `VARIANT` (and untyped variables) stay dynamic. A `VARIANT` variable may hold a
  string even though its name has no `$`, which is how mixed lists are iterated.
  Untyped variables keep the legacy `$` rule.
- `DIM`, `REDIM`, `OPTION`, `PRESERVE` are not reserved: `DIM = 3` still works.
  Typed declarations take effect when executed.

### OPTION EXPLICIT

With `OPTION EXPLICIT` every variable must be declared **textually before** use,
otherwise the program fails to compile with the line number
(`undeclared variable X (OPTION EXPLICIT is active)`). Declarations are `DIM`,
`LOCAL`, `PARAMETER`, `CONST`, and the implicit declaration by `FOR`, `FOR EACH`,
`INPUT`, `LINE INPUT`, `INPUT #` and `CATCH` variables (global at top level,
function-local inside a function). Without it, nothing changes.

### TYPEOF and friends

`TYPEOF(x)` / `TYPENAME$(x)` return `"INTEGER"`, `"DOUBLE"`, `"STRING"`,
`"ARRAY"`, `"LIST"` or `"MAP"`; new kinds (e.g. records) will add names here.
`ISNUMERIC`, `ISINTEGER`, `ISSTRING`, `ISARRAY`, `ISLIST`, `ISMAP` return `1`/`0`.

### Multi-dimensional arrays

`DIM A(3, 4)` (up to 8 dimensions) creates an array of 3 row arrays of 4
elements; indices are one-based and `A[i, j]` is the same as `A[i][j]`. An
optional element type only chooses the fill value (`0`, `0.0`, `""`); elements
are not type-checked afterwards. Out-of-bounds indices, negative or fractional
sizes are runtime errors. Limits (checked for every `DIM`/`REDIM`, even when
another dimension is zero): each dimension at most 16777216, at most 16777216
elements in total, and at most 1048576 array objects (the array plus all its
rows, summed over all levels); exceeding any of them is an explicit "array is
too large" error. For example `DIM A(200000, 2)` is fine, while
`DIM B(2000000000, 0, 0)` fails immediately. `UBOUND(a [, dim])` is
the size of a dimension, `LBOUND(a [, dim])` is always `1`, and `LEN` is the
first dimension. `REDIM` resizes the top-level array **in place** (aliases see
it); without `PRESERVE` all elements are reset, with `PRESERVE` the overlapping
elements are kept. Rows replaced by `REDIM` are new objects.

### Map iteration

`FOR EACH k$, v IN map` iterates key and value in insertion order over a
snapshot (changes during the loop do not affect it). With one variable, `FOR
EACH` over a map still iterates values. The two-variable form requires a map.

## Records

```basic
TYPE Person
  name$ AS STRING = "anon"   ' STRING fields end in $, others must not
  age AS INTEGER             ' INTEGER LONG DOUBLE BOOLEAN STRING LIST MAP ARRAY VARIANT or a TYPE name
  boss AS Person             ' record fields hold references, default NOTHING
END TYPE

DIM p AS Person                       ' fresh instance with defaults
DIM q AS Person = NEW Person(name$ = "Bob", age = 29)
p.age = 3
q.boss = p
PRINT q.boss.name$; " "; q.boss IS NOTHING
```

- `TYPE Name` must be alone on its line at top level (so a variable named `TYPE`
  still works). Types may be used before their declaration. Defaults are
  literals, `TRUE`/`FALSE`, `NOTHING` or a global `CONST`; collections cannot
  have defaults. Field and type names are case-insensitive (reported upper case).
- Records are heap reference values: assignment aliases, `=`/`<>` compare identity.
  `COPY(r)` is a shallow clone (nested records/collections stay shared; `COPY`
  also shallow-copies lists, maps, arrays).
- Record-typed fields and `DIM a(n) AS Person` elements start as
  `NOTHING`. `NOTHING` is reserved; test with `x IS NOTHING` / `x IS NOT NOTHING`.
  Reading or writing a field of `NOTHING` is a runtime error.
- Field writes are checked with the typed-declaration rules (e.g. an INTEGER field
  rejects `1.5`). Unknown fields are compile errors when the variable's type is
  statically known (`DIM`/`LOCAL`/`PARAMETER`/function result `AS Person`),
  otherwise runtime errors. `LOCAL p AS Person`, `PARAMETER p AS Person` and
  `FUNCTION F AS Person` work.
- `TYPEOF(r)` is `"RECORD"`, `TYPENAME$(r)` the type name, `ISRECORD(x)`,
  `FIELDS(r)` a list of field names in declaration order, `GETFIELD(r, "name")`,
  `SETFIELD r, "name", v` (names case-insensitive).
- `PRINT`ing a record or `NOTHING` is an error. `INPUT`, `FOR` and `FOR EACH`
  targets must be plain variables, not fields.
- GC traces record fields, including cycles (`a.next = b : b.next = a` written on separate lines).
- Phase 5 (CLASS) builds on this: see [Objects](#objects-class). Each type is a
  `RecordType` in `Program::types()`; a class's `methods` hold `(name, function index)`
  and each method is an ordinary function whose first parameter `SELF` is typed
  `VarType::Record(id)`.

## Objects (`CLASS`)

An object is a record whose type also owns functions. Storage, field typing, `NEW`,
`COPY`, `TYPEOF` (`"RECORD"`), `TYPENAME$` (the class name), `ISRECORD`, aliasing and GC
(including cycles) are exactly those of records.

```basic
CLASS Account
  owner$ AS STRING
  balance AS DOUBLE = 0.0
  FUNCTION NEW(PARAMETER who$ AS STRING, PARAMETER opening AS DOUBLE = 0.0)   ' constructor
    SELF.owner$ = who$
    SELF.balance = opening
  FEND
  FUNCTION Deposit(PARAMETER amount AS DOUBLE) AS DOUBLE
    SELF.balance = SELF.balance + amount
    Deposit = SELF.balance
  FEND
  FUNCTION Reset()
    SELF.balance = 0.0
  FEND
END CLASS

DIM a AS Account = NEW Account("Ann", 10.0)
PRINT a.Deposit(5.0)
CALL a.Reset()            ' statement form (parentheses optional)
```

- `CLASS Name` must be the only thing on its line (so a variable named `CLASS` still
  works); the body holds field declarations (same syntax as `TYPE`) and
  `FUNCTION ... FEND` methods, which are compiled like functions (parameters, `BYREF`,
  optional/named arguments, `LOCAL`, `EXIT FUNCTION`, `TRY`, `AS` result types).
  `TYPE` records still have no methods.
- **`SELF`** is the receiver, an implicit first parameter of every method: it is a GC
  root while the method runs (the object survives even if the caller reassigns its
  variable), it cannot be assigned, and it can't be a method or parameter name. Outside
  methods `SELF` is an ordinary name. Fields and other methods are reached only through
  `SELF.field` / `SELF.Other(...)`; bare field names or bare method names inside a
  method are *not* implicit (consistent with the usual scoping).
- **Calls**: `obj.Method(args)` in expressions and `CALL obj.Method(args)` as a
  statement, chainable (`a.b.Method()`, `list[i].Method()`, `F().Method()`). When the
  receiver's class is statically known (typed variable, parameter, field, function
  result) the method is bound at compile time with full checking (arity, named
  arguments, `BYREF`). Otherwise (untyped variables, list/map elements) it is
  dispatched at run time by the receiver's class: positional arguments only and no
  `BYREF` parameters there (a runtime error says so). Calling a method on `NOTHING`
  or an unknown method (naming class and method) are runtime errors. Names are
  case-insensitive; a method may not share a name with a field or another method
  (no overloading) — compile errors.
- **Constructors**: a method named `NEW` (at most one, no result type). `NEW Class(args)`
  creates the object with fields defaulted, then runs the constructor with the
  arguments (positional/named/optional); its result is ignored and `NEW` returns the
  object. If the constructor raises an error the object is simply unreachable. A class
  without a constructor keeps `NEW Class(field = value, ...)`. `DIM x AS Class` and typed
  `LOCAL` run the constructor with no arguments; a constructor with required
  parameters makes that a compile error (use `= NEW Class(...)`). `obj.NEW()` cannot be
  called directly.
- **Modules**: classes are exported like types (`DIM a AS Alias.Class`); a method's
  module-private globals resolve to the module that defines it.
- **Diagnostics** name methods `Class.Method` (`in function ACCOUNT.DEPOSIT`).
- Not supported: `PRIVATE`/`PUBLIC`, inheritance, interfaces, overloading, bare field
  access, nested CLASS declarations. See `examples/objects.bas`.
- Worked example: `examples/lib/complexutil.bas` is a `Complex` class library
  (immutable value objects, module-level factory functions because there are no
  static methods, `THROW` for domain errors; string-returning methods end in `$`,
  e.g. `ToString$()`), used by `examples/complex.bas`.

## Parameter passing, optional and named arguments

```basic
FUNCTION SWAP(PARAMETER BYREF A AS INTEGER, PARAMETER BYREF B AS INTEGER)
  LOCAL T AS INTEGER
  T = A
  A = B
  B = T
FEND
FUNCTION SCALE(PARAMETER V, PARAMETER K = 3, PARAMETER OPTIONAL OFFSET)
  SCALE = V * K + OFFSET
FEND
PRINT SCALE(2)                  ' 6
PRINT SCALE(2, OFFSET := 1)     ' 7   named argument
PRINT SCALE(K := 5, V := 1)     ' 5   any order
```

Syntax: `PARAMETER [BYVAL | BYREF] [OPTIONAL] name [AS type] [= default]`. The
modifier words are contextual (a parameter may still be called `BYREF`).

- **`BYVAL`** (the default) passes the value. Numbers and strings are copied;
  collections and records pass their *reference*, so element/field mutations are
  visible to the caller, but assigning the parameter does not touch the caller.
- **`BYREF`** binds the caller's variable. The argument must be a variable
  (global, local, parameter, or `Alias.name`), a list/map/array element
  (`a[i]`, `m["k"]`, `a[i, j]`) or a record field (`p.x`, `a[1].x`); anything
  else (expressions, calls, literals, constants) is a compile error. A `BYREF`
  parameter cannot be `OPTIONAL` or have a default.
- **Semantics are copy-in/copy-out**: the value is copied into the parameter at
  the call; when the function returns normally (at `FEND` or `EXIT FUNCTION`)
  the parameter's final value is stored back into the caller's place, using the
  caller's normal assignment rules (declared `AS` type check and coercion; a
  failure is a runtime error at the return). Element and field places are
  evaluated once at call time. **If an error unwinds out of the function (caught
  by a `TRY` in a caller) nothing is written back**, even if the parameter had
  been assigned; an error handled *inside* the function does not prevent the
  write-back. While the call runs, the caller's variable keeps its old value
  (visible to globals read by the callee). Recursion works: each frame copies out
  to its own caller's place.
- **Optional parameters**: `= literal` (a number, `5&`, a string, `TRUE`, `FALSE`,
  `NOTHING`; the literal must fit the declared type) or `OPTIONAL` (the type's
  default: `0`, `""`, empty collection... for untyped parameters `0`, or `""` if
  the name ends in `$`). Optional parameters must trail the required ones.
  Defaults are evaluated at each call.
- **Named arguments** use `name := value` (`:=` cannot be confused with the
  comparison `=`). They may appear in any order, in `CALL` and expression calls;
  a positional argument after a named one, an unknown name, a duplicate (also a
  name that was already filled positionally) and a missing required parameter
  are compile errors (forward-declared, later and imported functions included).
  Arguments are evaluated left to right as written. Built-ins (and
  `LIST`/`MAP`/`ARRAY`/`LEN`) do not take named arguments.
- Arity errors are explicit: `function F requires 1 to 2 arguments, got 3`
  (`requires 2 arguments` when nothing is optional).

## Modules (`IMPORT`)

```basic
IMPORT "lib/mathutil.bas" AS Math     ' alias optional: default is the file stem, upper-cased
PRINT Math.Square(7); " "; Math.SCALE
CALL Math.Swap(a, b)
DIM p AS Math.Point = NEW Math.Point(x = 1, y = 2)
```

- `IMPORT "path" [AS Alias]` must be on its own line at the top level (not in a
  function, block or inline `IF`). The path is resolved relative to the
  *importing file's* directory (so it works from any working directory);
  absolute paths are also accepted; the path is used as written (no extension
  added, `..` allowed). A string compiled with `compile(&str)` has no file, so
  its imports resolve relative to the process's current directory;
  `compile_from(source, path)` / `compile_file(path)` resolve relative to `path`.
  The CLI uses the program file's directory.
- An imported module's `FUNCTION`s, `CONST`s, `TYPE`s and global variables are
  reached as `Alias.Name` (calls `Alias.F(...)`, `CALL Alias.F(...)`, reads and
  writes `Alias.x`, types `Alias.Person`, `NEW Alias.Person(...)`). Constants are
  read-only. Unknown members are compile errors. Aliases are not re-exported:
  a module's own imports are visible only inside that module.
- **Isolation**: a module's globals are private to it: its functions and
  top-level code see their own globals, never the importer's, and the importer
  sees the module's only through `Alias.Name`. `OPTION EXPLICIT`, constants,
  labels/line numbers and types are per module file.
- **Execution order**: imports are *hoisted*: all imports of a file are loaded
  (recursively, depth first, in source order) before that file's own code is
  emitted, so a module's top-level statements run exactly once, before the
  importer's code starts, in import order (not at the position of the `IMPORT`
  line). `END` in a module ends the whole program.
- **Once semantics**: a file is loaded once per canonical path, however many
  times or under whichever alias it is imported; all aliases share the instance
  (same globals). **Cyclic imports are an error** naming the chain:
  `import cycle: a.bas -> b.bas -> a.bas`.
- **Alias shadowing rule**: `Alias.Name` is resolved at compile time before
  record field access, so a module alias shadows a variable of the same name for
  `name.member`. Choose aliases that do not collide with record variables.
  Aliases cannot be keywords or type names, and must be unique within a file.
- **Diagnostics**: errors in the main file keep `line N: message`. Errors in an
  imported file (compile time and runtime) read `path:N: message`; a missing,
  unreadable or non-UTF-8 import reports `cannot import "x": reason` at the
  importing file's line. Runtime function names of imported functions are
  qualified, e.g. `(in function MATH.SQUARE)`, and `ERRFUNC$()` returns the
  qualified name.

## Unit testing

Programs can carry their own unit tests: `TEST` blocks plus `ASSERT*`
statements, run by `basic-vm --test`. Runnable example:
[testing.bas](../examples/testing.bas); a real library suite is at the end of
[lib/complexutil.bas](../examples/lib/complexutil.bas).

```basic
FUNCTION Clamp(PARAMETER v, PARAMETER lo, PARAMETER hi)
  IF v < lo THEN
    Clamp = lo
  ELSEIF v > hi THEN
    Clamp = hi
  ELSE
    Clamp = v
  END IF
FEND

TEST Clamp_limitsBothEnds          ' setup, execute, evaluate
  ASSERTEQ 0, Clamp(-5, 0, 10)
  ASSERTEQ 10, Clamp(15, 0, 10), "upper bound"
END TEST
```

### `TEST name` ... `END TEST`

- `name` is a single identifier (letters, digits, `_`; not a keyword, no `$`).
  Convention: `Function` or `Function_case`, e.g. `Add_handlesNegatives`. Names
  are case-insensitive and must be unique within a file.
- The block is only legal at the **top level** of a file (the main program or an
  imported module), on its own lines. Inside a `FUNCTION`, `CLASS`, `TYPE`,
  `TRY`, `IF`, a loop or another `TEST` it is a compile error, as are a
  duplicate name, a missing name, a missing `END TEST`, `END TEST`/`EXIT TEST`
  without a `TEST`, `FEND` closing a `TEST`, and `EXIT FUNCTION` inside a
  `TEST` (use `EXIT TEST`).
- A test is compiled as a hidden function without parameters or result, in its
  file's scope: it sees that file's functions, classes, types and constants
  unqualified (like a module's own functions), has its own `LOCAL` scope and
  its own labels (`GOTO`/line numbers cannot cross the `TEST` boundary), and can
  read and write the file's global variables. The body is ordinary code:
  setup, calls, `LOCAL`, any control flow, `TRY`/`CATCH`.
- `EXIT TEST` ends the test early; it counts as a pass. It is the only way to
  pass early: `END` and `STOP` inside a test (or in a function it calls) make
  the test an **ERROR** with the message `test ended by END` / `test ended by
  STOP`, because they would silently skip the remaining assertions. A `TRY`
  inside the test cannot catch them. In a normal run `END`/`STOP` are unchanged.
- A test **passes** when it ends without an uncaught error, **fails** when an
  assertion fails (or a file is leaked, see below), and **errors** on any other
  uncaught runtime error (`THROW`, division by zero, bounds, call depth limit,
  `END`/`STOP`, ...). There is no explicit result.
- In a normal run (`basic-vm prog.bas`) the `TEST` blocks are jumped over and
  have no effect at all.
- `TEST`, `ASSERT`, `ASSERTEQ` and `ASSERTNEAR` are recognised only at the
  start of a statement and only when no `=`, `[` or `.` follows, so they are not
  reserved: `TEST = 5`, `PRINT TEST`, `DIM TEST(2)`, `TEST[1] = 7` and `obj.ASSERT`
  keep working in older programs. (A line that is just `TEST` with no name is
  the "missing name" compile error.)

### Assertions

`ASSERT`, `ASSERTEQ` and `ASSERTNEAR` can be used anywhere (also in normal
programs). When they fail they raise a runtime error that `TRY`/`CATCH` can
catch; `CATCH e$` receives the message. Uncaught, the error carries the
statement's file and line like any other runtime error, and the name of the
function if it was raised inside a called function. Assertion failures are
flagged internally (`Error::assertion`), so the test report can tell a
**FAIL** from an **ERROR** without matching message text. The flag survives a
bare `THROW` rethrow and `FINALLY`; an error thrown from a `CATCH` is a new,
ordinary error.

| Statement | Passes when | Failure message |
| --- | --- | --- |
| `ASSERT cond [, msg$]` | `cond` is numeric and nonzero (same truth as `IF`) | `ASSERT failed` |
| `ASSERTEQ expected, actual [, msg$]` | the values are equal (below) | `ASSERTEQ failed: expected 5, got 6` |
| `ASSERTNEAR expected, actual, eps [, msg$]` | `ABS(expected - actual) <= eps` | `ASSERTNEAR failed: expected 1, got 1.5 (difference 0.5, allowed 0.1)` |

- An optional message (a string) is appended as `... - msg`, e.g.
  `ASSERTEQ failed: expected 5, got 6 - total after discount`. In the test
  report control characters in the message are escaped like the values are
  (`\n`, `\r`, `\t`, others as `\u{1b}`), so every result stays on one line;
  ordinary Unicode text is shown as is. The same applies to the text of an
  `ERROR` line (for example a `THROW` message with a newline). `CATCH e$` and
  `Error::message` keep the raw text.
- Argument counts are checked at compile time (`ASSERT` 1-2, `ASSERTEQ` 2-3,
  `ASSERTNEAR` 3-4). The arguments are a comma list, not a call: `ASSERTEQ(1, 1)`
  with parentheses around the whole list is a compile error (it reads `(1, 1)` as
  one expression); the message ends with a hint, `write ASSERTEQ expected, actual
  without parentheses around the argument list`. Write `ASSERTEQ 1, 1`.
  Parentheses inside an argument are fine: `ASSERT (a > b) AND c`, `ASSERT (a)`,
  `ASSERTEQ (a + 1) * 2, 6`. Wrong *types* are ordinary runtime errors, not assertion
  failures: a non-numeric `ASSERT` condition, a non-string message,
  non-numeric `ASSERTNEAR` operands, or an `eps` that is negative or not finite.
- **Equality for `ASSERTEQ`**: numbers by value, exactly as `=` compares them
  (`3 = 3.0`; an `INT64` is compared with a double without rounding, so
  `9007199254740993&` is *not* equal to `9007199254740992.0`); strings by
  content; booleans are `1`/`0`; `NOTHING` equals `NOTHING`. Lists and arrays
  are compared element by element (order matters; a list is not equal to an
  array). Maps compare keys and values, insertion order does not matter.
  Records compare by type and fields. Nested collections are compared deeply and
  cycle-safely; two references to the *same* collection or record are equal
  immediately, without walking it. Values of different kinds are simply unequal
  (a failure, never an error).
- **Message rendering**: strings are quoted (`"` doubled, `\n \r \t` escaped,
  cut off after 60 characters); collections are summarised as `LIST(1, 2, 3)`,
  `ARRAY(..)`, `MAP("k": v)` with at most 8 elements, nesting depth 3 (deeper
  levels print `LIST(...)`) and about 160 characters in total; records print
  as `TYPENAME(FIELD = value, ...)`.

### Running tests

```text
basic-vm --test [--test-filter text] [--test-imports] prog.bas
```

`--run-tests` is an alias of `--test`. All options come before the program path.

| Option | Meaning |
| --- | --- |
| `--test`, `--run-tests` | Run the unit tests of the program instead of the program. |
| `--test-filter text` | Only run tests whose full name contains `text` (case-insensitive). |
| `--test-imports` | Also run the tests declared in imported modules. Without it only the tests of the program file itself run. |

- The **main program body is not executed**, so programs that draw, loop or
  read input do not start. Imported modules' top-level initialisation runs as in
  a normal run (imports hoisted, once). Of the program file's own top level, only
  `CONST` and `DIM` statements are re-run before each test (so tests can use the
  file's constants and arrays); use literals, other constants or pure built-ins
  like `LIST()` as initialisers. Other top-level statements such as `x = 5` are
  *not* run, so a test that reads such a variable gets "undefined variable X".
- Running a module file directly (`basic-vm --test examples\lib\complexutil.bas`)
  runs that module's own tests, as it is the program file. Its tests are
  unqualified and its non-`CONST`/`DIM` top-level statements are not run.
- **Isolation**: every test runs in a *fresh VM* created from the compiled
  program, so globals, the heap, open files and the `RND` state (including any
  `RANDOMIZE` seed) are never shared between tests. Tests run in source order.
- stdin is empty (`INPUT` raises an error, so the test errors) and there are no program
  arguments (`ARGC()` is `0`). Graphics always use the in-memory headless
  backend, even when the code calls `SCREEN`; `SCREENSHOT "file.png"` still
  works. Output (`PRINT`, console) is captured and shown only when the test did
  not pass.
- **Captured output is capped** at 64 KiB per test (`basic_vm::MAX_CAPTURED_OUTPUT`):
  the first 64 KiB are kept and later writes are silently discarded (never an
  error; the test runs on normally). When the output of a non-passing test was
  cut, its listing ends with `    ... output truncated (N bytes omitted)`.
- **`SLEEP` returns immediately** in test mode (no real waiting, also with a
  screen open, where pending events are still serviced). `TIMER()` includes the
  skipped time, so `SLEEP 1000` advances it by 1 s and polling loops such as
  `WHILE TIMER() < t + 1: SLEEP 10: WEND` terminate at once. Other clocks
  (`UNIXTIME`, `NOW$`, `DATE$`, `TIME$`) are real time and are not adjusted.
- **Resource leaks**: a file still `OPEN` when a test ends is closed and makes
  an otherwise passing test fail with
  `resource leak: file #1 still open at the end of the test (CLOSE it)`
  (located at the test's header line). Use `TRY ... FINALLY CLOSE #n` in tests.

### Report and exit codes

One line per test on stdout, in source order: the program file's tests first,
then, with `--test-imports`, the modules' tests in import order. The report is
**streamed**: each result (and its indented output) is printed and flushed as
soon as that test finishes, followed by the summary, so progress is visible
while a long suite runs. Module tests are qualified with the alias as written in
the importing file (`Cx.Add_basic`). A module file is loaded once however often
it is imported: if the same file is imported under two aliases, its tests run
once, under the **first** alias. A numeric suffix is only added when a
*different* module file reuses an alias that is already taken, e.g. a nested
`IMPORT "n.bas" AS Inner` and a later top-level `IMPORT "m.bas" AS inner` give
`Inner.T2` and `inner#2.T1`. Tests of nested modules show their own alias
(`Inner.T2`), not the path through the importer. The location is
the path as given on the command line for the main file, or the module file's
resolved path for modules.

```text
FAIL Twice_basic: ASSERTEQ failed: expected 6, got 7 (f.bas:7)
    checking
PASS Twice_ok
ERROR Twice_boom: boom (f.bas:15)
3 tests: 1 passed, 1 failed, 1 errors
```

- `PASS name`, `FAIL name: message (file:line)`, `ERROR name: message
  (file:line)`. Failures inside a called function add `, in function NAME`
  (upper-case, qualified for modules) after the line. The captured output of
  tests that did not pass is printed indented by four spaces below their line.
- The last line is `N tests: P passed, F failed, E errors`.

| Exit code | Meaning |
| --- | --- |
| `0` | At least one test ran and all passed. |
| `1` | A test failed or errored; or compile/IO errors (`basic-vm: line N: ...` on stderr); or no tests were found (`basic-vm: no tests found` on stderr, also when the filter matches nothing). |
| `2` | Command-line misuse: `--test` together with `--dump-bytecode`, `--screenshot`, `--auto-close-ms`, `--max-frames` or `--headless-graphics`; program arguments after the path; a missing path; `--test-filter` without text; `--test-imports`/`--test-filter` without `--test`. Usage on stderr, nothing on stdout. |

Library API: `run_tests(&program, &TestOptions { filter, include_imports })`
(with `program` from `compile`/`compile_file`) returns a `TestReport` whose
`results` hold a `TestResult` (name, `TestStatus` Passed/Failed/Errored,
message, file, line, function, captured output, `output_omitted` bytes) per
test; `report.render(name)` produces the lines above, and `Program::tests()`
lists the compiled `TestCase`s. `run_tests_with(&program, &options, |result| ..)`
calls the closure as each test finishes (this is what the CLI uses to stream).

### Testing limitations

- Tests cannot mock or stub functions, files or input; there is **no per-test
  timeout**, so an endless loop in a test hangs the run (stop it with Ctrl+C;
  the results of the finished tests have already been printed). Unbounded
  *recursion* is not a hang: the [call depth limit](#functions-and-local-variables)
  turns it into an `ERROR`.
- The main program body is not run (see above); there is no `SETUP`/`TEARDOWN`
  or test grouping - share setup with a helper `FUNCTION`.
- State isolation is by fresh VM per test, which costs one program load (a
  clone of the compiled program) per test, including the imported modules'
  initialisation. This cost is not optimised.
- A UTF-8 byte order mark at the start of a source file is not handled
  specially.
- Report names show functions upper-cased; there is no stack trace beyond the
  failing function.

## Console, graphics and events

### Text console

Console statements write ANSI escape sequences to the normal output, so tests
capture them byte for byte. The CLI enables Windows VT processing when stdout is
a terminal. If `COLOR` was used, `ESC[0m` is written when the program ends.

| Statement | Output / behaviour |
| --- | --- |
| `CLS` | `ESC[2J ESC[H` |
| `COLOR fg [, bg]` | QBasic colours 0-15 (0-7 normal, 8-15 bright) as SGR codes |
| `LOCATE row [, col]` | `ESC[row;colH`, 1-based, column defaults to 1 |
| `SLEEP ms` | Pause 0..3,600,000 ms (via the key provider / graphics backend). Under `--test` it returns immediately and only advances `TIMER()` |
| `INKEY$()` | Next pending key or `""`; never blocks |

`INKEY$()` uses the `KeyProvider` trait (`poll_key`, `sleep`). The CLI's
`ConsoleKeys` uses `_kbhit/_getch` on Windows terminals; on other platforms (or
when stdin is not a terminal) it returns `""`. Tests use `ScriptedKeys`.
In graphics mode `INKEY$()` equals `GETKEY$()`. Parentheses are required on
zero-argument functions.

### Graphics

Drawing happens on a `Canvas` (`u32` pixels `0x00RRGGBB`) behind the
`GraphicsBackend` trait. `Vm::set_graphics_backend(Box<dyn GraphicsBackend>)`
installs one; `HeadlessBackend` records presented frames and replays scripted
events. Without a backend `SCREEN` fails with an explanatory error.

Colours: `0..15` select the QBasic VGA palette, `16..16777215` are packed RGB.
`RGB(r, g, b)` (each 0-255) returns the packed integer. Omitted colours use the
current `GCOLOR` (initially white, reset by `SCREEN`). `GCLS` defaults to black.

- **Palette ambiguity:** a packed value below 16 (for example `RGB(0, 0, 5)` = 5)
  is indistinguishable from a palette index and is drawn as palette colour 5,
  not as that RGB colour. Avoid `RGB(0, 0, n)` with `n < 16`.

Coordinates truncate toward zero and may be far outside the canvas: everything is
clipped and nothing panics. Non-numbers are errors. All drawing statements fail
with `no graphics screen` unless `SCREEN w, h` was executed.

| Statement | Meaning |
| --- | --- |
| `SCREEN w, h [, title$ [, buffers]]` | Open a w x h canvas (1..4096), reopening if needed. `buffers` is `1` (default, single-buffered) or `2` (double-buffered, see below); anything else is an error. `SCREEN 0` releases the screen and all its buffers (after the program has ended the last active canvas stays readable through `Vm::canvas()`). |
| `GCLS [color]` | Clear the canvas |
| `GCOLOR color` | Set the default colour |
| `PSET x, y [, color]` | Set one pixel |
| `LINE x1, y1, x2, y2 [, color]` | Bresenham line, inclusive endpoints |
| `LINE x1, y1, x2, y2 [, color], B` / `BF` | Box outline / filled box (corners in any order); omit colour with `, , B` |
| `CIRCLE x, y, r [, color [, FILL]]` | Midpoint circle, `r` in 0..1,000,000; `CIRCLE x, y, r, , FILL` uses the default colour |
| `TEXT x, y, text$ [, color]` | Built-in 8x8 font (ASCII 32-126, other characters draw `?`); (x, y) is the top-left of the first cell |
| `FLIP` / `PRESENT` | Present the frame and poll events (no handlers run). In double-buffered mode this publishes the active buffer and swaps the buffers (below) |
| `FLIP CLEAR` / `PRESENT CLEAR` | Like `FLIP`, then clear the new back buffer to black. Double-buffered mode only |
| `BUFFER n` | Direct drawing at buffer `n` (1 or 2) |
| `COPYBUFFER src, dst` | Copy the whole of buffer `src` onto buffer `dst` (same size, so no clipping) |
| `SCREENSHOT file$ [, VISIBLE]` | Save a buffer to an image file (see below) |

Functions: `POINT(x, y)` (packed colour, or -1 when outside the canvas),
`ISGRAPHICS()`, `WINDOWOPEN()`, `SCREENW()`, `SCREENH()`, `RGB(r,g,b)`,
`BUFFERS()` (1 or 2), `ACTIVEBUFFER()` and `VISIBLEBUFFER()` (1-based buffer numbers).
The buffer statements and functions fail with `no graphics screen` without `SCREEN`.

Geometry helpers (all buffers share one size; all need `SCREEN`):

| Function | Result |
|---|---|
| `WIDTH()` / `HEIGHT()` | Screen size in pixels (same as `SCREENW()` / `SCREENH()`) |
| `CENTERX()` / `CENTERY()` | The pixel at the middle: `INT(WIDTH() / 2)` and `INT(HEIGHT() / 2)` (so 320 for a 640-wide screen and 2 for a 5-wide one) |
| `CENTER()` | A new two-element list `[CENTERX(), CENTERY()]`, e.g. `c = CENTER()` then `CIRCLE c[1], c[2], 20` |
| `INSIDE(x, y)` | TRUE (1) when the pixel lies on the screen, else FALSE (0). Fractions are truncated like in `PSET`; it does not touch the pixel, so it works as a clipping test before `PSET`/`POINT` |

#### Presenting in single-buffered mode

Frames are presented automatically by `DOEVENTS`, `SLEEP` (when something
changed) and at the end of the run, as well as by `FLIP`. `BUFFER 1` and
`COPYBUFFER 1, 1` are accepted (and do nothing); buffer 2, `FLIP CLEAR`
and `SCREEN w, h, t$, 3` are explicit errors.

#### Double buffering

`SCREEN w, h, title$, 2` creates two same-size buffers numbered 1 and 2. The window
(backend) only ever shows the **visible** (front) buffer. Every drawing statement
(`GCLS`, `PSET`, `LINE`, `CIRCLE`, `TEXT`) and `POINT` and `SCREENSHOT` work on the
**active** (back) buffer, so a half-drawn frame is never displayed. At `SCREEN`, buffer 1
is visible and buffer 2 is active; both start black.

- `FLIP` presents the active buffer to the backend (it becomes the visible one) and then
  makes the *other* buffer the active one. This is a true page flip: **the new back buffer
  is not cleared**; it still holds the frame that was shown before, i.e. the frame from
  two flips ago. Animations normally start each frame with `GCLS`; use `FLIP CLEAR` to get
  a black back buffer instead (equivalent to `FLIP` followed by `GCLS 0`).
- `BUFFER n` sends drawing to buffer `n` (offscreen drawing). `BUFFER` with the visible
  buffer draws into what is on screen; the next `DOEVENTS`/`SLEEP`/end of run presents
  that change (only a changed visible buffer is presented). `FLIP` after `BUFFER 1` presents
  buffer 1 and then switches drawing to buffer 2.
- `COPYBUFFER src, dst` copies all pixels, e.g. `COPYBUFFER VISIBLEBUFFER(), ACTIVEBUFFER()`
  starts a frame from what is shown. `src = dst` is allowed and does nothing.
- `DOEVENTS`, `SLEEP` and the end of the run never present the hidden buffer: only `FLIP`
  publishes frames. The last flipped frame therefore stays the final presented frame
  (what `--screenshot` saves) even when a half-finished frame was being drawn.
- `--max-frames` counts presented frames, so in double-buffered mode it counts flips.
- `SCREEN 0` or reopening `SCREEN` releases both buffers, and the active/visible state is reset.

#### SCREENSHOT

`SCREENSHOT file$ [, VISIBLE]` writes the **active** buffer (what drawing currently
targets; in single-buffered mode that is the one canvas) to `file$`. `VISIBLE` saves
the front buffer, i.e. what the window currently shows (the same canvas in
single-buffered mode). It reads the framebuffer, not the screen, so it works
headless and with a real window, and it does not present or change anything.
The format follows the case-insensitive extension: `.ppm` (P6), `.bmp` (24-bit) or `.png`
(8-bit RGB, written with uncompressed deflate blocks, so the files are valid but large).
A missing or other extension is an error. An existing file is overwritten (the file is
written directly, no temporary file). A file that is currently `OPEN`ed by the program
is refused. Failures (missing directory, permission, ...) are normal runtime errors
that `TRY` can catch. `SCREENSHOT` and `BUFFER` stay usable as variable names.

```
SCREEN 160, 120, "Anim", 2
FOR F = 0 TO 59
    GCLS 1
    CIRCLE 10 + 2 * F, 40 + F, 8, 14, FILL
    FLIP
    DOEVENTS
    SLEEP 16
NEXT F
SCREENSHOT "last.png", VISIBLE
```

Backend additions (non-breaking): `GraphicsBackend::open_buffered(w, h, title, buffers)`
has a default implementation calling `open`, so existing backends keep working and
simply receive `present(&Canvas)` calls with the frame being published.
`HeadlessBackend::front_frame()` returns the last presented frame, `buffers()` the
requested buffer count, and `record_frames(limit)` keeps the last `limit` presented
frames for `frame_history()` (off by default). `ImageFormat` and `Canvas::to_png()` /
`encode(format)` are exported for embedding.

### Events

Events are queued by the backend and observed in one of two ways. Polling:
`MOUSEX()`, `MOUSEY()`, `MOUSEB()` (button bit mask), `KEYDOWN(name$)`
(case-insensitive), `GETKEY$()` (next queued key press or `""`) and `CLOSED()`
(1 once a window close was requested; nothing ends automatically, the program
decides). Key names are upper case: letters/digits, `ENTER`, `ESC`, `TAB`,
`BACKSPACE`, `SPACE`, `UP`, `DOWN`, `LEFT`, `RIGHT`, `HOME`, `END`, `PGUP`,
`PGDN`, `INS`, `DEL`, `F1`..`F12`; punctuation keys report the character itself
(`,` `.` `-` `=` `;` `/` `[` `]` `\` `'` `` ` ``). Number-pad digits report as the digit and the
number-pad Enter as `ENTER`; modifier and lock keys are not reported.

Handlers are plain `FUNCTION`s of the same file registered by name:

```
FUNCTION OnKey(PARAMETER K$)
    PRINT "key " + K$
FEND
ON KEY CALL OnKey          ' handler(key$)
ON MOUSE CALL OnMouse      ' handler(x, y, buttons)
ON CLOSE CALL OnClose      ' handler()
ON KEY OFF                 ' unregister (also ON MOUSE OFF, ON CLOSE OFF)
DOEVENTS
```

The function must exist and accept the required number of arguments (checked at
compile time). `DOEVENTS` presents if needed, polls the backend and calls the
handlers synchronously, in arrival order, for events pending at that moment
(no preemption). Events without a handler are dropped (key presses still reach
`GETKEY$()`). Handlers run as normal calls with their own frame; an error inside
a handler propagates from `DOEVENTS` and can be caught with `TRY`, discarding
the rest of that batch. `END` inside a handler ends the program. `DOEVENTS`
inside a handler is an error. `ON TIMER` is not implemented.

### Desktop window

Without `--headless-graphics`, `SCREEN w, h [, title$ [, buffers]]` opens a real window through the
`WindowBackend` (src/window.rs). It uses the [`minifb`](https://crates.io/crates/minifb)
crate: pure Rust, no C compiler, SDK or DLL to install, a plain pixel-buffer
API that matches the `Canvas` exactly (Win32, X11/Wayland and Cocoa). The window is
fixed-size, shown at scale 1 (one canvas pixel per screen pixel) and presented
on `FLIP`, `DOEVENTS` and `SLEEP`. `SLEEP` keeps servicing the window, so it
stays responsive; mouse positions are clamped to the canvas and the mouse event
buttons are 1 left, 2 right, 4 middle. Closing the window reports `CLOSED()` /
`ON CLOSE` once; the program decides when to end.

The window is released by `SCREEN 0`, when the program ends, when it stops with a
runtime error, and when the VM is dropped. Text output (`PRINT`, `COLOR`, `LOCATE`,
`CLS`) keeps going to the console while a window is open; the window only shows
what graphics statements (including `TEXT`) draw. If the window cannot be created
(no display, ...) `SCREEN` fails with a normal runtime error.

Cargo feature `window` (default on) pulls in `minifb`. `cargo build --no-default-features`
gives a headless-only binary where `SCREEN` without `--headless-graphics` reports that
no window backend is available. On Linux a running X11 or Wayland session is required;
the unattended-run options below help automated checks.

Examples: `examples/graphics.bas` (static scene), `examples/paint.bas`
(mouse and keyboard handlers) and `examples/animation.bas` (double-buffered,
flicker-free animation of 60 frames).

### Command line

`basic-vm [--dump-bytecode] [--headless-graphics] [--auto-close-ms n] [--max-frames n] [--screenshot file.ppm|file.bmp|file.png] program.bas [args]`
Flags must come before the program path. `--screenshot` implies
`--headless-graphics` and writes the last presented frame (PPM P6, 24-bit BMP or PNG,
chosen by the extension). In double-buffered mode that is the last flipped frame.
`--auto-close-ms n` / `--max-frames n` are bounded validation aids: once the graphics
screen has been open for n ms, or n frames were presented, every event poll also reports a
close request (`CLOSED()`, `ON CLOSE`). They work with the window and the headless
backend, but a program that ignores close requests still runs on, and they do nothing
for programs that never use `SCREEN`.

Limitations: no window resizing or scaling, no HiDPI handling beyond what the OS does,
key presses shorter than one poll can be missed, and keyboard layout specifics follow
`minifb` (physical key codes).

## Current limitations

There is no `GOSUB`, `ON ERROR RESUME NEXT`, random-access records, nested function definition,
or classes beyond the record-plus-methods form (no inheritance). Integers are 64-bit only (no 32-bit or
single types), boolean values are numeric `1`/`0`, and array element types are
not enforced after creation. Colon-separated statements are
not supported. Execution ends at `END`, `STOP`, or the end of the source;
infinite loops are not automatically interrupted (except by `--auto-close-ms` /
`--max-frames` for programs that react to close requests).

A record type imported through several aliases is a single type; `TYPENAME$`
and error messages name it by the alias that was loaded first, even when the
type is used through another alias.
