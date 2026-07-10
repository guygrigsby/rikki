# A tour of rikki

Each example links to the playground, where the URL carries the program
itself; edit and rerun as you go.

## Hello

```rikki
fn main() {
    print("hello, rikki")
}
```

`fn main()` is the entry point. `print` renders anything. Four-space
indents and `rikki fmt` settles every other style question.

## Errors are values

```rikki
fn half(n int) (int, error?) {
    if n % 2 != 0 {
        return 0, error.new("odd number")
    }
    return n / 2, none
}

fn main() {
    v, err := half(42)
    if err != none {
        print("error: " + err.msg)
    } else {
        print(v)
    }
}
```

A fallible function ends its result list with `error?`. Dropping an error
is a compile error; you either bind it (`v, err :=`) and decide, or
propagate it with `check`, which requires your own function to return
`error?`:

```rikki
fn quarter(n int) (int, error?) {
    h := check half(n)      // on error: return it, zero values elsewhere
    return check half(h), none
}
```

A multi-value never travels as a unit, so Go's `return half(n)` is a
compile error here. Propagate early with `return check half(n), none`,
or bind and decide: `v, err := half(n); return v, err`. The error slot
stays visible at every hop.

Handle errors at the layer that can do something about them; propagate
only when the caller owns the decision.

## Options, no nil

```rikki
fn main() {
    m := map[str]int{"a": 1}
    v := m["a"]             // a map read is int?: present or none
    if v != none {
        print(v + 1)        // narrowed to int inside the branch
    }
}
```

There is no nil. Absence is an option type (`int?`), and the checker
makes you look before you touch: using `v` unnarrowed is a compile
error.

## The copy model

```rikki
struct User {
    Name str
    Age int
}

fn main() {
    u := User{Name: "rikki", Age: 1}
    v := u
    v.Age = 99
    print(u.Age)    // 1: structs copy

    xs := [1, 2, 3]
    ys := xs
    ys[0] = 99
    print(xs[0])    // 99: lists are references
}
```

Go's split: scalars, strings, and structs copy on assignment; lists,
maps, functions, and py values are references. Closures capture by
reference.

## Modules and visibility

`import "util.rk"` binds a sibling file as module `util`. Capitalized
top-level names are exported; lowercase is private to its file, fields
included. The Go rule, no keywords.

## The py bridge

```rikki
import py "torch"

fn main() (error?) {
    w := check torch.randn([784, 10], requires_grad: true)
    x := check torch.randn([32, 784])
    logits := check (x @ w)
    print(check str(logits.shape))
    return none
}
```

A chain of Python operations is one fallible unit: any exception anywhere
in `model(x).loss.item()` becomes one rikki error at the point of
consumption. Keyword arguments pass through, `@` is matrix
multiplication, `for range` iterates any Python iterable, and
`with torch.no_grad() { }` runs context managers. Inside a project,
every `import py` must be declared (`rikki py add torch`), so a missing
dependency is a compile error rather than a crash twenty minutes into a
run.
