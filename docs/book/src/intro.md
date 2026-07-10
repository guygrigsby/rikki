# Introduction

nevla exists because training and tuning language models means living in
Python, and Python kept core dumping mid-run. Everything good in ML is a
python library; everything painful about those runs — the crash twenty
minutes in, the typo that survives until the epoch ends, the exception
that ate the metrics — is the language around the libraries. nevla is a
new language around the same libraries: Go's discipline, CPython's
ecosystem.

```nevla
import py "torch"

// check propagates: the caller decides
fn logits(n int) (str, error?) {
    w := check torch.randn([784, 10], requires_grad: true)
    x := check torch.randn([n, 784])
    y := check (x @ w)
    return check str(y.shape), none
}

// main can act, so it handles
fn main() {
    shape, err := logits(32)
    if err != none {
        print("torch failed: " + err.msg)
        return
    }
    print("logits: " + shape)
}
```

That is real torch, one import away, with Python's exceptions arriving
as typed error values instead of tracebacks.

## The tenets

These are the principles the language is built on, each with its reason.
They are recorded as decisions in the repo's
[ADRs](https://github.com/guygrigsby/nevla/tree/main/docs/adr); this is
the reader's digest.

**The whole program is checked before any of it runs, and a checked
program cannot crash the process.** The worst outcomes are an error
returned from `main` or a controlled runtime fault with a nevla stack
trace. The reason is the twenty-minute crash: a training run should die
at `nevla check`, in milliseconds, or not at all.

**Errors are values, and handling them is mandatory.** Dropping an error
is a compile error; `check` propagates, `v, err :=` handles. There is no
exception control flow to forget about. Handle errors at the layer that
can act; propagate only when the caller owns the decision.

**Everything is data.** Errors carry inspectable fields (including the
`file:line` where they were born). A test is a fallible function whose
outcome is an error value. A test table is a list of structs. Faults are
the single deliberate exception — process death refuses to be a value,
which is exactly what makes the no-crash guarantee provable.

**Follow the Go way unless there is a compelling reason not to.** The
copy model, capitalization visibility, `_test` files seeing their
module's internals, one true format with no configuration — all Go,
adopted wholesale. Deviations exist (option types instead of nil, a
lowercase stdlib) and each one has its reason written down.

**Python is the ecosystem, not the runtime.** One file in the
interpreter speaks to CPython. A chain of Python operations is one
fallible unit. Dependencies are declared in the manifest or the program
does not compile — a missing package is a compile error, not a stack
trace at hour two.

**Break early.** Until the language has users to protect, fundamental
improvements ship now, not after they calcify. The version stamp in
every project says what it was built against, and that is the promise:
honesty about change, not absence of change.

**Nothing is remembered.** Anything derived is generated or enforced by
a test: these docs' reference chapters render from the spec, every
example in this book compiles in CI, the dependency lock fingerprints
its manifest, releases flow from a git tag to PyPI and Homebrew without
hands. If keeping two things in sync requires a human to remember,
that's a bug.

nevla is built primarily for its author and the agents that write most
of its code — and it would be worth building even if it never gains
another user. That freedom is why the tenets above can be held without
compromise.

## Getting started

Try it in [the playground](https://nevla.dev/play/) without installing
anything, or install it:

```sh
uv tool install nevla      # or: brew install guygrigsby/tap/nevla
nevla new hello && cd hello
nevla run
```

You don't need python installed first: uv fetches a managed CPython for
the install, and the wheels carry their own libpython. The py bridge
does need a matching CPython standard library at runtime, which the uv
and brew installs both guarantee; a hand-rolled setup missing one gets a
warning naming the fix (`uv python install <version>`).

Two binaries, split like uv and python: `nevla` does setup (`new`,
`py add`, `check`, `fmt`, `test`, `run`), `nv` runs code (`nv file.nv`;
bare `nv` is the repl).

This book is the guide. The normative reference is
[the language spec](https://github.com/guygrigsby/nevla/blob/main/language-spec.md),
and every behavior in it is pinned by
[golden tests](https://github.com/guygrigsby/nevla/tree/main/tests/golden).
