# mongoose v1 design

Status: approved 2026-07-01. Supersedes the ichor design entirely (removed at `fe59f69`).

Rationale document. For exact language rules the normative reference is `language-spec.md` at the repo root; where details here have drifted (known: Ctx is also a reference type, `bool(x)` exists, `.pytype`/`.traceback` exist on every error, inbound conversions include `none` and `py` passthrough, all failures exit 1), the spec governs.

Interpreted language with Go's discipline and CPython's ecosystem. Named for the animal that hunts snakes and shrugs off venom. CLI is `mongoose`, source files are `.mg`.

Four Python failure modes drove the design, and each gets a structural fix, not a lint:

1. Runtime type surprises. Fixed by static typing, checked before anything runs.
2. Packaging and env hell. Fixed by a single toolchain binary that owns the entire Python environment.
3. Error handling chaos. Fixed by errors as values, no exceptions anywhere.
4. Silent weirdness (truthiness, aliasing, implicit coercion). Fixed by strict semantics and value semantics.

This exists elsewhere (Mojo, Nim, Julia). Building it anyway; the point is the language.

## Language surface

Go-shaped. Braces, `:=` inference on locals, explicit signatures on functions.

```
fn fetch(url: str) (str, error?) {
    resp, err := http.get(url)
    if err != none {
        return "", err
    }
    return resp.body, none
}

fn main() (error?) {
    body := check fetch("https://example.com")
    print(body)
    return none
}
```

### Types

`int`, `float`, `bool`, `str`, `list[T]`, `map[K, V]`, structs, `fn(...) ...` function types, `T?` options, `error` and `py`. That's the whole v1 zoo.

- No implicit conversions, not even `int` to `float`.
- No truthiness. Conditions are `bool` or they don't compile.
- Value semantics: assignment and argument passing copy. Naive deep copy in v1, copy-on-write later if profiling ever demands it. Kills the aliasing and mutable-default-arg class of bug outright. One exception, `py` values; see the bridge section.

### No nil, options instead

`nil` does not exist. A plain `User` is always a real `User`. Absence lives in the type: `User?` is a `User` or `none`. The compiler refuses field access on an unchecked option. The check is the unwrap, via flow typing:

```
u := users.find("guy")    // User?
if u != none {
    print(u.name)         // u is User inside this block
}
```

`none` is the empty-option literal and only compares against option types. Same mechanism TypeScript and Kotlin use; null bugs die in the checker.

### Errors

Fallible functions return `(T, error?)`. Three rules:

- Dropping an error is a compile error. Handle it or propagate it.
- `check expr` propagates: if the error slot is non-none, return it from the enclosing function; otherwise yield the success value. Keyword comes from the Go 2 error-handling draft. Not `try`; no exception connotations wanted.
- Recovery is the plain two-value form plus `if err != none`, with flow typing narrowing `err` to `error` inside the block.

No exceptions, no panic exposed to user code. A mongoose program cannot crash the process; the worst it can do is return an error from `main`.

### First-class functions

Functions are values. Anonymous fns close over surrounding variables by value, consistent with value semantics. Param types are inferred when the call site pins them, so chaining stays readable:

```
big := nums.map(double).filter(fn(x) { x > 2 }).sum()    // 18
```

Chaining comes from built-in methods on `list` and `map` (`map`, `filter`, `each`, `sum`, `sorted`, ...). No pipe operator.

## Python bridge

Python enters through one door and wears a uniform the whole time.

```
import py "torch"

fn main() (error?) {
    t := check torch.randn([2, 3])         // t: py
    y := check torch.relu(t)               // py stays py
    shape := check list[int](t.shape)      // crossing back is explicit
    print(shape)
    return none
}
```

