# Introduction

rikki is a statically typed, interpreted language: Go's discipline,
CPython's ecosystem. It exists because training models means living in
Python, and Python core dumps have no manners.

The whole program is checked before any of it runs. A program that passes
the checker cannot crash the process: the worst outcomes are an error
returned from `main` or a controlled runtime fault with a rikki stack
trace. Errors are values and handling them is mandatory. Option types
replace nil. The copy model is Go's. And `import py "torch"` embeds real
CPython, so the entire python ecosystem is one import away, with Python's
exceptions arriving as rikki error values instead of tracebacks.

Try it in [the playground](https://rikki.aeryx.ai/) without installing
anything, or install it:

```sh
uv tool install rikki-lang      # or: brew install guygrigsby/tap/rikki
rikki new hello && cd hello
rikki run
```

Two binaries, split like uv and python: `rikki` does setup (`new`,
`py add`, `check`, `fmt`, `run`), `tk` runs code (`tk file.rk`; bare `tk`
is the repl).

This book is the guide. The normative reference is
[the language spec](https://github.com/guygrigsby/rikki/blob/main/language-spec.md),
and every behavior in it is pinned by
[golden tests](https://github.com/guygrigsby/rikki/tree/main/tests/golden).
