# 2. Embedded CPython via PyO3, provisioned by uv

Status: accepted

## Context

The point of the language is full access to the Python ecosystem including C extensions (PyTorch). C extensions require a real CPython in-process; reimplementing or wire-bridging the ecosystem is not viable. Packaging pain (pip, venvs, system Python drift) is one of the four problems the language exists to kill.

## Decision

Embed CPython through PyO3, isolated in a single bridge module (`src/bridge.rs`); nothing else in the interpreter names pyo3. The toolchain provisions a standalone CPython and a hidden per-project venv by driving `uv`. `mongoose.toml` plus `mongoose.lock` fully determine the environment. System Python is never used.

## Consequences

- Python objects appear in the language only as the explicit dynamic type `py`; every `py` expression is fallible and Python exceptions convert to mongoose error values at the border.
- Value semantics stop at the bridge; `py` values are references. Documented, visible in the type.
- The GIL constrains future concurrency; that work lands inside the one bridge module.
- Projects that never `import py` never provision or start CPython.

## Amendment 2026-07-10

In-process CPython bounds the no-crash guarantee: a C extension that
itself segfaults or aborts takes the process down, and no exception
translation can catch a SIGSEGV. The claim is scoped accordingly
everywhere it is made (spec section 1 and 12, README, the book): no
crash originates in nevla, Python exceptions arrive as values, and
native code below the bridge is the one documented boundary.
Out-of-process isolation would close it at a cost this trade-off
already rejected.
