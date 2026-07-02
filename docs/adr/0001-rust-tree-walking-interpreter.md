# 1. Rust tree-walking interpreter

Status: accepted

## Context

Mongoose v1 needs an interpreter fast enough for scripts whose heavy lifting happens inside Python libraries. Candidate implementations: bytecode VM, JIT, tree-walker. Candidate host languages narrowed to Rust and Go; embedding CPython from Go via cgo is miserable, PyO3 makes Rust the well-trodden path.

## Decision

Rust, single crate, four stages: lexer, parser, typechecker with flow narrowing, tree-walking evaluator over the AST. No bytecode, no JIT.

## Consequences

- Slowest interpreter architecture, acceptable because real workloads spend their time in PyTorch and I/O.
- Simplest to build and change while the language surface is still moving.
- Bytecode VM is the recorded v2 path if pure-mongoose loops ever hurt; the typechecker and AST survive that migration.
- Library-shaped crate (`pub mod lexer`, `parser`, `ast`, ...) with a thin CLI binary, so tooling can consume the language bits like Go's `go/parser`.
