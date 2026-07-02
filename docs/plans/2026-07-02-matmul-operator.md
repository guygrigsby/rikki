# `@` Matmul Operator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the `@` binary operator, py-only, dispatching to Python matmul through the bridge, per `docs/specs/2026-07-02-matmul-operator-design.md`.

**Architecture:** One new token and `BinOp` variant flow through the existing operator pipeline: lexer → parser (mul precedence) → checker (py-only rule) → eval's existing py binop dispatch → `bridge::binop` (pyo3 `matmul`). Exhaustive matches force every layer at compile time.

**Tech Stack:** Rust, pyo3 0.29 (`PyAnyMethods::matmul` exists at `types/any.rs:383`).

## Global Constraints

- Full gate green before the commit: `RIKKI_TEST_PY=1 cargo test`, clippy clean.
- Spec diff lands in the SAME commit as the semantics (CLAUDE.md rule).
- User programs must never panic the process.
- Only `src/bridge.rs` may name pyo3.
- Commit style: terse, verb-first, area prefix; no attribution trailers.

---

### Task 1: Failing tests for lex/parse, check, and bridge

**Files:**
- Modify: `src/parser.rs` (inline `mod tests`, near the existing `precedence` test at ~line 904)
- Create: `tests/golden/check/matmul-native.rk`, `tests/golden/check/matmul-native.err`
- Modify: `src/bridge.rs` (inline `mod tests` at file end)

**Interfaces:**
- Produces: test names `parser::tests::matmul_precedence`, `bridge::tests::matmul_dispatches_to_python`; golden `check/matmul-native`.

- [ ] **Step 1: Parser precedence test** — in `src/parser.rs` `mod tests`, alongside the existing `precedence` test:

```rust
#[test]
fn matmul_precedence() {
    // w @ x + b parses as (w @ x) + b; a @ b @ c is left associative
    let e = expr("w @ x + b");
    let ExprKind::Binary { op: BinOp::Add, lhs, .. } = e.kind else {
        panic!("expected +, got {:?}", e.kind)
    };
    assert!(matches!(
        lhs.kind,
        ExprKind::Binary { op: BinOp::MatMul, .. }
    ));
    let e = expr("a @ b @ c");
    let ExprKind::Binary { op: BinOp::MatMul, lhs, .. } = e.kind else {
        panic!("expected @, got {:?}", e.kind)
    };
    assert!(matches!(
        lhs.kind,
        ExprKind::Binary { op: BinOp::MatMul, .. }
    ));
}
```

- [ ] **Step 2: Check golden** — `tests/golden/check/matmul-native.rk`:

```
fn main() {
    a := [[1.0, 2.0], [3.0, 4.0]]
    b := [[1.0, 0.0], [0.0, 1.0]]
    c := a @ b
    print(c)
}
```

`tests/golden/check/matmul-native.err` (one substring per line):

```
@ needs py operands
```

- [ ] **Step 3: Bridge unit test** — in `src/bridge.rs` `mod tests` (the test venv has no numpy/torch, so define `__matmul__` in embedded Python):

```rust
#[test]
fn matmul_dispatches_to_python() {
    use crate::ast::BinOp;
    use crate::value::Value;
    super::init(None);
    let h = Python::attach(|py| {
        let ns = pyo3::types::PyDict::new(py);
        py.run(
            std::ffi::CString::new(
                "class M:\n    def __matmul__(self, other):\n        return 42",
            )
            .unwrap()
            .as_c_str(),
            None,
            Some(&ns),
        )
        .unwrap();
        let m = ns.get_item("M").unwrap().unwrap().call0().unwrap();
        PyHandle::new(m.unbind())
    });
    let out = super::binop(BinOp::MatMul, &Value::Py(h.clone()), &Value::Py(h)).unwrap();
    let Value::Py(r) = out else { panic!("{out:?}") };
    assert_eq!(super::display(&r), "42");
}
```

(`unwrap` in `#[cfg(test)]` is the assertion mechanism, allowed.)

- [ ] **Step 4: Run all three, verify RED for the right reasons**

Run: `cargo test matmul 2>&1 | tail; cargo test --test golden 2>&1 | grep -A3 matmul`
Expected: compile errors mentioning `BinOp::MatMul` does not exist (parser and bridge tests), and once code exists the golden currently fails with a parse diagnostic (`unexpected character`) — record what you see; the parser test cannot run before Task 2 because the variant does not compile. That IS the red state for a compile-enforced language change.

### Task 2: Implement across the pipeline

**Files:**
- Modify: `src/token.rs` (Token enum), `src/lexer.rs:~199`, `src/ast.rs` (BinOp), `src/parser.rs:~530`, `src/typecheck/expr.rs` (`binary`, after the Add..Rem arm), `src/bridge.rs` (`binop` match)

**Interfaces:**
- Produces: `Token::At`, `ast::BinOp::MatMul`, checker diagnostic `@ needs py operands; there is no native matrix type`, `bridge::binop(BinOp::MatMul, ..)`.

