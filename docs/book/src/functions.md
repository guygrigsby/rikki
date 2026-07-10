# Functions and structs

## Declaring functions

`fn` declares a function. Every parameter of a top-level function has a
declared type, results come after the parameter list, and a function
with results must return on every path; the checker rejects a missing
return at compile time, not twenty minutes into a run.

```rikki
fn double(n int) int {
    return n * 2
}

fn divmod(a int, b int) (int, int) {
    return a / b, a % b
}

fn main() {
    print(double(21))       // 42
    q, r := divmod(7, 3)
    print(q, r)             // 2 1
}
```

A function may declare several results. A call with multiple results is
a multi-value: bind every component (`q, r := divmod(7, 3)`) or, when
the last result is `error?`, propagate with `check`. Multi-values are
not first class; they cannot be stored, nested, or returned as a unit
(the [errors chapter](errors.md) covers the idioms).

A function with no results returns implicitly at the end of its body; a
bare `return` inside one exits early.

## Function values and literals

Functions are first class. A declared function is a value; a function
literal (`fn(x int) int { return x * 2 }`) is an expression. In a
context that supplies the function type, parameter types can be
omitted, and a literal whose body is a single expression returns that
expression's value:

```rikki
fn main() {
    nums := [1, 2, 3, 4]
    big := nums.map(fn(x) { x * 2 }).filter(fn(x) { x > 2 }).sum()
    print(big)              // 18
}
```

Closures capture by reference, as in Go: the closure and the enclosing
scope share the variable, and writes flow both ways.

```rikki
fn main() {
    total := 0
    add := fn(x int) { total = total + x }
    add(1)
    add(2)
    print(total)            // 3
}
```

Loop variables are fresh per iteration (Go 1.22 semantics), so closures
made in different rounds capture different variables.

## Structs

`struct` declares a nominal record type. Fields are separated by commas
or line breaks. A struct literal names the type and supplies every
field exactly once, in any order:

```rikki
struct User {
    Name str
    Age  int
}

fn main() {
    u := User{Age: 1, Name: "rikki"}
    u.Age = 2
    print(u.Name, u.Age)    // rikki 2
}
```

Structs are value types: assignment and argument passing copy (the
[copy model](values.md) chapter has the full split). Capitalization
controls visibility across modules, Go's rule
([modules](modules.md)). User structs have no methods in v1; write
functions that take the struct.

A struct must not contain itself by value; such a value could never be
constructed. Break the cycle with an option:

```rikki
struct Node {
    val  int
    next Node?      // `next Node` would be a compile error
}

fn main() {
    n := Node{val: 1, next: none}
    print(n.val)            // 1
}
```
