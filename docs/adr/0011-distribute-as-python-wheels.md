# 11. Distribute as python wheels

Status: accepted 2026-07-09

## Context

rikki embeds CPython, so a distributed binary must find a compatible
libpython and stdlib on the user's machine; a plain prebuilt binary would
need per-python-version builds and a discovery story anyway. The target
audience (ML users) already has uv, and `cargo install` from source demands
a Rust toolchain they do not have.

## Decision

Ship `rikki` and `tk` inside python wheels built by maturin
(`bindings = "bin"`; the wheel build links python-build-standalone CPython
statically). Wheels are tagged per interpreter and platform, so the install
channel itself keys the python match: `uv tool install rikki-lang` is the
blessed path (bare `rikki` was taken on PyPI; the binaries stay `rikki`
and `tk`). At startup the bridge resolves `PYTHONHOME` by probing
`pythonX.Y`, `python3`, then `uv python find X.Y`, because the build
python's baked prefix does not exist on user machines. `cargo install
--path .` remains the development path. The wheel version is read from
Cargo.toml (`dynamic = ["version"]`); Cargo.toml is the single version
source.

## Consequences

Releases are a CI matrix (platform x python minor) publishing to PyPI on
tag. Runtime needs a matching CPython stdlib on the machine; uv provides
one on demand. An explicit `PYTHONHOME` always wins over probing. If the
probe finds nothing, startup falls back to libpython's own search, which
is correct for dev builds linked against a system python.