- [ ] **Step 1: Token and lexer** — `src/token.rs` after `Percent,`: add `At,`. `src/lexer.rs` next to `'%' => Token::Percent,`: add `'@' => Token::At,`.

- [ ] **Step 2: AST** — `src/ast.rs` `BinOp`, after `Rem,`: add `MatMul,`.

- [ ] **Step 3: Parser** — `src/parser.rs` `binary()`, with the precedence-6 arms:

```rust
Some(Token::Star) => (BinOp::Mul, 6),
Some(Token::Slash) => (BinOp::Div, 6),
Some(Token::Percent) => (BinOp::Rem, 6),
Some(Token::At) => (BinOp::MatMul, 6),
```

- [ ] **Step 4: Checker** — `src/typecheck/expr.rs` `binary()`. The py pre-check at the top already returns `Type::Py` when either side is py (chain absorption included), so only the native case needs an arm. Add after the `Add | Sub | Mul | Div | Rem` arm:

```rust
BinOp::MatMul => {
    if unknown {
        return Type::Unknown;
    }
    self.diag(span, "@ needs py operands; there is no native matrix type");
    Type::Unknown
}
```

- [ ] **Step 5: Bridge** — `src/bridge.rs` `binop()` match, after `BinOp::Rem => a.rem(b),`:

```rust
BinOp::MatMul => a.matmul(b),
```

The interpreter needs no change: eval's Binary arm already routes either-side-py to `bridge::binop`, and native `@` cannot reach `Interp::binop`'s catch-all (`"bad operands"`) through checked code; the repl's unchecked path faults there, which is correct.

- [ ] **Step 6: Verify GREEN**

Run: `RIKKI_TEST_PY=1 cargo test 2>&1 | grep -E 'test result|FAILED'` and `cargo clippy --all-targets 2>&1 | grep -c warning`
Expected: all suites ok, 0 warnings. If the checker's `binary()` had a wildcard the new arm may be unreachable — it does not; the op match is exhaustive.

### Task 3: Spec, py golden, commit

**Files:**
- Modify: `language-spec.md:~894` (precedence table), `language-spec.md:~1480` (13.2 py operators), `language-spec.md` section 7.9 prose
- Create (conditional): `tests/golden/py/matmul.rk` + `.out` only if a pure-stdlib vehicle exists; otherwise skip — the bridge test carries runtime coverage (spec'd decision).

- [ ] **Step 1: Precedence table row** — `language-spec.md` 7.9 table, row 6 becomes:

```
| 6 | `*` `/` `%` `@` |
```

Add after the associativity sentence: `@` is matrix multiplication; it is defined only when at least one operand is `py` (section 13.2) and is a compile-time error otherwise (`@ needs py operands`). There is no native matrix type.

- [ ] **Step 2: 13.2 operator list** — extend the py binary-operator bullet:

```
- the binary operators `+ - * / % @ == != < <= > >=` when either operand is
```

with a following sentence: `@` dispatches to Python matrix multiplication (`__matmul__`/`__rmatmul__`); unlike the arithmetic operators it has no meaning on native operands.

- [ ] **Step 3: Full gate, then one commit** (semantics + spec + tests together):

```bash
RIKKI_TEST_PY=1 cargo test && cargo clippy --all-targets
git add -A && git commit -m 'lang: @ matmul operator, py-only

sugar over the bridge binop path at mul precedence; native operands are
a compile error, there is no native matrix type. spec 7.9 and 13.2.'
```

### Task 4: Real-path verification in lmtk

**Files:**
- Read/Modify: `../lmtk/src/train.rk`, `../lmtk/src/chinchilla.rk` (wherever matmul is spelled explicitly; if lmtk never calls matmul directly, verify with the snippet only and say so)
- Scratch: matmul snippet run against lmtk's venv torch

**Interfaces:**
- Consumes: the `tk` binary built from this repo (`cargo build`, `target/debug/tk`), lmtk's `.rikki/venv` torch.

- [ ] **Step 1: Snippet through real torch** — write `/tmp`-scratch `mm.rk` in the lmtk project dir (so the venv resolves):

```
import py "torch"

fn main() (error?) {
    w := check torch.randn(3, 4)
    x := check torch.randn(4, 2)
    a := check str((w @ x).shape)
    b := check str(torch.matmul(w, x).shape)
    print(a)
    print(b)
    eq := check str(torch.equal(check (w @ x), check torch.matmul(w, x)))
    print(eq)
    return none
}
```

Run: `cd ../lmtk && /Users/guygrigsby/projects/rikki/target/debug/tk <snippet path>` — expected: two identical shapes and `True`. Adjust check placement to whatever the checker demands; the assertion is `@` ≡ `torch.matmul`.

- [ ] **Step 2: Adopt in lmtk** — replace explicit matmul spellings with `@` where they exist; run `make quickstart` in lmtk before and after, byte-identical output (it is seeded). Commit in lmtk: `use @ matmul, rikki grew the operator`.

- [ ] **Step 3: Report** — state what was verified, including quickstart parity.
