# Language Spec (working draft)

> Status: design pass complete. This document captures the initial design
> session plus a second session that resolved every open question blocking
> implementation. It is the starting point for a first implementation
> (lexer → parser → AST → type checker → tree-walking interpreter).
>
> Working codename: *(undecided — `sanguine` used informally for the CLI tooling;
> candidate names in `possible-names.md`)*
> Reference program to eventually build in the language: **eviscerOS**.

---

## 1. Design philosophy

Two guiding principles:

**Readability first.** Thematic / flavorful keywords are used only where they
carry real semantic weight (lifecycle, concurrency, type sealing, death).
Ordinary control flow stays plain and boring, because there is no readability
upside to renaming a loop and real cost in confusing every future reader.

- **Themed keywords** are reserved for concepts where the word adds information:
  `genesis` (type birth), `spawn` (concurrent stream process), `cide`
  (process death), `entomb` (sealing a type), `clean` (absence of error),
  `Vein` (stream type).
- **Plain keywords** for everything universal: `if`, `else`, `for`, `do`,
  `return`, `let`, `var`, `fn`.
- **Minimal punctuation.** No colons anywhere. Type annotations are positional
  (`name Type`). Function signatures have no separators and no `->` return arrow.

**The program cannot crash.** There is no program-level panic and no invisible
control flow. Functions are total: their signatures state every way they can
fail. Death exists only at process granularity (§6) — a `spawn` stage can die,
its stream closes, the pipeline drains, and the organism survives.

The language is **statically typed** with **explicit type annotations**
(no type inference in v1), **nominally typed** (two structurally-identical types
are still distinct types), and **garbage collected**.

---

## 2. Type system

Locked decisions:

- **Explicit type annotations.** Every binding and every function parameter and
  return position carries an explicit type. No inference in v1. This includes
  each binding in a multi-return destructure (§3.2).
- **Nominal typing.** Type identity is by name, not shape.
- **Garbage collected.** No manual memory management, no ownership/borrow
  checker in v1. (Manual memory / ownership is explicitly deferred as a possible
  "second language" or future major version.)

### 2.1 Primitive types

- `Int` — 64-bit integer
- `Float` — 64-bit float
- `Str` — string
- `Bool` — boolean

No sized variants (`Int32` etc.) and no `Char` in v1; indexing a `Str` yields a
`Str`. Sized integers are a codegen-era concern a tree-walking interpreter does
not have.

There is no unit/void type: a function that returns nothing omits the return
type slot entirely (§3).

### 2.2 Bindings

```
let count Int = 0      // immutable binding
var counter Int = 0    // mutable binding
```

- `let` — immutable. This is the encouraged default.
- `var` — mutable. Explicit opt-in.
- Syntax is positional: `let NAME TYPE = EXPR`. No colon.

> **NOTE:** Full immutability (no `var` at all) was explored and deliberately
> deferred as too large a leap for v1. It remains a possible future direction.

### 2.3 Struct / type definitions — `genesis`

`genesis` defines a new named type (a struct). Fields are positional
(`name Type`), one per line, no colons, no trailing separators required.

```
genesis Vessel {
    name Str
    capacity Int
    sealed Bool
}
```

- `genesis` creates the *kind* (the type).
- `let` / `var` create *instances* (values) of that kind.

**Construction** mirrors declaration exactly: braced field-value pairs by
juxtaposition, order-independent.

```
let v Vessel = Vessel {
    name "cup"
    capacity 100
    sealed false
}

let w Vessel = Vessel { sealed false, name "jug", capacity 500 }
```

Field access and (for `var` bindings) field mutation use dot syntax:

```
v.capacity = v.capacity - amount   // legal when `v` is a mutable binding
```

Spread / functional-update syntax was explored and cut from v1; direct field
mutation on `var` bindings is the v1 approach.

### 2.4 Collections

Three built-in generic collections, all using the `<>` syntax (§2.6):

