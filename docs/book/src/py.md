# The py bridge

Python is the ecosystem, not the runtime. `import py "torch"` gives you
the real torch, and everything that crosses the boundary arrives as
typed values with typed errors. Nothing in this chapter exists in Go;
it is the half of rikki that Go could never give you.

## Importing and calling

```rikki
import py "math"

fn main() (error?) {
    v := check float(math.sqrt(2))
    print(v > 1.41 && v < 1.42)     // true
    return none
}
```

`import py "modname"` imports a Python module through the bridge
(dotted paths like `"os.path"` work). Inside a project every py import
must be declared in the manifest (`rikki py add torch`), so a missing
dependency is a compile error, not a crash after the first epoch.

Values of type `py` are references to live Python objects. They are the
one dynamic type in the language: attribute access, calls, indexing,
and operators on them dispatch to Python at runtime.

## Chains: one fallible unit

A sequence of Python operations is one fallible unit. Any exception
anywhere in `model(x).loss.item()` becomes one rikki error at the point
where the chain is consumed; you do not handle each step.

```rikki
import py "json"

fn main() (error?) {
    parsed := check json.loads("{\"a\": 1}")
    n := check int(parsed["a"])
    print(n)                        // 1
    return none
}
```

Consume a chain with `check` (propagate), with `v, err :=` (bind), or
with a conversion, which absorbs the chain's fallibility. Letting a
chain's error drop on the floor is the usual compile error.

Python exceptions become error values with the full story attached:
`.msg` is the rendered exception, `.pytype` names the exception class,
`.traceback` carries the Python traceback text.

## Crossing the boundary

Rikki scalars pass into Python calls directly, as do lists and maps
(converted recursively). Named arguments pass through to Python
keywords: `torch.randn([784, 10], requires_grad: true)`. `@` is matrix
multiplication, defined when an operand is `py`.

Coming back is explicit: a conversion extracts a typed value from a
`py`, and the parse-like forms are fallible.

```
w := check float(logits.item())     // py to float, fallible
xs := check []float(tensor.tolist())
```

`for x := range e` iterates any Python iterable. `with` runs Python
context managers; a rikki error return inside the block reaches
`__exit__` as an exception, so transaction-shaped managers see the
error path exactly as Python would (see
[the tour](tour.md#with-python-context-managers)).

## What stays out

`py` values do not leak into the rest of the type system: you cannot
put one in a condition, compare one with `==` and get a `bool`, or
slice one. Every branch decision needs an extracted, typed value. That
line is what keeps a rikki program checkable while half of it lives in
CPython.
