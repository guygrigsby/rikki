# mongoose v1 interpreter implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Working `mongoose` binary that typechecks and runs `.mg` programs per `docs/specs/2026-07-01-mongoose-v1-design.md`, including the CPython bridge and uv-driven toolchain.

**Architecture:** Single Rust binary crate. Pipeline: lexer → parser → typechecker (flow narrowing) → tree-walking evaluator. CPython isolated in one bridge module (PyO3). Toolchain shells to `uv`. Golden `.mg` files under `tests/golden/` are the language spec in executable form.

**Tech Stack:** Rust 2021, `pyo3` (bridge), `clap` (CLI), `ureq` (http), `toml` (manifest), `indexmap` (insertion-order maps), `rustyline` (repl), `ctrlc` (SIGINT for ctx).

## Global Constraints

- The spec file `docs/specs/2026-07-01-mongoose-v1-design.md` is authoritative. On any conflict between this plan and the spec, the spec wins; flag the conflict in the task's commit message.
- Interpreter code never panics on user input. Every `unwrap`/`expect` reachable from user source is a bug. Runtime faults exit via a reported error, not a Rust panic.
- No implicit conversions anywhere. No truthiness. `int` and `float` never mix.
- Value semantics: `Value` derives `Clone` and clone is deep (Vec/IndexMap clone). `py` values are the documented exception (reference clone).
- Golden tests first: every language-visible behavior lands as `tests/golden/**` before its implementation.
- Commit style: terse, verb-first, no em/en dashes, no AI attribution, prefix by area (`lexer:`, `parser:`, `check:`, `eval:`, `stdlib:`, `bridge:`, `cli:`).
- Commit after every green test cycle. Never commit red.

## File structure

```
Cargo.toml
src/main.rs        CLI entry (clap): run, check, repl, new, py
src/lib.rs         pub mods, run_source() entry used by golden harness
src/token.rs       Token enum + spans
src/lexer.rs       source → Vec<Token>
src/ast.rs         Expr, Stmt, Decl, TypeExpr, Program (all with spans)
src/parser.rs      tokens → Program (Pratt expressions)
src/types.rs       Type enum, type display
src/typecheck.rs   checker + flow narrowing
src/value.rs       runtime Value enum
src/env.rs         lexical scopes
src/interp.rs      tree-walking evaluator, Flow control enum, faults
src/builtins.rs    print, printf, sprintf, len, range, conversions
src/methods.rs     str/list/map built-in methods
src/stdlib/mod.rs  module registry
src/stdlib/math.rs, error.rs, file.rs, ctx.rs, http.rs
src/bridge.rs      ALL PyO3 code lives here and nowhere else
src/project.rs     mongoose.toml + lock parsing, uv driving
src/diag.rs        error reporting with file:line:col rendering
tests/golden.rs    harness
tests/golden/**/*.mg + *.out / *.err
docs/adr/          decision records
```

Dependency order: tasks 1→8 are strictly sequential. After task 8: tasks 9, 10, 11 can run in parallel; 12–15 (stdlib) parallel after 9; 16, 17, 18 after 9; 19 last.

---

### Task 1: Crate scaffold, golden harness, ADRs

**Files:**
- Create: `Cargo.toml`, `src/main.rs`, `src/lib.rs`, `tests/golden.rs`, `tests/golden/smoke/print.mg`, `tests/golden/smoke/print.out`
- Create: `docs/adr/0001-rust-tree-walking-interpreter.md`, `docs/adr/0002-embedded-cpython-pyo3-uv.md`, `docs/adr/0003-errors-as-values-options-no-nil.md`, `docs/adr/0004-value-semantics-py-reference-exception.md`

**Interfaces:**
- Produces: `mongoose::run_source(path: &Path) -> RunResult` where `RunResult { stdout: String, exit: ExitKind }`, `enum ExitKind { Ok, CompileError(String), RuntimeError(String) }`. The harness and CLI both consume this; every later task keeps it compiling.