- `Array<T>` — growable ordered sequence. Literal `[1, 2, 3]`. Indexing
  `xs[i]`, `xs.len()`, iteration with `for`.
- `Set<T>` — unordered unique values. Literal `{1, 2, 3}`.
- `Map<K, V>` — key/value. Literal uses juxtaposition, consistent with struct
  construction: `{ "a" 1, "b" 2 }`.

An empty literal `{}` (or `[]`) is unambiguous because every binding carries an
explicit type:

```
let nums Array<Int> = [1, 2, 3]
let seen Set<Str> = {}
let ages Map<Str, Int> = { "ada" 36, "linus" 55 }
```

**Map access is comma-ok** (a domain failure, so it is a value — §6):

```
let age Int, ok Bool = ages["ada"]
```

**Array indexing out of bounds is a machine fault** (§6.3): it kills the
enclosing process. Prefer `for x in xs` and bounds-known indexing.

### 2.5 Sealed types — `entomb`

`entomb` marks a type as sealed: **constructing a new instance is a compile
error**. Existing values still flow — they can be passed, read, and (via `var`)
mutated. The type is dead, not erased.

```
entomb LegacyVessel { }

fn handle(v LegacyVessel) Str {
    return v.name              // fine: reading the dead
}

// let v LegacyVessel = LegacyVessel { ... }   // compile error: entombed
```

When extension mechanisms exist someday, `entomb` will forbid those too.

### 2.6 Generics — built-ins only in v1

Angle-bracket syntax: `Type<Param>`. Angle brackets are *delimiters* (like
parens and braces, which the language keeps), not *separators* (like `:` and
`->`, which the language bans).

The parser understands `<>` in all type positions, but in v1 **only built-in
types are generic**: `Vein<T>`, `Array<T>`, `Set<T>`, `Map<K, V>`, plus the
non-type parameter in `spawn<N>` / `spawn<auto>` (§4.5).

User-defined generic functions and generic `genesis` types are deferred —
that is the full constraints/bounds/instantiation design, postponed intact.

### 2.7 `ossify` — reserved

`ossify` is reserved as a keyword with **no v1 meaning**. Using it anywhere is
a syntax error. Likely future meaning: compile-time constants
(`ossify MAX_CAPACITY Int = 1000`), decided when that feature is designed.
Reserving now is free insurance; re-adding a keyword later breaks user code.

---

## 3. Functions — `fn`

Functions are call/return. Fully positional signatures: no colons, no `->`
return arrow. The return type sits in a fixed grammatical slot immediately
after the parameter list.

```
fn fill(v Vessel, amount Int) Vessel {
    v.capacity = v.capacity - amount
    return v
}
```

- Parameters: `name Type`, comma-separated, inside parens.
- Return type: positional, after the closing paren.
- **Returns nothing = omit the slot.** No unit type, no ceremony:

```
fn log(msg Str) {
    // no return type slot: returns nothing
}
```

**Functions cannot die.** `cide` inside a `fn` is a compile error (the same
rule shape as `emit` inside a `fn`). A function's only failure channel is
multi-return (§3.2). A `fn` signature is therefore the whole truth: if it does
not return an `Err`, it cannot fail. See §6.

### 3.1 Multi-return

Functions may return multiple values, Go-style. Multi-return is a **function
signature feature only** — tuples are not first-class types (no tuple bindings,
fields, or parameters).

```
fn divide(a Int, b Int) (Int, Err) {
    if b == 0 {
        return 0, Err("division by zero")
    }
    return a / b, clean
}
```

### 3.2 Destructuring

Call-site destructuring annotates **each** binding — the no-inference rule has
no exceptions:

```
let result Int, err Err = divide(10, 0)
if err != clean {
    return 0, err
}
```

> **NOTE:** with mandatory annotations, the error-forwarding ladder gets heavy.
> A propagation sugar (a `?`-alike) is a likely future addition; it is additive
> and deferred.

### 3.3 Closures / lambdas

