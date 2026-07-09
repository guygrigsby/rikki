<img src="art/logo.png" alt="Rikki the mongoose" width="200" align="right">

# Rikki: A New ML Language
I have been doing some experiments training and tuning language models. All the good stuff to do that is in Python. I got so sick of Python just core dumping all over the place, I literally made a new "front end" for it.

> What would happen if I vibe-coded a language based solely on my hatred of Python?

Enter Rikki.

It's basically a wrapper around Python with proper error handling and typing. While I was at it, I decided to add some ML constructs as first class citizens.

```rikki
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

- Statically typed, whole program checked before any of it runs. A program that passes the checker cannot crash the process. Worst case is an error returned from `main` or a controlled runtime fault with a rikki stack and a nonzero exit. No panics, no core dumps.
- Errors are values and handling is mandatory. `check` propagates, `v, err :=` handles locally, silently dropping one is a compile error.
- Option types (`T?`) instead of nil, with flow narrowing: `if err != none` gives you the narrowed value in that branch.
- Go's copy model. Scalars, strings, and structs copy; lists, maps, functions, and py handles are references. Closures capture by reference.
- Embedded CPython, not a subprocess. `import py "torch"` binds the real module. A chain of Python operations is one fallible unit: `check model(x).loss.item()` yields the value or the Python exception converted to a rikki error, with no per-step ceremony. Keyword args pass through (`optim.Adam(params, lr: 0.001)`), `for range` works over any Python iterable, and you can assign into Python attributes and subscripts.
- ML sugar: `@` is matrix multiplication, dispatched to `__matmul__`.
- Small stdlib: `error`, `math`, `file`, `ctx` (cancellation handles: deadlines and SIGINT), `http`.

## Two binaries

Split like uv and python. `rikki` does setup: `rikki new`, `rikki py add torch`, `rikki check`, `rikki run`. `tk` runs code: `tk train.rk`, and bare `tk` is the repl. Python deps live in the project manifest and every `import py` is validated against it at compile time, so a missing dep is a compile error, not a stack trace twenty minutes into a training run.

## Where things live

`language-spec.md` is the normative spec. `tests/golden/` is the executable spec; every language-visible behavior has a golden test. Design rationale is in `docs/specs/`, decisions in `docs/adr/`. There's an nvim plugin under `editors/` with syntax highlighting and check-on-save.