- [ ] **Step 1: Write the harness and one smoke golden test**

`tests/golden/smoke/print.mg`:
```
fn main() {
    print("hello")
}
```
`tests/golden/smoke/print.out`:
```
hello
```

`tests/golden.rs`:
```rust
use std::{fs, path::PathBuf};

fn collect(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() { collect(&p, out); }
        else if p.extension().is_some_and(|x| x == "mg") { out.push(p); }
    }
}

#[test]
fn golden() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let mut cases = vec![]; collect(&root, &mut cases); cases.sort();
    let mut failures = vec![];
    for mg in &cases {
        let res = mongoose::run_source(mg);
        let out_f = mg.with_extension("out");
        let err_f = mg.with_extension("err");
        let ok = if out_f.exists() {
            let want = fs::read_to_string(&out_f).unwrap();
            matches!(res.exit, mongoose::ExitKind::Ok) && res.stdout == want
        } else if err_f.exists() {
            let want = fs::read_to_string(&err_f).unwrap();
            let got = match &res.exit {
                mongoose::ExitKind::CompileError(m)
                | mongoose::ExitKind::RuntimeError(m) => m.clone(),
                _ => String::new(),
            };
            want.lines().all(|l| got.contains(l.trim()))
        } else { false };
        if !ok { failures.push(format!("{}\n  got exit={:?} stdout={:?}", mg.display(), res.exit, res.stdout)); }
    }
    assert!(failures.is_empty(), "golden failures:\n{}", failures.join("\n"));
}
```

`.err` files match by substring per line, so error goldens pin the message content without freezing exact formatting.

- [ ] **Step 2: Run `cargo test`, verify it fails** (no `run_source` yet). Expected: compile error, `run_source` not found.
- [ ] **Step 3: Stub `src/lib.rs`** with `run_source` returning `ExitKind::CompileError("unimplemented".into())` and empty module decls; `src/main.rs` with clap skeleton (`run <file>`, `check <file>` subcommands calling `run_source`). Harness now runs and the smoke test fails on output mismatch, proving the harness detects failure.
- [ ] **Step 4: Write the four ADRs.** Each `# N. Title`, `Status: accepted`, Context/Decision/Consequences, one screen each, content lifted from the spec sections (interpreter, bridge, errors, value semantics).
- [ ] **Step 5: Commit** `scaffold: crate, golden harness, adrs`. The red smoke test is acceptable at this single commit; task 8 turns it green. Alternatively gate it: have the harness skip files listed in `tests/golden/PENDING` and list `smoke/print.mg` there; remove entries as features land. Use the PENDING mechanism; it keeps every commit green.

### Task 2: Lexer

**Files:**
- Create: `src/token.rs`, `src/lexer.rs`
- Test: unit tests in `src/lexer.rs` `#[cfg(test)]`

**Interfaces:**
- Produces: `lex(src: &str) -> Result<Vec<Spanned<Token>>, Diag>`; `Spanned<T> { node: T, line: u32, col: u32 }`; `Diag { msg: String, line: u32, col: u32 }` in `src/diag.rs`.

Token set:
```rust
pub enum Token {
    Ident(String), Int(i64), Float(f64), Str(String),
    Fn, Struct, Import, Py, Return, If, Else, For, In, Break, Continue, Check, None_, True, False,
    LParen, RParen, LBrace, RBrace, LBracket, RBracket,
    Comma, Dot, Colon, ColonEq, Eq, EqEq, NotEq, Lt, LtEq, Gt, GtEq,
    Plus, Minus, Star, Slash, Percent, AndAnd, OrOr, Bang, Question, Semi, Newline,
}
```

- [ ] **Step 1: Write failing unit tests**: keywords vs idents, `:=` vs `:` `=`, string escapes (`\n \t \" \\`), float vs int vs `xs[a:b]` colon, `//` comments to end of line, newline tokens (statement terminators), unterminated string produces `Diag` with line/col.
- [ ] **Step 2: Run, verify fail.** `cargo test lexer` fails to compile.
- [ ] **Step 3: Implement** hand-rolled scanner. Newlines are significant (statement separators like Go); emit `Token::Newline`, collapse runs.
- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Commit** `lexer: tokens, spans, comments, escapes`.

