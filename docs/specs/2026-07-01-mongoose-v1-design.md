# mongoose v1 design

Status: approved 2026-07-01. Supersedes the ichor design entirely (removed at `fe59f69`).

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
big := nums.map(double).filter(fn(x) { x > 2 }).sum()
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

One binary. No pip, no venv, no PATH archaeology. Drives `uv` under the hood for all Python provisioning rather than reimplementing any of it.

```
mongoose new hello        # scaffold: mongoose.toml + src/main.mg
mongoose run              # typecheck + run
mongoose check            # typecheck only
mongoose py add torch     # declare a Python dep
mongoose repl
```

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

Deliberately tiny: `print`, `len`, string and list/map methods, file read/write, and an `http` module with `get`/`post`. Everything else is what the bridge is for; Python already has the batteries. Exact surface settled during implementation planning.

## Deliberately out of v1

- Concurrency. Sequential only; the bridge module is the single place GIL work will land when it comes.
- User-defined generics. Built-in `list` and `map` are generic; user types wait.
- Interfaces and traits.
- `mongoose fmt`. Needs a lossless formatter; v1.1.
- Package registry. Mongoose imports are files in your project.

## Open questions

None blocking. Deferred decisions (lambda shorthand syntax, struct methods, string interpolation details) get settled during implementation planning.