- `import py "modname"` binds a Python module as a value of type `py`, the one dynamic type in the language. Attribute access, calls, indexing and operators on `py` dispatch to an embedded CPython.
- Every `py` expression is fallible as a unit. A chain like `model.forward(x)` evaluates to `(py, error?)`; the first Python exception becomes a mongoose `error` carrying exception type, message and traceback. Python's exception chaos converts to error discipline at the border, mechanically.
- Inbound conversion is automatic: `int`, `float`, `bool`, `str`, `list`, `map` convert to Python equivalents when passed to a `py` call.
- Outbound is explicit and fallible: results come back as `py`, and extraction is an assertion, `int(x)`, `str(x)`, `list[float](x)`, each returning `(T, error?)`.
- Caveat, stated plainly: `py` values are references to live Python objects. Value semantics stop at the bridge. A `py` tensor assigned to two variables is one tensor. The type tells you which rules a value plays by; weirdness is allowed but never disguised.

The checker can't see inside `py` values, but it guarantees the dynamic stuff never leaks into a `str` or a `User` without passing an explicit fallible conversion.

## Toolchain

Two binaries, split like uv and python. `mongoose` is the setup and project tool; `mg` is the runner. No pip, no venv, no PATH archaeology. `mongoose` drives `uv` under the hood for all Python provisioning rather than reimplementing any of it.

```
mongoose new hello        # scaffold: mongoose.toml + src/main.mg
mongoose py add torch     # declare a Python dep
mongoose check            # typecheck only
mongoose run              # typecheck + run the project entrypoint

mg script.mg              # run a file, python-style
mg                        # repl
```

Scripts are executable: a leading `#!/usr/bin/env mg` line is skipped by the lexer, so `chmod +x` works. Bare `mongoose run` and `mongoose check` resolve the nearest project's `src/main.mg`.

- `mongoose.toml` declares name, Python version pin and Python deps. `mongoose.lock` pins exact resolved versions. Both committed; together they fully determine the environment.
- The venv lives in a gitignored `.mongoose/`, created and repaired automatically on `run`. Delete it anytime; it regenerates.
- CPython itself is provisioned by uv as a standalone build matching the pin. System Python is never touched or trusted.
- `import py "x"` cross-checks the manifest at typecheck time. Undeclared import fails `mongoose check` with instructions to `mongoose py add x`.
- A project with no `import py` never provisions CPython at all.

## Interpreter

Rust. Four stages: lexer, parser, typechecker (with flow narrowing), tree-walking evaluator. No bytecode, no JIT; the heavy lifting in real programs happens inside PyTorch anyway. Bytecode VM is the recorded v2 path if pure-mongoose loops ever hurt.

The CPython bridge is one isolated Rust module wrapping PyO3. Nothing else in the interpreter knows Python exists; the evaluator sees a `py` value type with call, getattr and index operations returning mongoose values or mongoose errors. GIL handling lives in that one file for when concurrency arrives.

Interpreter panics are always interpreter bugs, never the program's fault.

## Testing

Golden files are the spec. A `tests/` tree of `.mg` programs, each paired with expected stdout or expected compile-error text, run by the harness under `cargo test`. Every language feature lands as golden tests first. Ordinary Rust unit tests cover lexer, parser and typechecker edges. Bridge tests run against a real pinned CPython in CI; no mocked Python.

## Stdlib

Small, not tiny. Python fills every other gap through the bridge.

### Builtins, no import

- `print(...)`, variadic, renders any value.
- `printf(format, ...)` and `sprintf(format, ...) str` with Go verbs: `%v` `%d` `%s` `%f` `%t` `%q` `%%`, width and precision (`%.2f`). No `fmt` module.
- `len(x)` for `str`, `list`, `map`.
- The fallible conversions already specced: `int()`, `str()`, `float()`, `list[T]()`.

### Modules

Stdlib modules import by bare name (`import "http"`); project files import by path. `import py` stays its own form.