### Task 3: AST and expression parser

**Files:**
- Create: `src/ast.rs`, `src/parser.rs`
- Test: unit tests in `src/parser.rs`

**Interfaces:**
- Produces:
```rust
pub enum Expr {
    Int(i64), Float(f64), Str(String), Bool(bool), NoneLit,
    Ident(String),
    List(Vec<Expr>), MapLit(Vec<(Expr, Expr)>),
    StructLit { name: String, fields: Vec<(String, Expr)> },
    Unary { op: UnOp, rhs: Box<Expr> },
    Binary { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
    Call { callee: Box<Expr>, args: Vec<Expr> },
    Method { recv: Box<Expr>, name: String, args: Vec<Expr> },
    Field { recv: Box<Expr>, name: String },
    Index { recv: Box<Expr>, idx: Box<Expr> },
    Slice { recv: Box<Expr>, lo: Box<Expr>, hi: Box<Expr> },
    Lambda { params: Vec<Param>, ret: Option<TypeExpr>, body: LambdaBody },
    Check(Box<Expr>),
    Conv { target: TypeExpr, arg: Box<Expr> },   // int(x), list[int](x)
}
pub enum TypeExpr { Named(String), List(Box<TypeExpr>), Map(Box<TypeExpr>, Box<TypeExpr>),
    Opt(Box<TypeExpr>), Fn(Vec<TypeExpr>, Vec<TypeExpr>), Py }
```
All spanned (wrap in `Spanned<Expr>`; elided here for brevity but required).

- [ ] **Step 1: Failing tests**: precedence (`1 + 2 * 3`, `!a && b || c`, comparison non-associative like Go is fine to allow left-assoc), method chains `xs.map(f).filter(g)`, `check torch.randn([2, 3])` parses as `Check(Method(...))` binding the full postfix chain, lambda `fn(x) { x > 2 }` (expression body) and `fn(x: int) int { return x * 2 }` (block body), `list[int](t.shape)` parses as `Conv`.
- [ ] **Step 2: Run, verify fail.**
- [ ] **Step 3: Implement Pratt parser.** `check` is lowest-precedence prefix over a unary-postfix chain. `int(...)`/`str(...)`/`float(...)`/`list[T](...)` become `Conv` when callee is a type name.
- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Commit** `parser: expressions, pratt, check, lambdas, conversions`.

### Task 4: Statement and declaration parser

**Files:**
- Modify: `src/ast.rs`, `src/parser.rs`

**Interfaces:**
- Produces:
```rust
pub enum Stmt {
    Let { names: Vec<String>, expr: Expr },              // a, b := e
    Assign { target: Expr, expr: Expr },                  // x = e, xs[i] = e, m[k] = e, u.f = e
    Expr(Expr),
    Return(Vec<Expr>),
    If { cond: Expr, then: Block, elifs: Vec<(Expr, Block)>, els: Option<Block> },
    ForIn { names: Vec<String>, iter: Expr, body: Block },
    ForCond { cond: Option<Expr>, body: Block },          // for cond {} / for {}
    Break, Continue,
}
pub enum Decl {
    Fn(FnDecl),                                            // name, params, ret: Vec<TypeExpr>, body
    Struct { name: String, fields: Vec<(String, TypeExpr)> },
    Import { path: String, py: bool },                     // import "http" / import py "torch"
}
pub struct Program { pub decls: Vec<Decl> }
```

- [ ] **Step 1: Failing tests**: full `fn` with multi-return `(str, error?)`, `a, b := f()`, `if/else if/else`, all four `for` forms, `import "http"`, `import py "torch"`, struct decl and literal, newline-terminated statements.
- [ ] **Step 2: Run, verify fail.**
- [ ] **Step 3: Implement.** Return type list: bare single type or parenthesized list.
- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Commit** `parser: statements, decls, imports`.

