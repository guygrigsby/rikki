# Errors and check

Errors are values, and dropping one is a compile error. Those two
sentences are most of the design; the rest is vocabulary.

## Error values

`error` is an ordinary type. Every error exposes `msg`, `cause` (the
wrapped inner error, or `none`), `origin` (the `file:line` where it was
born), and, for errors that crossed the Python bridge, `pytype` and
`traceback`. Construct with `error.new` and `error.wrap`:

```nevla
fn main() {
    e := error.new("boom")
    w := error.wrap(e, "while testing")
    print(w.msg)            // while testing
    c := w.cause
    if c != none {
        print(c.msg)        // boom: the chain stays structured
    }
}
```

A fallible function ends its result list with `error?`:

```
fn fetch(url str) (str, error?) { ... }
fn cleanup() (error?) { ... }
```

## Handling is mandatory

The checker rejects any path where an error is silently dropped.
Calling a fallible function as a bare statement, or binding one fewer
name than the value count, is the compile error "error result must be
handled". Handling means one of three things:

- bind it and decide: `v, err := f()`, then branch on `err != none`;
- propagate it: `check f()`;
- discard it explicitly: `v, _ := f()`. The underscore is legal and
  visible in review, which is the point.

## Recovery

Recovery is the two-value form plus a none-check; flow narrowing
unwraps the `error?` exactly like any other option:

```nevla
fn half(n int) (int, error?) {
    if n % 2 != 0 {
        return 0, error.new("odd number")
    }
    return n / 2, none
}

fn main() {
    v, err := half(42)
    if err != none {
        print("failed: " + err.msg)
        return
    }
    print(v)                // 21
}
```

## check propagates

`check e` evaluates a fallible expression. On success it yields the
value(s); on error the enclosing function returns immediately, the
error in its final slot and every other slot zero-filled. The enclosing
function must itself end in `error?`, so propagation is visible in
every signature it passes through.

```nevla
fn half(n int) (int, error?) {
    if n % 2 != 0 {
        return 0, error.new("odd number")
    }
    return n / 2, none
}

fn quarter(n int) (int, error?) {
    h := check half(n)      // on error: return it, zeros elsewhere
    return check half(h), none
}

fn main() {
    v, err := quarter(44)
    if err != none {
        print(err.msg)
        return
    }
    print(v)                // 11
}
```

A multi-value never travels as a unit: `return half(n)` is a compile
error even when the signatures line up. Bind and return
(`v, err := half(n); return v, err`) or propagate early
(`return check half(n), none`). The error slot stays visible at every
hop.

`main` may declare `(error?)`; returning an error from `main` prints it
and exits nonzero.

**Coming from Go:** `check` is the `if err != nil { return err }` you
were going to write anyway, reduced to one keyword and checked by the
compiler. The differences that matter: you cannot forget (dropping an
error does not compile), and wrapping keeps a typed cause chain instead
of one flattened string.

## Faults are not errors

Some failures do not return: index out of range, integer overflow,
division by zero. These are faults; they print a nevla stack trace and
terminate with a nonzero exit. Faults are deliberately not catchable,
and no user program can crash the process any other way (native code inside a C extension is the one documented boundary; see the intro). If a failure
is something a caller could reasonably handle, it is an error value; if
it is a bug in the program, it is a fault.