A lambda is an anonymous `fn` — identical syntax minus the name. Zero new
tokens, and `return` works identically everywhere. There is no `=>` form in v1
(it can be added later without breakage, and it would sit uncomfortably close
to the `->` reactive arrow).

```
let square = fn(x Int) Int { return x * x }

let doubled = nums.map(fn(n Int) Int { return n * 2 })
let total = nums.fold(0, fn(acc Int, n Int) Int {
    return acc + n
})
```

---

## 4. Concurrency — `spawn` (CSP)

This is the heart of the language and the most-designed part. The model is
**Communicating Sequential Processes** (Hoare), with **implicit channels** and
**no manual channel wiring**.

### 4.1 What a process is (and why it's not a function)

A `fn` is called once, runs top to bottom, and `return`s a single value.

A `spawn` process is structurally different: it is a **stream transducer** that
consumes an input stream and produces an output stream, reacting to each input
item as it arrives, over the process's whole lifetime. It cannot be called
synchronously and cannot `return` a value — it `emit`s zero or more values per
input item. That asymmetry is why `spawn` is its own keyword rather than a
modifier on `fn`.

Processes are also the language's unit of mortality: a process can die
(`cide`, or a machine fault); a function cannot. See §6.

### 4.2 Streams — `Vein<T>`

The stream type is `Vein<T>`: a stream of `T` flowing between processes.
Themed deliberately — streams are the signature concept of the language, which
is exactly where the design philosophy says a themed word earns its place.

```
spawn double(input Vein<Int>) Vein<Int> {
    input -> item {
        emit item * 2
    }
}
```

- Signature mirrors `fn`: name, positional params, positional return type. The
  input and output types are stream types.
- `input -> item { ... }` — the **reactive body**. `input` is the parameter
  name from the signature; `item` is a user-chosen binding for each element
  drawn from the stream. The block runs per item, implicitly looping for the
  process's lifetime, and ends when the input stream closes. (`->` here is the
  reactive-body arrow, a distinct construct — not a return separator.)
- `emit EXPR` — pushes a value into the output stream. **Only legal inside a
  `spawn` body.** `emit` inside a plain `fn` is a compile error (a function has
  no output stream).

### 4.3 Variable emit count — the key expressiveness feature

An input item may `emit` **zero, one, or many** times. This is what makes
`spawn` a full stream transducer rather than a 1:1 map:

- **Zero emits = filter** (drop the item):
  ```
  spawn evens(input Vein<Int>) Vein<Int> {
      input -> item {
          if item % 2 == 0 {
              emit item
          }
      }
  }
  ```
- **Many emits = expand / flatten**:
  ```
  spawn repeat(input Vein<Int>) Vein<Int> {
      input -> item {
          emit item
          emit item
      }
  }
  ```
- **State across items = scan / fold / batch / dedupe** (see §4.5 for the
  concurrency constraint that makes this safe or unsafe):
  ```
  spawn runningTotal(input Vein<Int>) Vein<Int> {
      var sum Int = 0
      input -> item {
          sum = sum + item
          emit sum
      }
  }
  ```

Together these give filter, map, flatMap, fold/scan, batching, and windowing
from one primitive.

### 4.4 Channels

- **Implicit.** The programmer does not declare or wire channels by hand.
- **Unbuffered / rendezvous** by default (true CSP): a send blocks until a
  receive is ready. (Buffered channels were considered and deliberately not
  included in v1 — one-in, one-out.)
- **Auto-close:** a process's output stream closes automatically when the
  process ends — whether from the input stream closing and draining, or from
  the process dying (§6). There is no manual `close()`.

### 4.5 Parallelism — sequential by default, opt-in scaling

**Default `spawn` is sequential and ordered:** items are processed one at a
time, in order, and the process has exclusive access to its own internal `var`
state. This is what makes stateful operators (§4.3, `runningTotal`) safe by
default.

Parallelism is **opt-in and explicit** via a worker count in angle brackets:

```
spawn double(input Vein<Int>) Vein<Int> { ... }        // sequential (default), ordered, state-safe
spawn<8> double(input Vein<Int>) Vein<Int> { ... }      // fixed: 8 parallel workers
spawn<auto> double(input Vein<Int>) Vein<Int> { ... }   // scheduler chooses the worker count
```

- `spawn<8>` — fixed degree of parallelism.
- `spawn<auto>` — runtime/scheduler decides the degree.
- Bare `spawn` — sequential.

`auto` (not `n`) is used for the scheduler-chosen keyword specifically so that
`n` stays free as a documentation metavariable (docs can say "`spawn<n>` where
`n` is your worker count" without `n` being real syntax).

**Parallel processes are unordered.** `spawn<N>` / `spawn<auto>` emit results
as workers finish; input→output order is not preserved. No reordering buffers,
no head-of-line blocking on a slow item. Bare `spawn` already provides ordered
processing when order matters; a reordering stage can live in the standard
library later.

> **CONSTRAINT (enforced by the type checker):** carrying mutable cross-item
> state (a `var` updated across items, like `sum` above) is a **compile
> error** inside a parallel process (`spawn<N>` / `spawn<auto>`), because
> parallel workers would race on it and ordering is not guaranteed. It is only
> legal in a sequential (bare `spawn`) process. This is the central safety rule
> of the concurrency model.

### 4.6 Composition — pipelines

Processes compose into pipelines with `|>`, which feeds one process's output
stream into the next's input stream. Channels between stages are created
implicitly by the runtime.

```
source |> double |> double |> sink
```

This is the Unix-pipe mental model and the payoff of implicit channels: no
stage declares or wires a single channel by hand.

### 4.7 Sources and sinks — structural

Pipeline ends fall out of existing rules; no new keywords or types:

- **Source** — a `spawn` with no input parameter. Its body runs once (no
  reactive arrow required), emits whatever it likes, and its stream auto-closes
  when the body ends.
- **Sink** — a `spawn` with no return slot (the omit-the-slot rule from §3
  applied to streams). It consumes its input and emits nothing.

```
spawn nums() Vein<Int> {          // source: no input
    for i in 0..100 {
        emit i
    }
}

spawn show(input Vein<Int>) {     // sink: no output slot
    input -> item {
        print(item)
    }
}

nums |> double |> show            // a complete, runnable pipeline
```

A pipeline expression ending in a sink is a **runnable statement**: executing
it runs the pipeline to completion (all streams drained and closed) before the
next statement.

> **DEFERRED:** fan-out / fan-in helpers (splitting one stream across N
> processes and merging results) belong in the **standard library**, built
> from `spawn` + pipeline primitives, once those are implemented.

---

## 5. Control flow

All plain, all unthemed, all standard semantics.

- `if` / `else` — conditional.
- `for` — iteration (`for item in xs { ... }`, `for i in 0..10 { ... }`).
- `do` / `while` — post-condition loop (`do { ... } while (cond)`), runs at
  least once. **Note:** the standalone `while` loop was dropped; only
  `do`/`while` remains.
- `return` — return from a function.

**Ranges are exclusive:** `0..10` yields 0 through 9. `0..xs.len()` walks a
whole collection with no off-by-one. An inclusive spelling can be added later
if real programs beg for it.

---

## 6. Errors and death

Design principle (§1): **signatures tell the whole truth, and the program
cannot crash.** There is no program-level panic and no try/catch. The model
splits failures by kind:

| Failure kind | Mechanism | Example |
|---|---|---|
| Domain failure (expected) | values: multi-return `(T, Err)`, comma-ok | parse error, missing key |
| Deliberate process death | `cide` (spawn-only) | poisoned input, violated invariant |
| Machine fault | process death (implicit) | divide by zero, index out of bounds |

### 6.1 Domain failures are values

Expected failures travel through return values, never control flow. The
built-in nominal type `Err` carries a message (`err.msg Str`); its absence is
the keyword literal **`clean`**. Only `Err` has an absence value — no general
nil exists in the language.

```
fn divide(a Int, b Int) (Int, Err) {
    if b == 0 {
        return 0, Err("division by zero")
    }
    return a / b, clean
}

let result Int, err Err = divide(10, 2)
if err != clean {
    return 0, err
}
```

Map lookup is comma-ok (`let v Int, ok Bool = m[k]`) — same principle,
no message needed.

### 6.2 `cide` — apoptosis, spawn-only

`cide` is deliberate process death. It is **only legal inside a `spawn` body**
(compile error inside a `fn` — the same scoping rule as `emit`). The process
dies, its output stream auto-closes (§4.4), downstream stages see end-of-stream
and drain.

```
spawn parse(input Vein<Str>) Vein<Config> {
    input -> item {
        let cfg Config, err Err = parseConfig(item)
        if err != clean {
            cide(err.msg)      // this stage dies; the organism survives
        }
        emit cfg
    }
}
```

**Functions cannot die.** A `fn` that detects a violated invariant must become
fallible and return an `Err` — that is the whole-truth property doing its job:
the fallibility appears in the signature, where callers can see it.

### 6.3 Machine faults kill the process

Integer division by zero and array indexing out of bounds are **machine
faults**: they kill the enclosing process, exactly as if it had `cide`d. Not
silently-defined values (Pony's `1/0 == 0` propagates silent corruption
through a dataflow program), not a program crash. A machine fault inside a
`fn` kills whichever process called it — the same honesty compromise every
"total" language already makes for OOM and stack overflow.

### 6.4 The program cannot crash

Top-level code is the **root process**. When any stage dies, its stream
closes, the pipeline drains, and execution continues or ends per the pipeline
structure. **V1 default policy: drain-and-report** — the program finishes what
can be finished, then reports which process died and why, and exits cleanly.

> **DEFERRED:** supervision — restart policies, backoff, dead-letter streams —
> is a future design layered on these semantics (Erlang-style, at the pipeline
> level). The v1 drain-and-report default is forward-compatible with it.

---

## 7. Full keyword reference

| Keyword     | Category      | Meaning                                                        | Status      |
|-------------|---------------|----------------------------------------------------------------|-------------|
| `genesis`   | themed        | Define a new named type (struct)                               | locked      |
| `let`       | plain         | Immutable binding (default)                                    | locked      |
| `var`       | plain         | Mutable binding                                                | locked      |
| `fn`        | plain         | Function definition (call/return); anonymous form is the lambda| locked      |
| `spawn`     | themed        | CSP stream process; `<N>` / `<auto>` for parallelism           | locked      |
| `emit`      | themed        | Push a value to a process's output stream (spawn-only)         | locked      |
| `entomb`    | themed        | Seal a type: no new instances (compile error)                  | locked      |
| `cide`      | themed        | Deliberate process death / apoptosis (spawn-only)              | locked      |
| `clean`     | themed        | The no-error value of the built-in `Err` type                  | locked      |
| `if`/`else` | plain         | Conditional                                                    | locked      |
| `for`       | plain         | Iteration                                                      | locked      |
| `do`/`while`| plain         | Post-condition loop                                            | locked      |
| `return`    | plain         | Return from a function                                         | locked      |
| `auto`      | plain         | Scheduler-chosen worker count (`spawn<auto>`)                  | locked      |
| `ossify`    | themed        | Reserved; no v1 meaning (likely: compile-time consts)          | **reserved**|

Built-in types: `Int`, `Float`, `Str`, `Bool`, `Err`, `Vein<T>`, `Array<T>`,
`Set<T>`, `Map<K, V>`.

Operators / punctuation:

| Token   | Meaning                                                        |
|---------|----------------------------------------------------------------|
| `<>`    | Generics (built-ins only in v1); `spawn` worker count          |
| `->`    | Reactive-body arrow in a `spawn` process (`input -> item`)     |
| `\|>`   | Pipeline composition between processes                         |
| `..`    | Range, exclusive upper bound (`0..10` = 0–9)                   |
| `=`     | Assignment / binding                                           |
| `[]`    | Array literal / indexing                                       |
| `{}`    | Blocks; struct construction; Set and Map literals              |

**No `:` anywhere. No `->` as a function return separator. No `=>` in v1.**

---

## 8. Decision log

Resolved (second design session, 2026-07-01):

1. **Struct construction** — braced field-value by juxtaposition, mirroring
   declaration; order-independent (§2.3).
2. **Stream type** — `Vein<T>` (§4.2).
3. **Primitives** — `Int`, `Float`, `Str`, `Bool`, all 64-bit where sized (§2.1).
4. **Returns nothing** — omit the return-type slot (§3).
5. **Closures** — anonymous `fn`; no `=>` (§3.3).
6. **Ranges** — exclusive upper bound (§5).
7. **Parallel ordering** — `spawn<N>`/`spawn<auto>` unordered; bare `spawn`
   ordered (§4.5).
8. **Error model** — signatures tell the whole truth; the program cannot
   crash. Domain failures are values (`(T, Err)` multi-return, `clean`,
   comma-ok); `cide` is spawn-only apoptosis; machine faults kill the
   enclosing process; v1 dead-stage policy is drain-and-report (§6).
9. **Multi-return** — signature feature only; no first-class tuples;
   destructuring annotates each binding (§3.1–3.2).
10. **Collections** — `Array<T>` (growable), `Set<T>`, `Map<K, V>`;
    colon-free literals (§2.4).
11. **Pipeline ends** — structural: no-input spawn = source, no-output spawn
    = sink; sink-terminated pipeline is a runnable statement (§4.7).
12. **entomb** — no new instances; existing values still usable (§2.5).
13. **ossify** — reserved, no v1 meaning (§2.7).
14. **Generics** — built-ins only in v1 (§2.6).

Deferred (recorded, not blocking):

- Supervision / restart policies for dead stages (§6.4).
- Error-propagation sugar (a `?`-alike) for the forwarding ladder (§3.2).
- User-defined generic functions and types; bounds (§2.6).
- Fan-out / fan-in stdlib helpers (§4.7).
- `=>` expression-bodied lambda sugar (§3.3).
- Buffered channels (§4.4).
- Full immutability (§2.2).
- Spread / functional-update syntax (§2.3).
- Inclusive range spelling (§5).
- `ossify` as compile-time constants (§2.7).

---

## 9. Suggested implementation path

Standard, and matches the interest in lexers/parsers/ASTs:

1. **Lexer** — tokenize the keyword set (§7) plus literals, identifiers,
   operators. Hand-written recommended over a generator for a first pass.
2. **Parser** — recursive descent; Pratt parsing for expression precedence.
3. **AST** — node types per construct (`GenesisNode`, `SpawnNode`, `FnNode`,
   etc.). This is where the design lives structurally.
4. **Type checker** — nominal, explicit. Enforces the scoping and safety rules:
   - `emit` only inside `spawn` (§4.2)
   - `cide` only inside `spawn` (§6.2)
   - no mutable cross-item state in parallel processes (§4.5)
   - no construction of entombed types (§2.5)
   - every binding annotated; multi-return arity and types match (§3)
5. **Tree-walking interpreter** — fastest path to running programs. Defer any
   bytecode VM / real codegen and the CSP scheduler's true parallelism until
   the sequential semantics work end to end.

Reference reading: *Crafting Interpreters* (Nystrom) for the lexer→parser→
tree-walking-interpreter path; *Writing an Interpreter in Go* (Ball) for a
code-forward companion.

> **Implementation note on concurrency:** get the **sequential** `spawn`
> semantics (ordered, single worker, stateful) working first as an ordinary
> in-order stream fold. Add real parallel workers (`spawn<N>` / `spawn<auto>`)
> and the scheduler only after that foundation runs — parallelism is the last
> layer, not the first.