### Task 5: Type representation and checker core

**Files:**
- Create: `src/types.rs`, `src/typecheck.rs`
- Create: `tests/golden/check/` goldens (`.err` files)

**Interfaces:**
- Produces:
```rust
pub enum Type { Int, Float, Bool, Str, List(Box<Type>), Map(Box<Type>, Box<Type>),
    Opt(Box<Type>), Fn(Vec<Type>, Vec<Type>), Struct(String), Error, Py, Module(String), Unit }
pub fn check(prog: &Program) -> Result<TypedInfo, Vec<Diag>>
```
`TypedInfo` records per-lambda inferred param types and per-`check` the enclosing return signature (evaluator consumes it).
- Consumes: `Program` from task 4.

- [ ] **Step 1: Golden `.err` tests** (each one `.mg` + `.err`):
  - `check/truthiness.mg`: `if 1 { }` → err contains `condition must be bool`
  - `check/mixed-arith.mg`: `1 + 2.0` → `int and float do not mix`
  - `check/undefined.mg`: use of unknown ident → `undefined: x`
  - `check/wrong-arg.mg`: call with wrong arg type → `expected str, got int`
  - `check/arity.mg`: `a, b := f()` where f returns one value → `expected 2 values, got 1`
- [ ] **Step 2: Run, verify these fail** (checker doesn't exist; harness reports no CompileError).
- [ ] **Step 3: Implement**: two passes (collect fn/struct/import signatures, then check bodies). Scoped symbol table. `:=` infers, `=` requires declared and same type. Function calls check arity and arg types. `print`/`len`/`range` typed as builtins here.
- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Commit** `check: core checker, scopes, calls`.

### Task 6: Options, flow narrowing, error rules, check

**Files:**
- Modify: `src/typecheck.rs`
- Create: goldens under `tests/golden/check/`

- [ ] **Step 1: Golden tests**:
  - `opt-unchecked.mg`: field access on `User?` → `.err`: `might be none`
  - `opt-narrow.mg`: access inside `if u != none { print(u.name) }` → `.out` (runs once eval lands; add to PENDING until task 8)
  - `opt-narrow-terminal.mg`: `if u == none { return }` then access after the if → compiles
  - `drop-error.mg`: calling `(T, error?)` fn as a single-value expression statement or `x := f()` without second binding → `.err`: `error result must be handled`
  - `check-outside.mg`: `check` in a fn whose last return component is not `error?` → `.err`: `check requires enclosing function to return error?`
  - `none-compare.mg`: `1 == none` → `.err`: `none only compares to option types`
- [ ] **Step 2: Run, verify fail.**
- [ ] **Step 3: Implement.** Narrowing environment: a stack of `HashMap<String, Type>` refinements. Conditions of shape `x != none` / `x == none` (x a local) narrow: `!=` narrows then-branch to inner type, `==` narrows else-branch; if a branch always diverges (all paths return/break/continue), apply the inverse refinement after the `if`. Assignment to a narrowed variable clears its refinement. `check e`: `e` must be `(T..., error?)`; enclosing fn last return must be `error?`; type of the expression is `T...` minus the error slot. Multi-value returns only exist at call/return boundaries (like Go): a `(T, error?)` value must be immediately destructured, `check`ed, or returned.
- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Commit** `check: options, narrowing, mandatory error handling, check keyword`.

### Task 7: Evaluator core

**Files:**
- Create: `src/value.rs`, `src/env.rs`, `src/interp.rs`, `src/builtins.rs` (print/len only for now)
- Create: goldens `tests/golden/eval/`

**Interfaces:**
- Produces:
```rust
#[derive(Clone)]
pub enum Value {
    Int(i64), Float(f64), Bool(bool), Str(String),
    List(Vec<Value>), Map(indexmap::IndexMap<MapKey, Value>),
    Struct { name: String, fields: indexmap::IndexMap<String, Value> },
    Opt(Option<Box<Value>>),
    Err(ErrVal),                       // .msg, .cause
    Fn(FnRef), Py(bridge::PyHandle), Tuple(Vec<Value>), Unit,
}
pub enum Flow { Normal(Value), Return(Value), Break, Continue }
pub enum Fault { Msg(String) }          // div by zero, index oob; carries mongoose stack
fn eval_expr(&mut self, e: &Expr) -> Result<Value, Fault>
fn exec_block(&mut self, b: &Block) -> Result<Flow, Fault>
```
- Consumes: `TypedInfo` from task 5/6.

- [ ] **Step 1: Goldens** (`.out` unless noted): arithmetic and precedence, string concat, bool ops with short-circuit, if/else chains, all four for-forms with break/continue, `range(3)` → iterating prints `0 1 2`, value-semantics proof:
  `eval/value-semantics.mg`:
  ```
  fn main() {
      a := [1, 2, 3]
      b := a
      b[0] = 99
      print(a[0])   // 1
      print(b[0])   // 99
  }
  ```
  and function-arg copy equivalent. Fault goldens (`.err`): `1 / 0` → `division by zero`, `xs[9]` → `index out of bounds` with `main` in the reported stack.
- [ ] **Step 2: Run, verify fail** (remove entries from PENDING as they land).
- [ ] **Step 3: Implement** environment as scope stack, functions as global decls callable by name, deep-copy on `:=`, `=`, arg binding and return (that's just `Value::clone`). Faults unwind through `Result::Err`, rendered by `run_source` with the interpreter call stack.
- [ ] **Step 4: Run, verify pass** including task 1's smoke test; delete PENDING entries.
- [ ] **Step 5: Commit** `eval: core evaluator, value semantics, faults`.

### Task 8: Multi-return, error values, check propagation

**Files:**
- Modify: `src/interp.rs`, `src/value.rs`
- Create: goldens `tests/golden/errors/`

- [ ] **Step 1: Goldens**:
  ```
  fn boom() (int, error?) {
      return 0, error.new("kaboom")
  }
  fn hello() (int, error?) {
      return 7, none
  }
  fn run() (int, error?) {
      n := check hello()
      m := check boom()
      return n + m, none
  }
  fn main() {
      v, err := run()
      if err != none {
          print("got: " + err.msg)
          return
      }
      print(v)
  }
  ```
  Expected `.out`: `got: kaboom`. Also: `main` returning `(error?)` with non-none error → program exits nonzero printing `error: kaboom` (assert via `.err` golden); two-value destructure `v, err :=`; returning tuples.
- [ ] **Step 2: Run, verify fail.**
- [ ] **Step 3: Implement**: `Return` carries `Tuple`; `Let` with N names destructures; `Check(e)`: eval `e` → tuple, if last slot is `Opt(Some(err))`, produce `Flow::Return` of zero-values-plus-err per enclosing signature (from `TypedInfo`), else yield tuple minus error slot. `error.new` lives in the `error` stdlib module but implement the `ErrVal` type and `.msg`/`.cause` field access now with a temporary builtin `error.new`; the module registry in task 12 takes it over.
- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Commit** `eval: multi-return, error values, check propagation`.

### Task 9: Builtins: printf, sprintf, conversions

**Files:**
- Modify: `src/builtins.rs`
- Create: goldens `tests/golden/builtins/`

**Interfaces:**
- Produces: `printf(format, ...)`, `sprintf(format, ...) str`, fallible `int(x) (int, error?)`, `float(x)`, `str(x)` (infallible on native values, `(str, error?)` uniformly anyway per spec), `list[T](x) (list[T], error?)`.

- [ ] **Step 1: Goldens**: `%v` on every value kind (canonical forms: lists `[1, 2, 3]`, maps `{a: 1}`, options `none` / the inner value, structs `User{name: guy}`), `%d %s %t %q %%`, `%.2f`, verb/arg count mismatch → fault `printf: wrong argument count`, `int("42")` ok, `int("x")` err with narrowing recovery, `int(3.9)` → `3`.
- [ ] **Step 2: Run, verify fail.**
- [ ] **Step 3: Implement** small verb scanner (no regex). One shared `render(v: &Value, verb)` used by print, %v and sprintf.
- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Commit** `builtins: printf family, fallible conversions`.

### Task 10: First-class functions, closures, container methods

**Files:**
- Modify: `src/interp.rs`, `src/typecheck.rs`
- Create: `src/methods.rs`, goldens `tests/golden/fns/`

**Interfaces:**
- Produces: `Value::Fn(FnRef)` closures capturing by value; methods per spec Surface details: str `split trim contains replace upper lower starts_with ends_with`, list `map filter each sum sorted sorted_by append contains join`, map `keys values has delete`. All pure: `append`/`delete`/`sorted` return new values.

- [ ] **Step 1: Goldens**: the spec's chain `nums.map(double).filter(fn(x) { x > 2 }).sum()` → `14`; closure capture-by-value proof:
  ```
  fn main() {
      n := 1
      f := fn() int { return n }
      n = 2
      print(f())   // 1
  }
  ```
  lambda param inference from `map` on `list[int]`; `sorted_by(fn(a, b) { a < b })`; `m.keys()` insertion order; `xs.append(4)` leaves `xs` unchanged unless reassigned.
- [ ] **Step 2: Run, verify fail.**
- [ ] **Step 3: Implement.** Checker: contextual typing for lambda params (expected `Fn` type from method/callee signature). Eval: closure = params + body + captured env snapshot (deep copy at creation).
- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Commit** `eval: closures, container methods, chaining`.

### Task 11: Structs end to end

**Files:**
- Modify: `src/typecheck.rs`, `src/interp.rs`
- Create: goldens `tests/golden/structs/`

- [ ] **Step 1: Goldens**: declare/instantiate/access, missing field in literal → `.err` `missing field: age`, unknown field access → `.err`, field assignment `u.name = "x"` mutates local copy only (value-semantics proof through a helper fn), struct inside list, `%v` rendering.
- [ ] **Step 2: Run, verify fail.**  
- [ ] **Step 3: Implement** (checker: nominal types, literal completeness; eval: `IndexMap` fields).
- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Commit** `lang: structs`.

### Task 12: stdlib registry, math, error modules

**Files:**
- Create: `src/stdlib/mod.rs`, `src/stdlib/math.rs`, `src/stdlib/error.rs`
- Create: goldens `tests/golden/stdlib/`

**Interfaces:**
- Produces: `stdlib::lookup(name: &str) -> Option<Module>` where `Module` carries typed signatures (for the checker) and native fns (for the evaluator). `import "math"` binds `Type::Module("math")`; `math.sqrt(2.0)` dispatches through it. `error.new(msg) error`, `error.wrap(err, msg) error`.

- [ ] **Step 1: Goldens**: `math.abs(-3)`, `min/max` (int and float overloads: separate names not needed, type-directed), `sqrt pow floor ceil round`, `math.pi`, `import "nope"` → `.err` `unknown module`, `error.wrap` chain exposing `.cause.msg` under narrowing.
- [ ] **Step 2: Run, verify fail.**
- [ ] **Step 3: Implement**; move task 8's temporary `error.new` here.
- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Commit** `stdlib: registry, math, error`.

### Task 13: stdlib file

**Files:**
- Create: `src/stdlib/file.rs`, goldens `tests/golden/stdlib/file.mg` (+ tempdir-driven unit tests in the module for remove/mkdir)

- [ ] **Step 1: Tests**: golden writes to a path under `std::env::temp_dir()` communicated via... goldens can't parameterize paths, so file module behavior tests live as Rust unit tests in `src/stdlib/file.rs` using `tempfile`-free `std::env::temp_dir()` + unique suffix from the test name (no `Date::now` analog needed; use process id). One golden covers the error path only: `file.read("/nonexistent/xyz")` second value is an error whose `.msg` contains `nonexistent`.
- [ ] **Step 2: Run, verify fail.**
- [ ] **Step 3: Implement** `read write append exists list remove mkdir` per spec, all `(T, error?)` or `error?` shaped.
- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Commit** `stdlib: file`.

### Task 14: stdlib ctx

**Files:**
- Create: `src/stdlib/ctx.rs`, unit tests inline; golden `tests/golden/stdlib/ctx.mg`

**Interfaces:**
- Produces: ctx values (`Value::Struct`-backed opaque type `Ctx`): `ctx.background()`, `ctx.timeout(parent, secs: float)`, `ctx.interrupt(parent)`, methods `done() bool`, `err() error?`. Internally `Arc<CtxInner { deadline: Option<Instant>, cancelled: Arc<AtomicBool> }>`. `ctx.interrupt` installs a `ctrlc` handler (once, process-global) setting the flag.

- [ ] **Step 1: Tests**: unit tests for deadline expiry (`timeout(bg, 0.0)` is immediately done, `err().msg` contains `deadline`), golden: `c := ctx.background()` then `print(c.done())` → `false`.
- [ ] **Step 2: Run, verify fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Commit** `stdlib: ctx, deadline and sigint`.

### Task 15: stdlib http

**Files:**
- Create: `src/stdlib/http.rs`, unit tests with a local `std::net::TcpListener` one-shot server thread (no external network in tests)

**Interfaces:**
- Produces: `http.get(c, url) (Response, error?)`, `http.post(c, url, body)`, `http.request(c, req)`; `Response` struct `{status int, body str, headers map[str, str]}`; ctx deadline → ureq timeout; cancelled ctx → immediate error.

- [ ] **Step 1: Tests**: unit test spins a listener returning a canned 200 with body `ok`, asserts status/body/headers; expired ctx returns error containing `deadline`; connection refused surfaces as error value, not fault.
- [ ] **Step 2: Run, verify fail.**
- [ ] **Step 3: Implement** with `ureq` (blocking; sequential v1).
- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Commit** `stdlib: http client`.

### Task 16: Project-file imports

**Files:**
- Modify: `src/typecheck.rs`, `src/interp.rs`, `src/lib.rs`
- Create: goldens `tests/golden/imports/` (multi-file: `main.mg` + `util.mg`)

- [ ] **Step 1: Goldens**: `import "util.mg"` (path relative to importing file) binds namespace `util` (file stem); `util.double(2)` works; import cycle → `.err` `import cycle`; missing file → `.err`. Harness change: a golden dir containing `main.mg` runs that file (supporting sibling files).
- [ ] **Step 2: Run, verify fail.**
- [ ] **Step 3: Implement**: parse/check imported files once (dedupe by canonical path), top-level fns and structs exposed under the stem namespace.
- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Commit** `lang: project file imports`.

### Task 17: CPython bridge

**Files:**
- Create: `src/bridge.rs`
- Create: goldens `tests/golden/py/` (guarded: harness skips `py/` when env `MONGOOSE_TEST_PY` unset; CI sets it)

**Interfaces:**
- Produces: `PyHandle` (cloneable reference), `bridge::import(name) -> Result<PyHandle, ErrVal>`, `bridge::getattr/call/index/binop`, `bridge::to_py(&Value) -> ...`, `bridge::extract(target: &Type, h: &PyHandle) -> Result<Value, ErrVal>`, `bridge::init(venv: Option<&Path>)`. Nothing outside this file names pyo3.
- Consumes: checker types `Type::Py`; evaluator routes all `py`-typed operations here.

- [ ] **Step 1: Goldens** (stdlib-python only, no torch in tests):
  ```
  import py "json"

  fn main() (error?) {
      obj := check json.loads("{\"a\": [1, 2, 3]}")
      xs := check list[int](obj["a"])
      print(xs.sum())          // 6
      bad := json.loads("{nope")
      _, err := bad
      if err != none {
          print(err.pytype)    // JSONDecodeError
      }
      return none
  }
  ```
  Plus: automatic inbound conversion (`json.dumps([1, 2, 3])` → `check str(...)` → `[1, 2, 3]`), Python exception carries `.msg`, `.pytype`, `.traceback`, extraction type mismatch (`int(json.loads("\"x\""))`) is an error not a fault.
- [ ] **Step 2: Run, verify fail** (`MONGOOSE_TEST_PY=1 cargo test`).
- [ ] **Step 3: Implement.** pyo3 `auto-initialize` off; `bridge::init` sets `PYTHONHOME`/`PYTHONPATH` from the resolved project env (task 18) before `Python::initialize`; unit tests fall back to system python via pyo3 auto-config. Every entry point wraps in `Python::attach`, converts `PyErr` → `ErrVal` with pytype/traceback. Checker rule: any expression whose head is `py`-typed infects the chain as `(py, error?)` consumed by `check` or destructure, matching spec.
- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Commit** `bridge: embedded cpython, py type, exception conversion`.

### Task 18: Toolchain: manifest, uv, py add

**Files:**
- Create: `src/project.rs`
- Modify: `src/main.rs`, `src/typecheck.rs` (manifest cross-check)
- Test: unit tests in `src/project.rs`; one golden `check/py-undeclared.mg` (`import py "torch"` with no manifest → `.err` contains `mongoose py add torch`)

**Interfaces:**
- Produces: `Project::load(dir) (Project, ...)` parsing `mongoose.toml` (`[project] name`, `python = "3.12"`, `[py-deps] torch = "..."`), `project.ensure_env()` running `uv python install`, `uv venv .mongoose/venv`, `uv pip sync` against `mongoose.lock` (generated by `uv pip compile` on `py add`); `mongoose py add <pkg>` edits toml + regenerates lock.

- [ ] **Step 1: Tests**: toml round-trip unit tests; `ensure_env` invoked with a fake `uv` on PATH (shell script recording args into a file) asserting the exact uv invocations; typecheck golden above.
- [ ] **Step 2: Run, verify fail.**
- [ ] **Step 3: Implement.** `mongoose run` resolves project by walking up to `mongoose.toml` (single-file mode when absent: no py imports allowed unless system python fallback flag `--system-py`). uv failures surface as CompileError-style diagnostics with uv's stderr included. Honor spec: no `import py` → never touch uv.
- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Commit** `cli: project manifest, uv env, py add`.

### Task 19: CLI polish: new, repl

**Files:**
- Modify: `src/main.rs`; create `src/repl.rs`
- Test: `tests/cli.rs` using `std::process::Command` on the built binary (`env!("CARGO_BIN_EXE_mongoose")`)

- [ ] **Step 1: Tests**: `mongoose new hello` creates `hello/mongoose.toml` + `hello/src/main.mg` that immediately passes `mongoose run` printing `hello, mongoose`; `mongoose check bad.mg` exits 1 with diagnostic on stderr; repl smoke: pipe `1 + 2\n` to `mongoose repl`, stdout contains `3`.
- [ ] **Step 2: Run, verify fail.**
- [ ] **Step 3: Implement.** Repl: rustyline loop, each line parsed as stmt, persistent env, prints non-Unit expression values with `%v` rendering, `check` disallowed at top level (message: `check needs a function`).
- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Commit** `cli: new, repl`.

---

## Self-review notes

- Spec coverage: every spec section maps to a task (surface → 2-11, stdlib → 12-15 + 9, bridge → 17, toolchain → 18, testing → 1, imports → 16, CLI → 19). ADRs → task 1.
- The `smoke/print.mg` red-window issue is closed by the PENDING mechanism in task 1 step 5.
- Type/name consistency: `run_source`/`ExitKind` (1) consumed by harness and CLI; `TypedInfo` produced in 5/6 consumed in 7/8/10; `ErrVal` produced in 8 consumed by 12/13/15/17; `PyHandle` only in 17; `Project` only in 18.
