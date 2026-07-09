# py context managers: the with statement

Date: 2026-07-09. Status: approved. Source: training-project friction list
(torch.no_grad / autocast need set_grad_enabled or manual
`__enter__`/`__exit__` calls today). Queued in docs/backlog.md behind the
capture redesign; unblocked by ADR 0010.

## Syntax

```
WithStmt = "with" Expression Block .
```

`with` becomes a keyword (spec 4.4). The operand must be py: a py chain
(the statement absorbs it, like `for range`) or a py-typed value. Any other
operand type is a compile error ("with needs a py value"). Struct literals
are suppressed in the header like every other statement header (7.2.3).

No binding form in v1. `__enter__`'s return value is discarded; the torch
cases need none, and most py context managers expose their state on the
manager object, so bind the manager first when you need it:

```
import py "torch"

fn train_step() (error?) {
    with torch.no_grad() {
        ...
    }
    return none
}
```

Upgrade path if it ever hurts: `with x := expr { }` binding enter's result,
a parser-and-checker-only addition.

## Semantics

1. Evaluate the operand to a py handle. Call `mgr.__enter__()`. An
   exception in either faults (`py with: <exception>`): the statement has
   no error slot, same rule as py assignment and py range. Fallible
   acquisition is still expressible; do it before the statement
   (`f := check open(p)` then `with f { }`).
2. Run the body, a normal block scope.
3. `__exit__` runs on every rikki-level exit from the body:
   - Normal completion, `break`, `continue`, and a return whose final
     `error?` slot is `none` (or whose function has no error slot): the
     call is `__exit__(None, None, None)`, its return value ignored,
     exactly what Python passes when no exception is live. Control then
     continues where it was headed.
   - A return whose final `error?` slot holds an error, whether from
     `check` propagation or an explicit `return`: the bridge synthesizes
     an exception, an instance of a dedicated `Error` class (subclass of
     `Exception`, created once by the bridge), carrying the error's
     rendered text, and calls `__exit__(type, instance, None)`. This is
     what makes `with conn` on a sqlite connection roll back on the error
     path instead of silently committing; a None-always design would be
     the capture-snapshot bug again, silent wrongness at a transaction
     boundary. If `__exit__` returns truthy, Python contract says the
     exception is suppressed and control resumes after the block; rikki
     control flow cannot be resurrected by a py call, so a truthy return
     against a synthesized error faults
     (`py with: __exit__ cannot suppress a rikki error`). Loud beats
     silently continuing with zeroed results. Falsy: the return
     propagates as it was going to.
   - An exception raised by `__exit__` itself faults (`py with: ...`).
4. A fault in the body skips `__exit__` and terminates as usual. Faults
   are not catchable and no construct observes one (chapter 12); running
   user-visible Python during fault unwinding would be observing it. The
   interpreter's shutdown finalizers cover resource cleanup. Ceiling
   documented in the spec; revisit only with evidence.

Nesting is ordinary statement nesting; exits run innermost first because
each statement unwinds its own frame. No multi-item `with a, b` sugar.

## Checker

- Operand checks as a py chain or py value; the statement consumes the
  chain (no error-slot obligation, mirroring range).
- Body is a block scope. `break`/`continue` bind to an enclosing loop as
  usual and are legal inside the body; `return` is legal; narrowing and
  invalidation follow existing block rules.

## Runtime and bridge

- interp: on body completion inspect the `Flow`: `Normal`/`Break`/
  `Continue` and error-free `Return` take the None path; a `Return` whose
  final slot is an error takes the exception path. Fault (`Err`) skips
  exit and propagates.
- bridge additions, the only pyo3 surface:
  - `enter(h) -> Result<(), ErrVal>` (calls `__enter__`, drops the result);
  - `exit(h, err: Option<&ErrVal>) -> Result<bool, ErrVal>` (calls
    `__exit__` with None-triple or the synthesized exception; returns the
    result's truthiness).

## Spec changes

4.4 keyword list gains `with`; new section 8.9 With statements; 7.2.3
suppression list gains the with header; chapter 12 fault list gains the
`py with` conditions; 13.2 cross-references the statement.

## Verification

Golden tests (`py/`, stdlib modules only): enter and exit both run
(`threading.Lock` held inside the body, released after); exit sees None on
the normal path and the synthesized exception on a check propagation
(`sqlite3` in-memory transaction: insert then fail, table stays empty;
insert and succeed, row lands); suppression faults
(`contextlib.suppress(Exception)` around a propagating check, `.err`);
enter exception faults (`.err`); break and continue leave the lock
released; non-py operand and `with` outside py are compile errors (check
goldens). Then the real path: torch.no_grad in the training project
replaces its set_grad_enabled workaround.
