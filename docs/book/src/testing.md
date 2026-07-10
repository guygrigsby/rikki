# Testing

> Status: design. This chapter is the design document for `rikki test`;
> the implementation follows it, not the other way around. Anything here
> that ships differently is a bug in one of the two.

A test is a fallible function. There is no test framework type, no
assertion keyword, and no new control flow: a test fails by returning an
error, which means everything you already know about errors — `check`,
`error.new`, wrapping — is the testing vocabulary too.

## Writing tests

Tests live in `*_test.rk` files beside the code they test, in functions
whose names start with `Test` and whose only result is `error?`:

```rikki
// util_test.rk
import "test"
import "util.rk"

fn TestDouble() (error?) {
    check test.eq(util.Double(21), 42)
    return none
}

fn TestParseRejectsGarbage() (error?) {
    _, err := util.Parse("nope")
    if err == none {
        return error.new("Parse accepted garbage")
    }
    return none
}
```

`check` is the assertion propagator: the first failing `test.eq` returns
its error, and that error is the failure report. Anything else in a
`_test.rk` file (helpers, lowercase functions, structs) is ordinary code.

Run them:

```sh
rikki test              # every *_test.rk under the project
rikki test src/util_test.rk
```

```text
ok   util_test.rk  TestDouble
ok   util_test.rk  TestParseRejectsGarbage
FAIL util_test.rk  TestHalf
     util_test.rk:14: expected 21, got 20
2 passed, 1 failed
```

The runner exits nonzero when anything fails.

## The test module

`import "test"` provides helpers that return `error?`, built to sit
behind `check`:

| Helper | Behavior |
|--------|----------|
| `test.eq(got, want)` | structural equality over any two values (the deep comparison `==` deliberately does not offer); error describes both sides |
| `test.neq(got, unwanted)` | structural inequality |
| `test.err(err)` | fails when `err` is `none`: asserts an error happened |
| `test.skip(reason)` | returns a sentinel the runner reports as skipped, not failed |

That is the whole v1 surface. `test.eq` covers most assertions because
its error message carries both values; when it doesn't fit, build the
error yourself — `return error.new(...)` is always available, and a
custom helper is just a function returning `error?`.

## Failure locations

An error value records its origin: the file and line where `error.new`
created it (or where a py exception crossed the bridge). The runner
prints the origin with the failure, so `test.eq`'s errors point at the
`check test.eq(...)` line that produced them. Origins ride the error
through `check` propagation and `error.wrap`, so a failure deep in a
helper still names the source line that started it. (Origins are a
property of all rikki errors, not a test feature; production error
reports carry them too.)

## Isolation and parallelism

Every test function runs in a fresh interpreter instance: globals,
imports, and module state are rebuilt per test, and the `test` module's
bookkeeping hangs off that instance — per-test identity is injected by
the runtime, not threaded through your code as a handle. Because tests
share nothing on the rikki side, the runner executes them in parallel by
default (`-j 1` to serialize).

Two things per-test isolation cannot isolate, both properties of the
world rather than the runner:

- **CPython is one interpreter per process.** Python module state
  (imports, caches, monkeypatching) is shared across tests, and the GIL
  serializes py-heavy tests. The same is true of pytest in one process.
- **The filesystem is shared.** Two parallel tests writing the same path
  race, exactly as in Go with `t.Parallel`. `test.tmpdir()` is the
  planned escape hatch; until then, derive per-test paths.

A fault in a test (integer overflow, index out of range) fails that test
with its rikki stack trace and the run continues; faults cannot cross
test boundaries because nothing else lives in that interpreter.

Output discipline: `print` output is captured per test and shown only
for failures. A passing test is silent.

## Deliberate omissions

- **Soft failures** (Go's `t.Error`, "record and continue"): v1 is
  fail-fast per test. The design composes — a recording `test.fail(msg)`
  can be added without changing any existing test — but it waits for the
  first table test that genuinely hurts.
- **Subtests** (`t.Run`): same posture. `test.run(name, fn () (error?))`
  fits the model when named table cases earn it.
- **In-test concurrency**: rikki has no concurrency yet. When it lands,
  spawned work inside a test will carry identity the way all rikki code
  carries cross-cutting context — through `ctx` — not through a
  test-only handle. This is a standing requirement on the concurrency
  design.
- **Cleanup**: rikki has no `defer`, so a test that creates external
  state and fails via `check` skips its own teardown. `with` covers py
  resources; native cleanup is an open language question that testing
  will keep pressure on.
- **Benchmarks and fuzzing**: out of scope for v1; the `Test` name
  prefix leaves room for siblings.

## White box, Go's way

`util_test.rk` sits inside `util.rk`'s trust boundary: the `_test` stem
pairing lets the test file touch `util`'s unexported names — functions,
struct fields, literals — through the ordinary qualified syntax
(`util.helper(...)` just compiles there). This is Go's same-package
testing translated to file modules: decomposed internals are unit-testable
without exporting them, while every other file still sees only the API.
The pairing follows the file name, so it holds anywhere a `_test.rk`
file appears, and nothing else about visibility changes.
