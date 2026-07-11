# Writing nevla (agent primer)

nevla is a statically typed, interpreted language: Go's discipline,
CPython's ecosystem. Files end in `.nv`. Run with `nv file.nv`; typecheck
with `nevla check` (no argument inside a project). The whole program is
checked before any of it runs. Trust the checker's messages; they name the
fix.

## Shape of a program

```nevla
import "error"
import py "torch"

struct Config {
    lr float
    steps int
}

fn train(cfg Config) (float, error?) {
    w := check torch.randn([784, 10], requires_grad: true)
    opt := check torch.optim.SGD([w], lr: cfg.lr)
    loss := 0.0
    for i := range cfg.steps {
        with torch.no_grad() {
            check w.clamp_(-1.0, 1.0)
        }
        check opt.step()
        loss = check float(torch.rand([1]).item())
    }
    return loss, none
}

fn main() (error?) {
    loss, err := train(Config{lr: 0.01, steps: 3})
    if err != none {
        return error.wrap(err, "training failed")
    }
    printf("loss %.4f\n", loss)
    return none
}
```

## Syntax deltas from Go

- `:=` declares, `=` assigns. No `var`, no semicolons, one statement per line.
- One loop keyword: `for { }`, `for cond { }`, `for i, v := range xs { }`.
  `range` covers int, lists, maps, strings, and py iterables.
- No parens around conditions. `if x != none { } else if ... { } else { }`.
- Lists are `[]T` with literals `[1, 2, 3]` or `[]int{}`; maps are
  `map[K]V` with literals `map[str]int{"a": 1}`.
- Option types `T?` replace nil. `none` is the absent value.
  `if x != none` narrows `x` to `T` inside the branch.
- Struct literals must name every field: `User{name: "g", age: 44}`.
- No ternary, no `while`, no `++`. String concat is `+`.

## Errors (the part that is not Python)

Errors are values; dropping one is a compile error. A fallible function
ends its result list with `error?`:

- Handle: `v, err := f()` then `if err != none { ... }`.
- Propagate: `v := check f()` returns the error from the enclosing
  function, which must itself return `error?`.
- A bare fallible call as a statement must be wrapped: `check f()`.
- Make errors with `error.new("msg")`, wrap with
  `error.wrap(err, "context")`, read `err.msg`.
- Prefer handling at the layer that can act; do not blanket-`check`
  everything up to main.

## The py bridge (the part that is not Go)

`import py "torch"` binds the real module as a `py` value. Any chain of
Python operations (`model(x).loss.item()`) is ONE fallible unit typed
`(py, error?)` at the point of consumption, and it must be consumed by
exactly one of:

1. `check`: `y := check model(x).loss.item()`
2. a two-name destructure: `y, err := model(x)`
3. a conversion, which absorbs the fallibility: `n := check int(obj["k"])`

`x := some_py_call()` (single name, no check) and a bare py call statement
are compile errors. Other bridge rules:

- Keyword args pass through: `optim.Adam(params, lr: 0.001)`.
- `@` is matrix multiplication; `check (x @ w)` (parens: `check` binds
  tighter than binary operators).
- Comparisons on py values yield py, not bool. A py value cannot be a
  condition; extract first: `if check bool(x > 0) { }`.
- `for i, item := range loader { }` iterates any Python iterable.
- Assignment into py targets works and faults on exception:
  `param.requires_grad = false`, `batch["labels"] = y`.
- `with expr { }` runs a Python context manager (`torch.no_grad()`);
  exceptions in enter/exit fault. Acquire fallibly before the statement.
- Convert py -> nevla with `int(x)`, `float(x)`, `bool(x)`, `str(x)`,
  `[]int(x)` etc; all fallible, so `check` or destructure them.
- In a project, every `import py` module must be declared:
  `nevla py add torch`. Python stdlib modules need no declaration.

## Copy model

Scalars, strings, structs, and tuples copy on assignment and calls.
Lists, maps, functions, and py values are references. Closures capture by
reference.

## Gotchas, in the order they will bite

1. `append` is pure: `xs = append(xs, v)`, never bare `append(xs, v)`.
2. Map reads return `V?`: `v := m["k"]` needs a none-check before use.
3. Multi-values cannot be stored or nested: consume `(a, b, error?)` at
   the call site with destructure or `check`.
4. `==` on lists, maps, and structs is a compile error in v1; the
   `contains` method compares structurally.
5. `check` needs the enclosing function (or lambda) to return `error?`;
   the checker will say so.
6. Strings are immutable; `s[i] = ...` faults. Index yields a one-char
   `str` per character; slices are `s[lo:hi]`.

## Builtins and stdlib

Builtins: `print`, `printf`/`sprintf` (`%v` for anything, `%.4f` etc),
`len`, `append`, `clone` (one-level copy of a list or map),
`charcode`/`char` (code point of a character and back). Lists have
`map`/`filter`/`sum` and friends; maps have `keys`/`values`/`delete`
(spec 14.9 lists all). Program argv and stdin live in the `os` module,
not builtins.

Stdlib modules (plain `import "name"`): `error`, `math` (`abs`, `min`,
`max`, `sqrt`, `pow`, `exp`, `ln`, `log`, trig, `floor`/`ceil`/`round`,
`pi`, `e`), `file` (read/write/append/exists/list/remove/mkdir/glob/
modified, all fallible), `ctx` (`background`, `timeout`, `interrupt`
cancellation handles), `http` (`get`/`post`/`request`/`stream`, all
take a `Ctx`), `os` (`workdir`, `env` returning an option, `args`,
`readline`), `time` (int nanoseconds everywhere: `now`, `clock`,
ctx-aware `sleep`, `parts`, and the duration constants
`time.second` etc.), `regex` (`compile` to a `Re` handle;
`matches`/`find`/`find_all`/`replace`; RE2 flavor, no backtracking),
`flag` (`value`/`toggle`/`parse`/`get`, help is an error value carrying
the usage text). Durations are always `int` nanoseconds written with
the constants: `ctx.timeout(c, 30 * time.second)`.

Multi-file: `import "util.nv"` binds the sibling file as module `util`.
Only Capitalized top-level names (functions, structs, fields) are visible
across modules, Go's rule; lowercase is private to its file.

The normative reference is `language-spec.md` in the nevla repo;
`tests/golden/` there is a corpus of small correct programs.
