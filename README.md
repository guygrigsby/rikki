# Nevla: A New ML Language

<img src="art/logo.png" alt="Nevla the mongoose" width="200" align="right">

I have been doing some experiments training and tuning language models. All the good stuff to do that is in Python. I got so sick of Python just core dumping all over the place, I literally made a new "front end" for it.

> What would happen if I vibe-coded a language based solely on my hatred of Python?

Enter Nevla.

It's basically a wrapper around Python with proper error handling and typing. While I was at it, I decided to add some ML constructs as first class citizens.

```nevla
import py "torch"

fn main() (error?) {
    w := check torch.randn([784, 10], requires_grad: true)
    x := check torch.randn([32, 784])
    logits := check (x @ w)
    print(check str(logits.shape))
    return none
}
```

## What you get

- Statically typed, whole program checked before any of it runs. A program that passes the checker cannot crash the process. Worst case is an error returned from `main` or a controlled runtime fault with a nevla stack and a nonzero exit. No panics, no core dumps.
- Errors are values and handling is mandatory. `check` propagates, `v, err :=` handles locally, silently dropping one is a compile error. You *can* still avoid ever dealing with an error by growing `(error?)` on every function and `check`ing your way up to `main`, and that is strongly recommended against: it moves every failure to the top with no context and no recovery. Handle errors at the layer that can do something about them; propagate only when the caller owns the decision.
- Option types (`T?`) instead of nil, with flow narrowing: `if err != none` gives you the narrowed value in that branch.
- Go's copy model. Scalars, strings, and structs copy; lists, maps, functions, and py handles are references. Closures capture by reference.
- Embedded CPython, not a subprocess. `import py "torch"` binds the real module. A chain of Python operations is one fallible unit: `check model(x).loss.item()` yields the value or the Python exception converted to a nevla error, with no per-step ceremony. Keyword args pass through (`optim.Adam(params, lr: 0.001)`), `for range` works over any Python iterable, and you can assign into Python attributes and subscripts.
- ML sugar: `@` is matrix multiplication, dispatched to `__matmul__`.
- Small stdlib: `error`, `math`, `file`, `ctx` (cancellation handles: deadlines and SIGINT), `http`.

## Two binaries

Split like uv and python. `nevla` does setup: `nevla new`, `nevla py add torch`, `nevla check`, `nevla run`. `nv` runs code: `nv train.nv`, and bare `nv` is the repl. Python deps live in the project manifest and every `import py` is validated against it at compile time, so a missing dep is a compile error, not a stack trace twenty minutes into a training run.

Try it without installing anything: [the playground](https://nevla.aeryx.ai/) runs the interpreter in your browser (the py bridge needs a real CPython, so that part is native-only). [The nevla book](https://nevla.aeryx.ai/book/) is the guide.

## Getting started

nevla ships as a python wheel carrying both binaries, so [uv](https://docs.astral.sh/uv/) is the whole story:

```sh
uv tool install nevla-lang
nevla new hello && cd hello
nevla run                 # hello, nevla
nevla py add numpy        # declare a Python dep; uv builds .nevla/venv
nv src/main.nv            # run a file directly; bare nv is the repl
```

Homebrew works too, same wheels underneath: `brew install guygrigsby/tap/nevla`.

New projects come with `AGENTS.md`, a nevla primer for coding agents; `nevla new --claude-hook` also installs a Claude Code hook that typechecks after every edit. `nevla new` only ever writes into the directory it creates; it refuses to run where anything already exists.

## Developing

The gate is `NEVLA_TEST_PY=1 cargo test`, green before every commit (the py goldens need a `python3` on PATH). Language behavior lives in `tests/golden/`: a `.nv` file next to a `.out` (expected stdout) or `.err` (expected error substrings), and a directory with a `main.nv` is one multi-file case. Any change to language semantics updates `language-spec.md` in the same commit, no exceptions. `nevla fmt` rewrites source in the one true style (`--check` for CI). The front end has a fuzz target, `cargo +nightly fuzz run parse_check`, and CI runs the full gate plus a 60 second fuzz pass on every push.

## Where things live

`language-spec.md` is the normative spec. `tests/golden/` is the executable spec; every language-visible behavior has a golden test. Design rationale is in `docs/specs/`, decisions in `docs/adr/`. There's an nvim plugin under `editors/` with syntax highlighting and check-on-save.

## The name

Nevla (नेवला) is Hindi for mongoose. The project was briefly named for
Kipling's Rikki-Tikki-Tavi and renamed once the story's colonial
subtext was pointed at directly; [ADR 0014](docs/adr/0014-rename-to-nevla.md)
and [the book's mascot page](docs/book/src/mascot.md) give the full
account. Same purple mongoose: the animal was never the
problem.