- **http.** Client only. `http.get(ctx, url)` and `http.post(ctx, url, body)` return `(Response, error?)`; `http.request(ctx, Request)` for full control of method and headers. `Response` is a plain struct: `status int`, `body str`, `headers map[str, str]`. Follows redirects, honors ctx deadline.
- **file.** `read(path) (str, error?)`, `write(path, s) error?`, `append(path, s) error?`, `exists(path) bool`, `list(dir) (list[str], error?)`, `remove(path) error?`, `mkdir(path) error?`. Paths are `str`.
- **math.** `abs`, `min`, `max`, `sqrt`, `pow`, `floor`, `ceil`, `round`, constants `pi` and `e`.
- **ctx.** Sequential v1, so a ctx is a deadline plus an interrupt flag, no cross-task cancellation yet. `ctx.background()`, `ctx.timeout(parent, secs)`, `ctx.interrupt(parent)` which cancels on SIGINT so Ctrl-C surfaces as a normal error through `check` chains instead of killing the process. I/O ops take ctx as first arg (http in v1, file later). Methods: `done() bool`, `err() error?`.
- **error.** `error.new(msg)`, `error.wrap(err, msg)`. Available without an import (constructing errors is core to the language; `import "error"` stays legal and does nothing extra). Error values expose `.msg str` and `.cause error?`; bridge errors add `.pytype` and `.traceback`. Chain-walking helpers wait for real usage.

### Known hole

No `bytes` type in the v1 zoo, so `file` and `http` bodies are utf-8 `str` and binary I/O is out until v1.1. Adding bytes touches literals, indexing and conversions; nothing in the day-one workload needs it (tensors live on the Python side as `py` values).

## Surface details

The mundane stuff, pinned so the implementation plan has exact answers.

- Comments: `//`.
- Control flow: `if` / `else if` / `else`, conditions strictly `bool`. One loop keyword: `for x in xs`, `for k, v in m`, `for cond { }`, bare `for { }` infinite. `break` and `continue`. `range(n)` and `range(a, b)` are builtins returning `list[int]`.
- Operators: `+ - * / %` on `int`; `+ - * /` on `float`; `+` concatenates `str` and `list`; `== !=` on comparable values; `< <= > >=` on `int`, `float`, `str`; `&& || !` on `bool`. No mixed `int`/`float` arithmetic.
- Structs: `struct User { name: str, age: int }`, literal `User{name: "guy", age: 44}`, dot access. No methods on user structs in v1.
- Indexing: `xs[i]` (fault if out of bounds), `xs[i] = v`, slices `xs[a:b]`. Map read `m[k]` returns `V?` (missing key is `none`, flow typing applies); `m[k] = v` inserts or updates. Map iteration and `keys()` follow insertion order.
- String methods: `split`, `trim`, `contains`, `replace`, `upper`, `lower`, `starts_with`, `ends_with`. List methods: `map`, `filter`, `each`, `sum`, `sorted`, `sorted_by`, `append`, `contains`, `join` (on `list[str]`). Map methods: `keys`, `values`, `has`, `delete`.
- Runtime faults (division by zero, integer overflow, index out of bounds, recursion past 1000 frames): not catchable in v1. The interpreter stops the program and reports the fault with a mongoose stack trace, exiting nonzero. Still never a process crash or a Rust panic. The parser likewise bounds expression nesting (256 levels) rather than crash on hostile input.
- Recursive structs must break the cycle with an option (`next: Node?`), list, or map; a by-value cycle is a compile error (such a value could never be constructed).
- A bare `[]` with no surrounding type context is a compile error; in a typed position (argument, return, field, assignment to a declared list) it infers fine.
- Concatenating `list[T] + list[T?]` widens to `list[T?]`, never the reverse.
- `contains` compares structurally (lists, structs, maps, recursively). The `==` operator stays scalar-only in v1.
- `printf`/`sprintf` with a literal format string are verb-checked at compile time (`%d` on a str is a compile error); dynamic formats check at runtime.
- The zero value of `py` (what a failed `check` destructure leaves behind) is Python's `None`, so touching it yields a normal Python error value, not a fault.
- The repl (`mg` with no file) is unchecked in v1: lines go straight to the evaluator, faults are reported and survived.

## Deliberately out of v1

- Concurrency. Sequential only; the bridge module is the single place GIL work will land when it comes.
- User-defined generics. Built-in `list` and `map` are generic; user types wait.
- Interfaces and traits.
- `mongoose fmt`. Needs a lossless formatter; v1.1.
- Package registry. Mongoose imports are files in your project.

## Open questions

None blocking. Deferred decisions (lambda shorthand syntax, struct methods, string interpolation details) get settled during implementation planning.
