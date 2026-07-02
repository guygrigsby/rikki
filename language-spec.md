# The Rikki Programming Language Specification

Version 1 (v1). This document is the normative reference for the rikki
language: it states what a conforming implementation must do. Rationale and
design history live in the design document at
`docs/specs/2026-07-01-mongoose-v1-design.md`; where that document and this
one could disagree, this one governs the language, and the golden tests under
`tests/golden/` are its executable companion (see the final chapter).

Source files use the `.rk` extension.

## Table of contents

1. [Introduction](#1-introduction)
2. [Notation](#2-notation)
3. [Source code representation](#3-source-code-representation)
4. [Lexical elements](#4-lexical-elements)
5. [Types](#5-types)
6. [Declarations and scope](#6-declarations-and-scope)
7. [Expressions](#7-expressions)
8. [Statements](#8-statements)
9. [Flow narrowing](#9-flow-narrowing)
10. [Errors and error handling](#10-errors-and-error-handling)
11. [Value semantics and equality](#11-value-semantics-and-equality)
12. [Runtime faults](#12-runtime-faults)
13. [The Python bridge](#13-the-python-bridge)
14. [Builtin functions](#14-builtin-functions)
15. [Standard library](#15-standard-library)
16. [Modules and multi-file programs](#16-modules-and-multi-file-programs)
17. [Program execution](#17-program-execution)
18. [Implementation limits](#18-implementation-limits)
19. [Conformance and maintenance](#19-conformance-and-maintenance)

## 1. Introduction

Rikki is a statically typed, interpreted language with value semantics,
errors as values, option types instead of nil, and an embedded Python bridge.
A rikki program is checked in full before any of it runs; a program that
passes the checker cannot crash the hosting process at runtime. The worst
outcomes available to a running program are returning an error from `main`
and a runtime fault, both of which terminate the program in a controlled way
(chapter 12, chapter 17).

Throughout this document, "must" states a requirement on conforming
implementations or on valid programs, "may" states a permission, and
"unspecified in v1" marks behavior a program must not rely on.

## 2. Notation

The syntax is specified using Extended Backus-Naur Form (EBNF) in the style
of the Go specification:

```
Syntax      = { Production } .
Production  = production_name "=" Expression "." .
Expression  = Term { "|" Term } .
Term        = Factor { Factor } .
Factor      = production_name | token | Group | Option | Repetition .
Group       = "(" Expression ")" .
Option      = "[" Expression "]" .
Repetition  = "{" Expression "}" .
```

Productions are expressions built from terms and the following operators, in
increasing precedence:

```
|   alternation
()  grouping
[]  option (0 or 1 time)
{}  repetition (0 to n times)
```

Lowercase production names denote lexical tokens. Non-terminals are in
CamelCase. Terminal symbols appear in double quotes `""`.

The token `newline` denotes the line-terminator token produced by the lexer
(section 4.2). Where a production does not mention `newline`, line breaks are
significant unless section 4.2 permits them.

## 3. Source code representation

Source code is Unicode text encoded in UTF-8. A source file is a sequence of
Unicode code points; the lexer processes them directly, without any
normalization.

### 3.1 Shebang line

If the first two characters of a source file are `#!`, the entire first line
up to and including nothing beyond the first line feed is ignored by the
lexer. The line feed itself is retained for line counting, so diagnostics on
subsequent lines report true line numbers. This permits executable scripts:

```
#!/usr/bin/env tk
fn main() {
    print("hello")
}
```

The shebang form is recognized only at the very start of the file. A `#`
anywhere else in a program is a lexical error.

### 3.2 Characters

Identifiers and keywords are restricted to ASCII (section 4.3). Arbitrary
Unicode may appear in string literals and comments. String indexing, slicing,
and `len` operate on Unicode code points, not bytes (sections 7.5, 7.6, 14.4).

## 4. Lexical elements

### 4.1 Comments

A comment starts with `//` and runs to the end of the line. There is no block
comment form. Comments do not produce tokens and do not suppress the newline
that ends their line.

```
x := 1  // this is a comment
```

### 4.2 Tokens and line terminators

Tokens are identifiers, keywords, operators and punctuation, and literals.
Space (U+0020), horizontal tab, and carriage return are white space and are
ignored except as token separators.

A line feed produces a `newline` token, with these rules:

- Consecutive line feeds produce a single `newline` token.
- Line feeds before the first token of the file produce no token.
- The `newline` token acts as the statement terminator inside blocks
  (section 8.1) and is otherwise skipped only where the grammar permits.

There are no semicolons; `;` is not a token and its appearance is a lexical
error.

Line breaks are permitted, and consumed without producing a statement break,
in the following positions:

- after a binary operator (`x := 1 +` may be continued on the next line);
- after the opening delimiter of a parenthesized expression, list literal,
  call argument list, parameter list, parenthesized return-type list, map
  literal body, or struct literal body;
- after a comma in any of those constructs;
- before the closing delimiter of those constructs.

A line break is not permitted before a binary operator, before a `.` selector,
before an `else` (which must follow its `}` on the same line; section 8.6),
or in the middle of any other production. In particular, a method chain must
keep each `.` on the same line as the expression it follows.

```
ok := 1 +
    2

// invalid: line break before the operator
// bad := 1
//     + 2
```

### 4.3 Identifiers

```
identifier = letter { letter | ascii_digit } .
letter     = "a" ... "z" | "A" ... "Z" | "_" .
```

Identifiers name variables, functions, structs, fields, parameters, and
modules. They consist of ASCII letters, ASCII digits, and underscore, and must
not start with a digit. Identifiers are case sensitive.

The identifier `_` is the blank identifier (section 6.6).

### 4.4 Keywords

The following identifiers are reserved and must not be used as names:

```
break     check     continue  else      false     fn
for       if        import    in        none      py
return    struct    true
```

`py` is a keyword; it also serves as the name of the `py` type (section 5.8)
and as the marker in `import py` declarations (section 6.4).

The names `int`, `float`, `bool`, `str`, `error`, and `map` are not
keywords. They are predeclared type names, resolved contextually; `int`,
`float`, `str`, and `bool` followed immediately by `(` in expression position
always denote a conversion (section 7.7), and `map [` in expression position
always begins a map literal. Slice types and slice conversions are written
with the `[]` prefix (`[]int`, `[]float(x)`; sections 5.9, 7.7) and involve
no reserved name. Builtin function names (`print`, `printf`, `sprintf`,
`len`, `range`, `args`, `input`) are also not reserved; a variable or
function declaration with the same name shadows the builtin.

### 4.5 Operators and punctuation

```
+    -    *    /    %
==   !=   <    <=   >    >=
&&   ||   !
=    :=   ?    :
(    )    [    ]    {    }
,    .
```

A single `&` or a single `|` is a lexical error. All other characters not
covered by this chapter are lexical errors.

### 4.6 Integer literals

```
int_lit = ascii_digit { ascii_digit } .
```

Integer literals are decimal only. There is no sign in the literal itself;
negative values are formed with the unary `-` operator. An integer literal
must fit in the range of `int` values, 0 through 9223372036854775807
(2^63 - 1); a larger literal is a lexical error. Consequently the minimum
`int` value, -9223372036854775808, is not writable as a negated literal; it
can only be produced by arithmetic.

### 4.7 Float literals

```
float_lit = int_lit "." ascii_digit { ascii_digit } .
```

A float literal is a digit sequence, a period, and at least one further digit.
There is no exponent form and no leading or trailing period form. A period
not followed by a digit is not part of a numeric literal, so `1.abs` lexes as
the integer `1`, `.`, and the identifier `abs`.

```
2.5     // float
3.0     // float
1       // int
```

### 4.8 String literals

```
string_lit = `"` { unicode_char | escape } `"` .
escape     = `\` ( "n" | "t" | `"` | `\` ) .
```

String literals are delimited by double quotes and must not span multiple
lines; a line feed inside a string literal is a lexical error, as is an
unterminated string. Exactly four escape sequences are recognized:

| Escape | Meaning |
|--------|---------|
| `\n`   | line feed (U+000A) |
| `\t`   | horizontal tab (U+0009) |
| `\"`   | double quote |
| `\\`   | backslash |

Any other character after a backslash is a lexical error. All other
characters, including arbitrary Unicode, stand for themselves.

## 5. Types

The complete set of v1 types is:

```
int   float   bool   str
[]T   map[K, V]   T?
fn(...) ...   struct types   error   py
```

Two types are identical if and only if they have the same structure: the same
kind, and identical element, key, value, parameter, and result types. Struct
types are identical when they have the same name.

### 5.1 Boolean, numeric, and string types

- `bool` has exactly the values `true` and `false`. There is no truthiness:
  no other type converts implicitly to `bool`, and conditions must have type
  `bool` (section 8.6).
- `int` is a 64-bit signed integer. Overflow is a runtime fault (chapter 12),
  never a silent wrap.
- `float` is an IEEE 754 64-bit binary floating point number. Float
  arithmetic follows IEEE 754: division by zero yields an infinity, `0.0/0.0`
  yields NaN, and no float arithmetic faults.
- `str` is an immutable sequence of Unicode code points.

There are no implicit conversions between any of these types, including
between `int` and `float` (section 7.9.1).

### 5.2 List types

`[]T` is an ordered sequence of values of element type `T`. Lists have
value semantics (chapter 11): assignment copies the whole list.

### 5.3 Map types

`map[K, V]` maps keys of type `K` to values of type `V`. The key type `K`
must be `int`, `str`, or `bool`; any other key type is a compile-time error.
Maps preserve insertion order: iteration, `keys()`, and `values()` visit
entries in the order the keys were first inserted, and `delete` preserves the
relative order of the remaining entries. Maps have value semantics.

Reading `m[k]` yields `V?`: a missing key reads as `none` (section 7.5).
Writing `m[k] = v` inserts or updates (section 8.3).

### 5.4 Option types

For any non-option type `T`, the option type `T?` holds either a value of
type `T` or the absent value `none`. There is no nil; absence exists only
inside option types.

- `none` is the untyped empty-option literal. It is assignable to every
  option type and compares only against operands of option type
  (section 7.9.3).
- A value of type `T` is assignable where `T?` is expected (widening,
  section 5.10). The reverse never holds implicitly.
- A value of option type must be narrowed (chapter 9) before its fields or
  methods can be used, and it does not support the operators of `T`. Using an
  unchecked option is a compile-time error:

```
fn find() User? { ... }

u := find()
print(u.name)        // compile error: value might be none
if u != none {
    print(u.name)    // ok: u is User here
}
```

Option types do not nest syntactically: `T??` is not writable.

### 5.5 Function types

`fn(P1, ..., Pn) R` and `fn(P1, ..., Pn) (R1, ..., Rm)` are the types of
function values with the given parameter and result types. A function type
with no results is written `fn(P1, ..., Pn)`. Functions are first class:
declared functions and function literals are values of function type and may
be stored, passed, and returned.

In a function-type result position, an unparenthesized result must begin with
an identifier or `py`; a result that is itself a function type must be
parenthesized: `fn() (fn() int)`.

### 5.6 Struct types

A struct type is declared at the top level (section 6.3) and consists of an
ordered list of named, typed fields. Structs are nominal: two structs with
the same fields but different names are distinct types. Struct values have
value semantics; fields are accessed with `.` and assigned through assignment
statements. User struct types have no methods in v1.

Recursive structs are restricted: a struct must not contain itself by value,
directly or through a chain of struct-typed fields. Such a value could never
be constructed, and the declaration is a compile-time error. The cycle must
be broken with an option, list, or map along the way:

```
struct Node {
    val int
    next Node?      // ok; `next Node` would be a compile error
}
```

### 5.7 The error type

`error` is the type of error values (chapter 10). Every error value exposes
the fields:

| Field | Type | Meaning |
|-------|------|---------|
| `msg` | `str` | human-readable message |
| `cause` | `error?` | wrapped inner error, or `none` |
| `pytype` | `str` | Python exception type name; `""` for non-bridge errors |
| `traceback` | `str` | Python traceback text; `""` for non-bridge errors |

Error values are constructed with `error.new` and `error.wrap`
(section 15.1) and by the Python bridge (chapter 13). Error values do not
support `==` (section 7.9.3); test for presence with `err != none` on an
`error?` and inspect `.msg`.

### 5.8 The py type

`py` is the type of references to live Python objects (chapter 13). It is
the single dynamic type in the language and the documented exception to value
semantics: assigning a `py` value copies the reference, not the object.

The zero value of `py` is a handle to Python's `None`; operations on it
produce ordinary Python error values (for example `AttributeError`), never a
fault.

### 5.9 Type syntax

```
Type      = BaseType [ "?" ] .
BaseType  = TypeName | SliceType | MapType | FnType | "py" .
TypeName  = identifier [ "." identifier ] .
SliceType = "[" "]" Type .
MapType   = "map" "[" Type "," Type "]" .
FnType    = "fn" "(" [ TypeList ] ")" [ FnResult ] .
FnResult  = "(" [ TypeList ] ")" | Type .
TypeList  = Type { "," Type } .
```

A `TypeName` is one of the predeclared names `int`, `float`, `bool`, `str`,
`error`, a struct name, or a dotted name `module.Struct` referring to a
struct declared in an imported file module (chapter 16). Any other name is a
compile-time error ("unknown type"). The unparenthesized `FnResult`
alternative must begin with an identifier or `py` (section 5.5).

### 5.10 Assignability

A value of type `V` is assignable to a location (variable, parameter, field,
element, return slot) of type `T` when:

- `T` and `V` are identical; or
- `T` is `T0?` and `V` is assignable to `T0`, or `V` is `V0?` and `V0` is
  assignable to `T0` (widening into options, applied recursively); or
- `T` is `[]A`, `V` is `[]B`, and `B` is assignable to `A`; or
- `T` is `map[AK, AV]`, `V` is `map[BK, BV]`, and `BK`, `BV` are assignable
  to `AK`, `AV` respectively; or
- either side's type could not be determined because of a prior compile
  error (assignability then does not produce a second error).

The literal `none` is assignable to every option type. An empty list literal
`[]` is assignable to every list type, but only in a context that supplies
the list type (section 7.2.1).

### 5.11 Zero values

Every type has a zero value, used to fill the non-error result slots when a
`check` expression propagates an error (section 7.8) and as the success slot
of failed conversions and stdlib calls:

| Type | Zero value |
|------|-----------|
| `int` | `0` |
| `float` | `0.0` |
| `bool` | `false` |
| `str` | `""` |
| `[]T` | `[]` |
| `map[K, V]` | empty map |
| `T?` | `none` |
| struct | struct with every field set to its zero value, recursively |
| `error` (bare, non-option) | `none` |
| `py` | a handle to Python `None` |
| `fn(...) ...` | the zero function value; calling it is a runtime fault |

## 6. Declarations and scope

### 6.1 Program structure

```
SourceFile  = { Declaration } .
Declaration = ImportDecl | StructDecl | FunctionDecl .
```

A source file consists solely of import, struct, and function declarations,
separated by any number of line breaks. There are no top-level variables and
no constants. Top-level declarations are visible throughout the program
regardless of order; a function may call a function declared later in the
file.

Declaring two functions with the same name, or two structs with the same
name, is a compile-time error.

A complete program must declare `fn main` with an entry-point signature
(section 17.1).

### 6.2 Function declarations

```
FunctionDecl = "fn" identifier Parameters [ Result ] Block .
Parameters   = "(" [ ParameterList [ "," ] ] ")" .
ParameterList = Parameter { "," Parameter } .
Parameter    = identifier [ Type ] .
Result       = Type | "(" [ TypeList [ "," ] ] ")" .
```

Every parameter of a top-level function must have a declared type; omitting
one is a compile-time error. (The grammar shares `Parameter` with function
literals, where types may be inferred; section 7.3.)

A function may declare zero, one, or several result types. A function with a
nonempty result list must end in a statement that diverges (section 8.1.1)
on every path; otherwise "missing return" is a compile-time error. A function
with no results returns implicitly at the end of its body, and `return` with
no values is permitted inside it.

```
fn fetch(url str) (str, error?) {
    return "", none
}
```

### 6.3 Struct declarations

```
StructDecl = "struct" identifier "{" { newline } [ FieldDeclList ] "}" .
FieldDeclList = FieldDecl { ( "," | newline ) { newline } FieldDecl } [ "," | newline ] { newline } .
FieldDecl  = identifier Type .
```

Fields are separated by commas or line breaks; both of these are valid:

```
struct User { name str, age int }

struct User {
    name str
    age int
}
```

The recursive-struct restriction of section 5.6 applies.

### 6.4 Import declarations

```
ImportDecl = "import" [ "py" ] string_lit .
```

Three forms exist, distinguished by the `py` marker and the path string:

- `import "name"` where `name` is one of the standard library modules
  `math`, `error`, `file`, `ctx`, `http` imports that module (chapter 15).
- `import "path.rk"` where the path ends in `.rk` imports another rikki
  source file as a module (chapter 16).
- `import py "modname"` imports a Python module through the bridge
  (chapter 13). Dotted module paths (`import py "os.path"`) are permitted.

An import path that is none of these is a compile-time error
("unknown module").

### 6.5 Blocks and scope

```
Block = "{" { newline } [ StatementList ] "}" .
```

Each block introduces a new scope. A short variable declaration (section 8.2)
declares its names in the innermost enclosing scope; declaring a name twice
in the same scope is a compile-time error, while an inner scope may shadow an
outer name. Function parameters are declared in the function's outermost
scope. `for x in xs` declares its iteration variables in a scope enclosing
the loop body, fresh on each iteration.

Name resolution inside a function proceeds from the innermost scope outward,
then to top-level functions, then to imported module names, then to builtins.
A local variable therefore shadows a top-level function of the same name,
which shadows a module name, which shadows a builtin.

### 6.6 The blank identifier

The blank identifier `_` discards a value. It may appear:

- as a name in a short variable declaration: `_, err := f()` or `_ := f()`;
- as an iteration variable: `for _, v in m { ... }`.

`_` is never declared: it cannot be read, and it is not a valid assignment
target (`_ = f()` is a compile-time error, "undefined: _"). Binding an error
slot to `_` counts as handling it (section 10.2): `v, _ := f()` is legal and
deliberately drops the error.

## 7. Expressions

### 7.1 Operands

```
PrimaryExpr = int_lit | float_lit | string_lit
            | "true" | "false" | "none"
            | identifier
            | "(" Expression ")"
            | ListLit | MapLit | StructLit
            | FunctionLit
            | Conversion .
```

An identifier denotes, in resolution order, a variable in scope, a top-level
function (as a function value), or an imported module. An identifier that
resolves to nothing is a compile-time error ("undefined").

A module name is not a first-class value: it may only appear as the receiver
of a selector (`math.pi`, `util.double(2)`).

### 7.2 Composite literals

#### 7.2.1 List literals

```
ListLit = "[" { newline } [ ExpressionList [ "," ] ] { newline } "]" .
ExpressionList = Expression { "," { newline } Expression } .
```

A nonempty list literal has type `[]E` where `E` is the element type
supplied by context, or, absent context, the type of the first element; each
subsequent element must be assignable to `E`.

An empty list literal `[]` requires a context that supplies its list type: an
argument position, return position, field value, map value, or assignment to
a location of known list type. A bare `[]` with no such context is a
compile-time error ("cannot infer element type of []").

```
fn total(xs []int) int { return len(xs) }

print(total([]))     // ok: parameter supplies []int
// xs := []          // compile error
```

#### 7.2.2 Map literals

```
MapLit   = "map" "[" Type "," Type "]" "{" { newline } [ MapEntryList [ "," ] ] { newline } "}" .
MapEntryList = MapEntry { "," { newline } MapEntry } .
MapEntry = Expression ":" Expression .
```

A map literal names its key and value types explicitly. Each key must be
assignable to `K` and each value to `V`; `K` must satisfy the map key
restriction (section 5.3). Entries are inserted left to right; a repeated key
overwrites, keeping the original insertion position.

```
m := map[str, int]{"a": 1, "b": 2}
```

#### 7.2.3 Struct literals

```
StructLit  = identifier "{" { newline } [ FieldValueList [ "," ] ] { newline } "}" .
FieldValueList = FieldValue { "," { newline } FieldValue } .
FieldValue = identifier ":" Expression .
```

A struct literal must name a declared struct type and must supply every
declared field exactly once, each value assignable to the field's type.
Missing fields and unknown fields are compile-time errors. Field order in the
literal is free; the constructed value's field order follows the declaration.

Struct literals are suppressed in control-flow headers: in the condition of
an `if` or `else if`, and in the header expression of a `for` statement
(both the condition form and the iterable of `for ... in`), an identifier
followed by `{` is not parsed as a struct literal, so the `{` opens the
statement's block. To use a struct literal in a header, parenthesize it.
Struct literals remain available inside any parenthesized or bracketed
subexpression of a header.

The struct type `Ctx` (section 15.4) is opaque and cannot be constructed by
a struct literal; `Ctx{...}` is a compile-time error.

### 7.3 Function literals

```
FunctionLit = "fn" Parameters [ Result ] Block .
```

A function literal (lambda) evaluates to a function value. Parameter types
may be omitted when the context supplies an expected function type, from
which they are inferred positionally; otherwise omitting a parameter type is
a compile-time error ("lambda parameter needs a type here"). Contexts that
supply a function type include arguments to list methods (`map`, `filter`,
`each`, `sorted_by`), arguments to parameters of function type, and
assignment to a location of known function type.

Result typing:

- If the literal declares result types, its body must diverge on every path,
  exactly as for declared functions, and `check` may be used inside it
  subject to section 7.8.
- If the literal declares no result types and its body is a single
  expression statement, the literal is an expression-bodied function: its
  result type is the expression's type (no result if the expression has no
  value), and calling it returns the expression's value.
- Otherwise the literal has no results.

```
nums := [1, 2, 3, 4]
big := nums.map(fn(x) { x * 2 }).filter(fn(x) { x > 2 }).sum()   // 18

f := fn(x int) int { return x * 2 }
```

Function literals capture by value: at the moment the literal is evaluated,
the variables visible at that point are snapshotted (deep-copied, per value
semantics), and the function body reads those copies. Later mutation of the
originals is invisible to the closure, and the closure cannot mutate the
originals:

```
n := 1
f := fn() int { return n }
n = 2
print(f())    // 1
```

Top-level functions and imported modules are not captured; they resolve
normally at call time.

### 7.4 Selectors, calls, and method calls

```
PostfixExpr = PrimaryExpr { Selector | Arguments | Index | Slice } .
Selector    = "." identifier .
Arguments   = "(" { newline } [ ArgumentList [ "," ] ] { newline } ")" .
ArgumentList = Argument { "," { newline } Argument } .
Argument     = Expression | identifier ":" Expression .
```

`x.f` selects, depending on the type of `x`:

- a struct field, whose type is the field's declared type;
- an error field (`msg`, `cause`, `pytype`, `traceback`; section 5.7);
- a member of a module: a stdlib constant (`math.pi`, `math.e`) reads as its
  value; a module function member (stdlib or file module) must be called
  directly (`math.sqrt(4.0)`, `util.double(3)`). Module functions are not
  first class in v1: `f := math.sqrt` is a compile-time error;
- a Python attribute, when `x` has type `py` (chapter 13).

Selecting through an option type is a compile-time error; narrow first
(chapter 9).

`f(a1, ..., an)` calls a function value. The argument count must equal the
parameter count and each argument must be assignable to the corresponding
parameter type. A call of a non-function is a compile-time error
("not callable").

`x.m(a1, ..., an)` is a method call. Methods exist only on strings, slices,
and maps (section 14.8), on `Ctx` (section 15.4), on modules
(where `mod.f(...)` calls the module function), on `error` as the receiver of
the builtin constructors `error.new` and `error.wrap` (section 15.1), and on
`py` values (chapter 13). User struct types have no methods in v1.

A named argument (`identifier ":" Expression`) binds the value to the named
Python parameter and is permitted only in calls whose callee is a py value
(chapter 13); a named argument in any other call is a compile-time error, as
is a positional argument following a named one. Rikki functions are
positional only.

Arguments are evaluated left to right, after the callee (or receiver)
expression. Calls with multiple results are covered in section 7.10.

### 7.5 Index expressions

```
Index = "[" { newline } Expression { newline } "]" .
```

For `a[i]`:

- If `a` has type `[]T`, `i` must be `int` and the result is `T`.
  Indices run from 0. An index outside `0 <= i < len(a)` is a runtime fault.
  Negative indices are not supported.
- If `a` has type `str`, `i` must be `int` and the result is a `str` holding
  the single code point at position `i` (positions count code points).
  Out of range is a runtime fault.
- If `a` has type `map[K, V]`, `i` must be assignable to `K` and the result
  is `V?`; a missing key yields `none`. Reading a map never faults.
- If `a` has type `py`, indexing is a Python subscript operation
  (chapter 13).

Indexing any other type is a compile-time error.

```
m := map[str, int]{"k": 1}
v := m["missing"]     // v: int?, none here
if v != none {
    print(v + 1)
}
```

### 7.6 Slice expressions

```
Slice = "[" { newline } Expression ":" Expression "]" .
```

`a[lo:hi]` slices a `[]T` (yielding `[]T`) or a `str` (yielding
`str`, positions in code points). Both bounds are required and must be `int`.
The bounds must satisfy `0 <= lo <= hi <= len(a)`; anything else is a runtime
fault. The result is a copy of the half-open range `[lo, hi)`; `a[n:n]` is
empty. Slicing any other type, including `py`, is a compile-time error.

### 7.7 Conversions

```
Conversion = ( "int" | "float" | "str" | "bool" ) "(" Expression ")"
           | SliceType "(" Expression ")" .
```

Conversions are the explicit, fallible casts of the language. Every
conversion expression has the multi-value type `(T, error?)` and must be
consumed accordingly (sections 7.10, 10.2), even when it cannot in fact fail:

```
n, err := int("42")       // 42, none
m, err2 := int("x")       // 0, error("cannot parse \"x\" as int")
s, _ := str(123)          // "123", never fails
```

Permitted operand types and behavior, for non-`py` operands:

| Conversion | Operand types | Behavior |
|------------|---------------|----------|
| `int(x)` | `int` | identity |
| | `float` | truncation toward zero; values outside the `int` range saturate to the nearest bound, NaN yields 0 |
| | `str` | decimal parse after trimming leading and trailing white space; failure is an error value |
| `float(x)` | `float` | identity |
| | `int` | exact or nearest representable value |
| | `str` | float parse after trimming; failure is an error value |
| `bool(x)` | `bool` | identity |
| | `str` | after trimming, exactly `"true"` or `"false"`; anything else is an error value |
| `str(x)` | any type | the canonical rendering of section 14.1; never fails |
| `[]T(x)` | `[]U` | yields the operand list unchanged; element types are not validated in v1 (see below) |

Any other operand type is a compile-time error ("cannot convert").

`[]T` applied to a rikki list performs no per-element checking in v1:
the list value passes through, and the expression's static type becomes
`[]T`. If the actual elements do not match `T`, later operations on them
fault at runtime. Programs must not rely on this as a checked cast; its
intended use is extraction from `py` values, where elements are genuinely
converted (section 13.5).

Conversions applied to `py` operands are the outbound bridge conversions and
are specified in section 13.5. A conversion applied to a py chain absorbs the
chain's fallibility: if the chain raised, the conversion yields
`(zero value of T, the error)`.

### 7.8 check expressions

```
UnaryExpr = PostfixExpr | ( "check" | "!" | "-" ) UnaryExpr .
```

`check` is a prefix operator over a unary-postfix chain: it binds the whole
selector/call/index chain to its right, but nothing past a binary operator.
`check f() + 1` means `(check f()) + 1`; `check torch.randn([2, 3])` applies
to the whole call.

`check e` requires:

1. The enclosing function's declared result list must end in `error?`;
   otherwise "check requires enclosing function to return error?" is a
   compile-time error. This applies per function; a `check` inside a function
   literal looks at the literal's declared results.
2. `e` must be fallible: its type must be `(T1, ..., Tn, error?)` for
   `n >= 0` (including a lone `error?`, and including a py chain, which is
   fallible as a unit; section 7.11). Otherwise "check needs a fallible
   expression" is a compile-time error.

Semantics: evaluate `e`. If its error slot is `none`, the `check` expression
yields the remaining values: no value for `n = 0`, the single value for
`n = 1`, the multi-value for `n > 1`. If the error slot holds an error, the
enclosing function returns immediately: its final result slot is the error,
and every preceding result slot is filled with the zero value of its declared
type (section 5.11).

```
fn boom() (int, str, error?) {
    return 0, "", error.new("bad")
}

fn run() (int, str, error?) {
    a, b := check boom()    // on error: run returns 0, "", the error
    return a, b, none
}
```

Applied to a py chain, `check` yields the chain's `py` value on success and
propagates the converted Python exception on failure (section 13.3).

### 7.9 Operators

Binary operators, in increasing precedence:

| Precedence | Operators |
|------------|-----------|
| 1 | `\|\|` |
| 2 | `&&` |
| 3 | `==` `!=` |
| 4 | `<` `<=` `>` `>=` |
| 5 | `+` `-` |
| 6 | `*` `/` `%` |

All binary operators are left associative. Unary operators (`!`, `-`,
`check`) bind tighter than any binary operator.

```
Expression = UnaryExpr | Expression binary_op Expression .
binary_op  = "||" | "&&" | "==" | "!=" | "<" | "<=" | ">" | ">="
           | "+" | "-" | "*" | "/" | "%" .
```

#### 7.9.1 Arithmetic operators

| Operator | Operand types | Result |
|----------|---------------|--------|
| `+` `-` `*` `/` `%` | `int`, `int` | `int` |
| `+` `-` `*` `/` | `float`, `float` | `float` |
| `+` | `str`, `str` | `str` (concatenation) |
| `+` | `[]A`, `[]B` | list concatenation, see below |

`int` and `float` never mix: `1 + 2.5` is a compile-time error
("int and float do not mix"). `%` is defined on `int` only. Integer `/` and
`%` fault on a zero divisor; integer `+`, `-`, `*`, `/`, `%`, and unary `-`
fault on overflow (chapter 12). Integer division truncates toward zero.
Float arithmetic never faults (section 5.1).

List concatenation requires one element type to be assignable to the other;
the result takes the wider element type, so concatenation widens toward
options and never narrows:

```
xs := [1]              // []int
ys := [maybe()]        // []int?
zs := xs + ys          // []int?
```

If neither element type accepts the other, concatenation is a compile-time
error. When either operand of any of these operators has type `py`, the
operation is a bridge operation instead (section 13.2).

Unary `-` requires `int` or `float`; unary `!` requires `bool`.

#### 7.9.2 Comparison operators

`<`, `<=`, `>`, `>=` are defined on `int` with `int`, `float` with `float`,
and `str` with `str` (lexicographic by code point). The result is `bool`.
All other operand combinations are compile-time errors. Because comparisons
are left associative and yield `bool`, a chained comparison such as
`a < b < c` parses but is rejected by the type checker.

#### 7.9.3 Equality operators

`==` and `!=` yield `bool` and are defined in exactly two shapes:

- Scalar equality: both operands have the same type, which must be `int`,
  `float`, `bool`, or `str`. Float equality follows IEEE 754 (NaN is not
  equal to itself).
- None comparison: one operand is the literal `none` and the other has an
  option type. `x == none` is true iff `x` is absent. Comparing `none`
  against a non-option operand is a compile-time error
  ("none only compares to option types"). `none == none` is true.

Everything else, including list, map, struct, error, fn, and option-to-option
comparison, is a compile-time error ("cannot compare"). Structural equality
is available through the `contains` method (section 11.2). When either
operand is `py`, equality is a bridge operation yielding `py`
(section 13.2).

#### 7.9.4 Logical operators

`&&` and `||` require `bool` operands and yield `bool`. They short-circuit:
`a && b` does not evaluate `b` when `a` is false; `a || b` does not evaluate
`b` when `a` is true. `py` operands are not permitted.

### 7.10 Multiple values

A call of a function whose type declares `n >= 2` results, a conversion
(always `(T, error?)`), and a py chain at its point of consumption
(`(py, error?)`) produce a multi-value. Multi-values are not first class:
they cannot be stored, nested, or passed on. A multi-value may be consumed
only:

- by a short variable declaration whose name count equals the value count:
  `a, b := f()`;
- by a `check` expression (which strips the error slot; section 7.8).

In particular, a multi-value cannot be forwarded by a `return` statement as
a unit; each result is returned by listing expressions (section 8.5).

Using a multi-value in a single-value context is a compile-time error
("multiple values in single-value context"), and binding it to the wrong
number of names is a compile-time error, with the special diagnostic
"error result must be handled" when exactly the trailing `error?` was left
unbound (section 10.2).

### 7.11 py chains

An expression is a py chain when it applies an operation to a value of type
`py`: attribute selection, call, method call, indexing, or a binary operator
with a `py` operand. Within further postfix or binary operations the chain
continues to act as `py`, so consecutive Python steps need no per-step
handling; at its point of consumption the chain as a whole has type
`(py, error?)`:

```
import py "json"

fn main() (error?) {
    a := check json.loads("40")        // whole call is one fallible unit
    b := check (a + json.loads("2"))   // operators extend the chain
    print(check int(b))                // 42
    return none
}
```

The first Python exception raised anywhere in the chain aborts the rest of
the chain and becomes the chain's error value (section 13.4).

A py chain must be consumed by one of: a two-name destructure
(`v, err := chain`), a `check`, or a conversion (which absorbs the
fallibility; section 7.7). Binding a chain to a single name, or evaluating it
as a bare expression statement, is a compile-time error ("error result must
be handled"). Assigning into a py expression (`obj.attr = x` or
`obj[i] = x`) is a compile-time error in v1.

When a destructured chain fails, the value slot receives the zero value of
`py` (a Python `None` handle) and the error slot the error; on success the
error slot is `none`.

Comparisons on `py` operands yield `py` (Python semantics), not `bool`, so a
`py` expression cannot appear directly in a condition; extract with
`check bool(x)` first. `&&` and `||` reject `py` operands outright.

### 7.12 Evaluation order

Expressions evaluate left to right:

- binary operands: left then right (subject to short-circuit, 7.9.4);
- calls: callee (or method receiver), then arguments left to right;
- index and slice: the indexed expression, then the index or bounds;
- list literals: elements left to right; map literals: for each entry in
  order, key then value; struct literals: field values in the order written.

In an assignment statement the right-hand side is evaluated first; index
expressions inside the target are then evaluated from the outermost
(rightmost) index inward (section 8.3).

## 8. Statements

```
Statement = ShortVarDecl | Assignment | ExpressionStmt
          | ReturnStmt | BreakStmt | ContinueStmt
          | IfStmt | ForStmt .
```

### 8.1 Statement lists and terminators

```
StatementList = Statement { newline { newline } Statement } [ newline { newline } ] .
```

Inside a block, statements are separated by one or more line breaks. Every
statement except the last before the closing `}` must be followed by a line
break; two statements on one line are a syntax error. The line-continuation
positions of section 4.2 do not terminate a statement.

#### 8.1.1 Divergence and unreachable code

A statement diverges when control cannot flow past it: `return`, `break`,
`continue`, a `for` with no condition whose body contains no `break`
lexically at its own level, and an `if` statement with an `else` block in
which the `then` block, every `else if` block, and the `else` block all
diverge. An `if` without an `else` never diverges. A statement following a
diverging statement in the same block is a compile-time error
("unreachable code").

### 8.2 Short variable declarations

```
ShortVarDecl   = IdentifierList ":=" Expression .
IdentifierList = identifier { "," identifier } .
```

`x := e` declares `x` in the innermost scope with the type of `e` and binds a
copy of `e`'s value. `e` must produce a value; `x := f()` where `f` has no
results is a compile-time error.

With multiple names, `e` must be a multi-value (or py chain) of matching
arity; each name is declared with the corresponding component type. The
blank identifier discards its component. Redeclaring a name already declared
in the same scope is a compile-time error. There is no way to declare a
variable with an explicit type; the type is always the initializer's.

### 8.3 Assignments

```
Assignment = AssignTarget "=" Expression .
AssignTarget = identifier
             | PostfixExpr Index
             | PostfixExpr Selector .
```

The target must be a variable, an element access, or a field access rooted at
a variable, for example `x`, `xs[i]`, `u.name`, `ps[0].y`, `m["k"]`. Any
other expression as target is a syntax or compile-time error. The assigned
value must be assignable to the target's type. Assignment to a bare variable
invalidates any flow narrowing of that variable (chapter 9).

Element and field targets mutate in place through the path:

- `xs[i] = v` on a list requires `0 <= i < len(xs)`; out of range faults.
- `m[k] = v` on a map inserts or updates; the static type of the assigned
  value must be `V` (not `V?`). A map access may appear only as the final
  operation of a target path: because a map read has type `V?`, indexing or
  selecting through it (`m["a"][0] = v`) is a compile-time error; narrow the
  value out first.
- `s[i] = v` on a string type-checks but is a runtime fault
  ("cannot assign into a string"); strings are immutable.

The right-hand side is evaluated before the target path; the target's index
expressions are evaluated outermost first (section 7.12).

### 8.4 Expression statements

```
ExpressionStmt = Expression .
```

An expression may stand alone as a statement. Its value, if any, is
discarded, subject to the mandatory error handling rule: an expression
statement whose type is `error?`, ends in `error?`, or is a py chain is a
compile-time error ("error result must be handled"), except that a `check`
expression handles the error itself and may stand alone:

```
fn main() (error?) {
    check boom()      // ok: check consumes the error slot
    // boom()         // compile error
    return none
}
```

### 8.5 Return statements

```
ReturnStmt = "return" [ Expression { "," Expression } ] .
```

`return e1, ..., en` returns from the enclosing function (or function
literal). The number of expressions must equal the number of declared
results, and each expression must be assignable to the corresponding result
type. In a function with no declared results, `return` takes no expressions.
A bare `return` in a function with declared results is a compile-time error.

### 8.6 If statements

```
IfStmt    = "if" Condition Block { "else" "if" Condition Block } [ "else" Block ] .
Condition = Expression .
```

A `Condition` is an ordinary expression, parsed with struct literals
suppressed (section 7.2.3); the same production supplies the header
expressions of `for` statements (section 8.7).

Each condition must have type `bool`; any other type, including `py`, is a
compile-time error ("condition must be bool"). `else` and `else if` must
appear on the same line as the closing `}` of the preceding block. Struct
literals are suppressed in conditions (section 7.2.3). Conditions may narrow
option-typed variables inside the branches (chapter 9).

### 8.7 For statements

```
ForStmt = "for" Block
        | "for" IdentifierList "in" Condition Block
        | "for" Condition Block .
```

Rikki has one loop keyword with three forms:

- `for { ... }` loops forever; only `break` or `return` leaves it.
- `for cond { ... }` evaluates `cond` (a `bool`) before each iteration and
  stops when it is false.
- `for x in xs { ... }` iterates a `[]T`, binding `x: T` for each
  element in order. `for k, v in m { ... }` iterates a `map[K, V]` in
  insertion order, binding `k: K`, `v: V`. The name count must match the
  iterated type: one name for a list, two for a map; anything else, or
  iterating any other type (including `str` and `py`), is a compile-time
  error. The iteration variables are fresh copies each round; mutating them
  does not affect the container.

Struct literals are suppressed in the loop header (section 7.2.3).

### 8.8 Break and continue

```
BreakStmt    = "break" .
ContinueStmt = "continue" .
```

`break` terminates the innermost enclosing loop; `continue` begins its next
iteration. Either outside any loop is a compile-time error. Both diverge for
the purposes of section 8.1.1: statements after them in the same block are
unreachable.

## 9. Flow narrowing

Flow narrowing (flow typing) is the mechanism that unwraps option types. The
checker refines the type of a variable within regions where a condition
proves it is not `none`.

### 9.1 Narrowing conditions

Exactly two condition forms narrow, and only when the whole condition is that
form:

- `x != none` (or `none != x`), where `x` is a variable of option type
  `T?`: narrows `x` to `T` where the condition holds.
- `x == none` (or `none == x`): narrows `x` to `T` where the condition
  fails.

No other form narrows. In particular, a compound condition such as
`x != none && y != none` narrows neither variable, a narrowing comparison on
a field or element (`u.next != none`) narrows nothing (bind it to a variable
first), and a function call returning `bool` never narrows.

```
u := find()          // User?
if u != none {
    print(u.name)    // u: User here
}
```

### 9.2 Branch scope

For `if cond` with a narrowing condition:

- the positive narrowing applies within the `then` block;
- if there are no `else if` arms, the negative narrowing applies within the
  `else` block:

```
if x == none {
    // x: int? here
} else {
    print(x + 1)     // x: int here
}
```

Each `else if` arm applies its own condition's positive narrowing within its
own block. Narrowings do not combine across arms.

### 9.3 Terminal narrowing

For an `if` statement with a narrowing condition and no `else if` arms,
narrowing extends past the statement in exactly two cases:

- the `then` block diverges (section 8.1.1) and there is no `else`: the
  negative narrowing applies after the statement;
- there is an `else` block that diverges while the `then` block does not:
  the positive narrowing applies after the statement.

```
x := maybe()         // int?
if x == none {
    return
}
print(x + 1)         // x: int from here on
```

### 9.4 Invalidation

Narrowing is erased wherever it can no longer be proven:

- An assignment `x = e` anywhere, including in a nested block, erases every
  active narrowing of `x` from that point on (in all scopes; losing a
  narrowing is always sound):

```
x := maybe()
if x != none {
    if true {
        x = none
    }
    y := x + 1       // compile error: x is int? again
}
```

- A loop body runs more than once, so an assignment to `x` anywhere in a
  loop body erases the narrowing of `x` for the entire body, including
  statements before the assignment:

```
x := maybe()
if x != none {
    for i in range(2) {
        y := x + 1   // compile error: x assigned below in this body
        x = none
    }
}
```

Rebinding with `:=` creates a new variable and does not affect the outer
one's narrowing.

## 10. Errors and error handling

### 10.1 Error values

Errors are ordinary values of type `error` (section 5.7). There are no
exceptions and no user-visible panic. Fallible functions return their error
in a trailing `error?` result:

```
fn fetch(url str) (str, error?) { ... }
fn cleanup() (error?) { ... }
```

By convention, and as required by `check`, the error slot is the last result.

### 10.2 Mandatory handling

Dropping an error is a compile-time error. Specifically, the diagnostic
"error result must be handled" is issued when:

- an expression statement's value is `error?` or ends in `error?`, or is a
  py chain (section 8.4), unless the expression is a `check`;
- a short variable declaration binds one fewer name than the value count and
  the unbound trailing component is `error?` (`n := f()` where `f` returns
  `(int, error?)`);
- a py chain is bound to a single name or otherwise consumed as one value
  (section 7.11).

Handling means one of: binding the error slot to a name (which may be `_`,
an explicit and legal way to discard it), or propagating with `check`.

### 10.3 Recovery

Recovery is the two-value form plus a none-check, with flow narrowing
unwrapping the `error?`:

```
v, err := run()
if err != none {
    print("failed: " + err.msg)
    return
}
print(v)
```

### 10.4 Propagation and wrapping

`check` (section 7.8) propagates: on error, the enclosing function returns
the error in its final slot with all other slots zero-filled. `error.wrap`
adds context while preserving the cause chain:

```
fn parse_age(s str) (int, error?) {
    n, err := int(s)
    if err != none {
        return 0, error.wrap(err, "bad age")
    }
    return n, none
}
```

`main` may itself declare `(error?)`; returning a non-none error from `main`
terminates the program with a nonzero exit status (section 17.2).

## 11. Value semantics and equality

### 11.1 Value semantics

Assignment, argument passing, returning, capturing in a function literal,
placing in a container, and iteration binding all copy the value, deeply.
After `b := a`, mutating `b` never affects `a`, for every type except the two
documented reference types:

- `py` values are references to live Python objects (section 5.8); copying
  copies the reference.
- `Ctx` values (section 15.4) are opaque handles; copying copies the handle.

```
a := [1, 2, 3]
b := a
b[0] = 99
print(a[0])   // 1

fn mutate(xs []int) { xs[0] = 42 }
mutate(a)
print(a[0])   // 1
```

Builtin methods are value-semantic too: `xs.append(v)`, `m.delete(k)`, and
`xs.sorted()` return new containers and leave the receiver untouched.

### 11.2 Equality

The `==` operator is scalar-only (section 7.9.3). Structural equality is
provided by `list.contains`, which compares its argument against each element
recursively: lists element-wise, structs by name and field values,
maps by key set and per-key values, scalars and `none` by value. `py`, `fn`,
`Ctx`, and module values never compare equal to anything under structural
equality.

```
ps := [Point{x: 1, y: 2}]
print(ps.contains(Point{x: 1, y: 2}))   // true
```

## 12. Runtime faults

A fault is a runtime error that terminates the program. Faults are not
catchable in v1: no language construct observes or recovers one. A fault must
terminate the program with a nonzero exit status and a diagnostic including a
rikki call-stack trace, and must never crash the hosting process, raise a
foreign exception, or trigger undefined behavior. An interpreter panic is an
implementation bug, never specified program behavior.

The complete set of fault conditions reachable from checked programs:

- integer division or remainder by zero;
- integer overflow in `+`, `-`, `*`, `/`, `%`, or unary `-` on `int`
  (including `-9223372036854775808 / -1`);
- list or string index out of bounds;
- slice bounds out of range (`lo < 0`, `hi < lo`, or `hi > len`);
- assignment into a string index (`s[i] = v`);
- calling the zero value of a function type (section 5.11);
- exceeding the call-depth limit (chapter 18), diagnostic
  "recursion limit exceeded";
- `printf`/`sprintf` with a non-literal format string whose verbs do not
  match the arguments at runtime (wrong count, wrong type, unknown verb, or
  a format ending inside a verb; section 14.3);
- operations on list elements whose actual type does not match the list's
  static element type after an unchecked `[]T` conversion
  (section 7.7).

Float arithmetic never faults (section 5.1). Reading a map never faults.
Python exceptions are not faults; they become error values (chapter 13).

## 13. The Python bridge

### 13.1 import py

`import py "modname"` binds the named Python module as a value of type `py`.
Module resolution follows Python's own import rules in the embedded
interpreter; a dotted path imports the submodule. If the import fails at
program start (for example the module does not exist), the program terminates
with a runtime error before `main` runs, carrying the Python exception text.

Inside a project, py imports are validated against the manifest at
compile time (section 17.5).

### 13.2 Operations on py values

A `py` value supports:

- attribute selection `x.attr`, yielding `py`;
- calls `x(args...)` and method calls `x.m(args...)`, yielding `py`.
  Named arguments (`f(x, lr: 0.001)`) pass as Python keyword arguments;
- subscript `x[i]`, yielding `py`;
- the binary operators `+ - * / % == != < <= > >=` when either operand is
  `py`, dispatched to the corresponding Python operation and yielding `py`
  (comparisons included: the result is a Python bool as a `py` value, not a
  rikki `bool`).

`&& ||` reject `py` operands; unary `!` and `-` are not defined on `py`;
`py` values cannot be sliced, iterated with `for ... in`, used as conditions,
or assigned into (`x.attr = v`, `x[i] = v` are compile-time errors).

### 13.3 Fallibility

Every operation of section 13.2 may raise a Python exception. As specified in
section 7.11, a chain of such operations is fallible as a single unit typed
`(py, error?)` at consumption. The first exception in a chain converts to a
rikki error value and aborts the remainder of the chain.

### 13.4 Exception conversion

A Python exception becomes an error value with:

- `pytype`: the exception type's name (for example `"JSONDecodeError"`,
  `"ModuleNotFoundError"`);
- `msg`: `"<pytype>: <str(exception)>"`;
- `traceback`: the formatted Python traceback, or `""` when unavailable;
- `cause`: `none`.

### 13.5 Conversions across the bridge

Inbound (rikki value passed as an argument or index to a Python
operation): conversion is automatic, per this table, applied recursively to
list elements and map entries:

| Rikki | Python |
|----------|--------|
| `int` | `int` |
| `float` | `float` |
| `bool` | `bool` |
| `str` | `str` |
| `none` | `None` |
| `[]T` | `list` |
| `map[K, V]` | `dict` |
| `py` | the referenced object itself |

Passing any other type (a struct, function, error, Ctx, or module) is not a
conversion error at compile time but produces an error value at runtime
("cannot pass ... to python"), flowing through the chain's error slot like
any Python exception.

Outbound (Python object to rikki value): always explicit and fallible,
via the conversions of section 7.7 applied to a `py` operand. Each yields
`(T, error?)`:

| Conversion | Succeeds when | Notes |
|------------|---------------|-------|
| `int(x)` | the object is a Python integer (extractable as a 64-bit int) | a Python float is an error |
| `float(x)` | the object supports extraction as a Python float | Python ints convert |
| `bool(x)` | (practically) always | Python truthiness of the object |
| `str(x)` | (practically) always | Python `str()` of the object |
| `[]T(x)` | the object is iterable and every element extracts as `T` | `T` may be `int`, `float`, `bool`, `str`, or `py`; `py` keeps elements as handles |

Failure produces `(zero value of T, error)` with the Python exception
converted per section 13.4.

### 13.6 Rendering

`print`, `%v`, and `str()` render a `py` value as Python's `str()` of the
object.

## 14. Builtin functions

Builtins are available without import. They are resolved only when the name
is not bound by a variable in scope or a declared function; such a binding
shadows the builtin entirely (a shadowed builtin is not callable through any
other path).

### 14.1 print

```
print(v1, ..., vn)
```

Takes zero or more arguments of any single-value types, renders each
canonically, joins them with single spaces, and writes the result followed by
a line feed to standard output. Canonical rendering (shared by `print`, the
`%v` verb, and `str()`):

| Type | Rendering |
|------|-----------|
| `int` | decimal |
| `float` | shortest decimal that round-trips; integral values render without a fractional part (`3.0` renders as `3`); infinities as `inf`/`-inf`, NaN as `NaN` |
| `bool` | `true` / `false` |
| `str` | the string itself, unquoted |
| `list` | `[e1, e2, ...]`, elements rendered recursively |
| `map` | `{k1: v1, k2: v2}` in insertion order |
| struct | `Name{f1: v1, f2: v2}` in declaration order |
| `none` | `none` |
| `error` | `error(<msg>)` |
| `py` | Python `str()` of the object |
| `fn` | `fn` |

### 14.2 printf and sprintf

```
printf(format, a1, ..., an)          // writes to standard output, no implicit newline
sprintf(format, a1, ..., an) str     // returns the formatted string
```

The format string uses Go-style verbs:

| Verb | Argument type | Output |
|------|---------------|--------|
| `%v` | any | canonical rendering (section 14.1) |
| `%d` | `int` | decimal |
| `%s` | `str` | the string |
| `%t` | `bool` | `true` / `false` |
| `%q` | `str` | double-quoted, backslash-escaped |
| `%f` | `float` | fixed-point, default 6 fractional digits |
| `%%` | none | literal `%` |

A verb may carry a minimum width (`%5s`) and a precision (`%.2f`), both
decimal digit sequences, in the form `%[width][.precision]verb`. Width pads
on the left with spaces to the given count of code points (not bytes).
Precision is honored by `%f`; on other verbs it is accepted and ignored.
There is no left-align or zero-pad flag.

### 14.3 Static and dynamic format checking

When the format argument is a string literal, the format is checked at
compile time: verb count must equal argument count, each argument's type must
match its verb, verbs must be from the table, and the format must not end
inside a verb. Violations are compile-time errors.

When the format is not a literal, the same checks happen at runtime and a
violation is a fault (chapter 12).

### 14.4 len

```
len(x) int
```

For `str`, the number of Unicode code points; for `list`, the element count;
for `map`, the entry count. Any other argument type is a compile-time error.

### 14.5 range

```
range(n) []int        // 0, 1, ..., n-1
range(a, b) []int     // a, a+1, ..., b-1
```

Returns a freshly built list. If `b <= a` (or `n <= 0`), the result is the
empty list; `range` never faults.

### 14.6 args

```
args() []str
```

Returns the program's arguments: everything after the source file on the
command line (`tk prog.rk a b` and `rikki run prog.rk a b` both yield
`["a", "b"]`). Takes no arguments. In contexts with no command line (tests,
embedding) the list is empty.

### 14.7 input

```
input(prompt str) (str, error?)
```

Writes `prompt` to standard output (no trailing newline, flushed), then reads
one line from standard input. The returned string excludes the line
terminator. End of input and read failures are error values, not faults
(`eof` on end of input). When a program runs through the CLI runner its
output is streamed unbuffered, so a prompt is visible before input blocks.

### 14.8 Methods on builtin types

All receivers are unchanged; results are new values.

String methods (receiver `str`):

| Method | Signature | Behavior |
|--------|-----------|----------|
| `split` | `(sep str) []str` | split on separator |
| `trim` | `() str` | strip leading and trailing white space |
| `upper` | `() str` | uppercase |
| `lower` | `() str` | lowercase |
| `contains` | `(sub str) bool` | substring test |
| `starts_with` | `(prefix str) bool` | prefix test |
| `ends_with` | `(suffix str) bool` | suffix test |
| `replace` | `(from str, to str) str` | replace all occurrences |

List methods (receiver `[]T`):

| Method | Signature | Behavior |
|--------|-----------|----------|
| `map` | `(f fn(T) U) []U` | apply `f` to each element |
| `filter` | `(f fn(T) bool) []T` | keep elements where `f` is true |
| `each` | `(f fn(T))` | call `f` on each element; no result |
| `sum` | `() T` | `T` must be `int` or `float`; sum of elements. Summing an empty `[]int` yields 0; the result of summing an empty `[]float` is unspecified in v1 (the reference implementation yields a value that faults on later float use) |
| `sorted` | `() []T` | `T` must be `int`, `float`, or `str`; ascending copy |
| `sorted_by` | `(before fn(T, T) bool) []T` | sorted copy per comparator; stable |
| `append` | `(v T) []T` | copy with `v` appended |
| `contains` | `(v T) bool` | structural membership (section 11.2) |
| `join` | `(sep str) str` | `T` must be `str`; concatenation with separator |

Map methods (receiver `map[K, V]`):

| Method | Signature | Behavior |
|--------|-----------|----------|
| `keys` | `() []K` | keys in insertion order |
| `values` | `() []V` | values in insertion order |
| `has` | `(k K) bool` | key presence |
| `delete` | `(k K) map[K, V]` | copy without `k`; remaining order preserved |

## 15. Standard library

Standard library modules are imported by bare name: `import "math"`. The
module name then acts as a namespace: `math.sqrt(2.0)`, `math.pi`. The v1
modules are `math`, `error`, `file`, `ctx`, and `http`.

Where a stdlib signature below ends in `error?` or `(T, error?)`, failures
are ordinary error values subject to chapter 10; stdlib functions do not
fault on I/O failure.

### 15.1 error

The constructors `error.new` and `error.wrap` require no import; they are
part of the core language. `import "error"` remains legal and adds nothing.

| Function | Signature | Behavior |
|----------|-----------|----------|
| `error.new` | `(msg str) error` | new error with the message; empty `pytype`, `traceback`, no cause |
| `error.wrap` | `(cause error, msg str) error` | new error with the message and the given cause |

Error fields are specified in section 5.7.

### 15.2 math

| Member | Signature | Behavior |
|--------|-----------|----------|
| `abs` | `(int) int` or `(float) float` | absolute value (polymorphic over the two numeric types) |
| `min`, `max` | `(int, int) int` or `(float, float) float` | both arguments the same numeric type |
| `sqrt` | `(float) float` | square root |
| `pow` | `(float, float) float` | exponentiation |
| `floor` | `(float) int` | round down |
| `ceil` | `(float) int` | round up |
| `round` | `(float) int` | round half away from zero (not banker's rounding) |
| `pi`, `e` | `float` constants | |

### 15.3 file

Paths are `str`. Contents are UTF-8 `str`; there is no bytes type in v1.

| Function | Signature | Behavior |
|----------|-----------|----------|
| `file.read` | `(path str) (str, error?)` | whole-file read; zero slot is `""` on error |
| `file.write` | `(path str, s str) error?` | create or truncate, then write |
| `file.append` | `(path str, s str) error?` | create if missing, append |
| `file.exists` | `(path str) bool` | existence test; never errors |
| `file.list` | `(dir str) ([]str, error?)` | entry names, sorted lexicographically |
| `file.remove` | `(path str) error?` | remove a file or an empty directory |
| `file.mkdir` | `(path str) error?` | create the directory and any missing parents |

### 15.4 ctx

Importing `"ctx"` also brings the opaque struct type `Ctx` into scope. A
`Ctx` is a cancellation handle: a deadline plus an interrupt flag. `Ctx`
values are handles with reference semantics (section 11.1) and cannot be
constructed with a struct literal (section 7.2.3).

| Function | Signature | Behavior |
|----------|-----------|----------|
| `ctx.background` | `() Ctx` | never done |
| `ctx.timeout` | `(parent Ctx, secs float) Ctx` | deadline `secs` from now, clamped so a child deadline never exceeds its parent's |
| `ctx.interrupt` | `(parent Ctx) Ctx` | additionally becomes done when the process receives SIGINT |

Methods on `Ctx`:

| Method | Signature | Behavior |
|--------|-----------|----------|
| `done` | `() bool` | whether the deadline has passed or the interrupt fired |
| `err` | `() error?` | `none` while live; `"deadline exceeded"` or `"interrupted"` when done |

### 15.5 http

Importing `"http"` also declares two struct types:

```
struct Request  { method str, url str, body str, headers map[str, str] }
struct Response { status int, body str, headers map[str, str] }
```

| Function | Signature |
|----------|-----------|
| `http.get` | `(c Ctx, url str) (Response, error?)` |
| `http.post` | `(c Ctx, url str, body str) (Response, error?)` |
| `http.request` | `(c Ctx, req Request) (Response, error?)` |
| `http.stream` | `(c Ctx, url str, body str, f fn(str)) (Response, error?)` |

Behavior:

- If the ctx is already done, the call returns an error before any network
  I/O.
- A live ctx deadline bounds the whole request; without a deadline, an
  implementation-defined default timeout applies (30 seconds in the
  reference implementation).
- A completed HTTP exchange is a success regardless of status code: a 404 is
  a `Response` with `status` 404 and a `none` error. Only transport-level
  failures (connection refused, timeout, invalid request) are error values,
  with the zero `Response` in the value slot.
- Redirects are followed automatically.
- For `http.request`, an empty body on a GET request sends no body.
- `http.stream` POSTs `body` and invokes `f` once per response line as lines
  arrive, before the response completes (server-sent events are consumed this
  way). The returned `Response.body` holds the accumulated lines, newline
  terminated, so the program can reparse the full payload afterward; closures
  capture by value and therefore cannot accumulate it themselves. Its default
  deadline, absent a ctx deadline, is 300 seconds rather than 30.
- Response header names are as received; values that are not valid strings
  read as `""`.

## 16. Modules and multi-file programs

### 16.1 File imports

`import "util.rk"` imports another rikki source file. The path is
resolved relative to the directory of the importing file. The imported
file's top-level functions and structs become visible under a namespace
equal to the file's stem (the file name without `.rk`):

```
// util.rk
struct Pair { a int, b int }
fn double(x int) int { return x * 2 }
fn make(a int, b int) Pair { return Pair{a: a, b: b} }

// main.rk
import "util.rk"

fn sum(p util.Pair) int { return p.a + p.b }

fn main() {
    print(util.double(21))       // 42
    p := util.make(1, 2)
    print(sum(p))                // 3
}
```

Struct types of a file module are named in type positions with the dotted
form `util.Pair` (section 5.9). A struct literal names a single identifier
(section 7.2.3); there is no dotted literal form `util.Pair{...}`, so values
of a module's struct types are constructed by functions of that module.

### 16.2 Semantics

- Imports are transitive: an imported file may import further files, each
  resolved relative to its own directory.
- A file imported through multiple paths is loaded once (diamond imports are
  fine).
- An import cycle is a compile-time error naming the cycle.
- An unreadable or missing import path is a compile-time error.
- The root file (the one being run) is not namespaced. Namespacing respects
  local shadowing inside the imported module: a local variable that shadows
  a module-level name refers to the local.
- Modules are namespaces only; they are not first-class values.

## 17. Program execution

### 17.1 Entry point

Program execution begins at `fn main`. `main` must be declared, must take no
parameters, and must declare either no results or the single result
`(error?)`. Any other signature, or a missing `main`, is a compile-time
error.

Before `main` runs, all `import py` modules are imported; a failing Python
import terminates the program as a runtime error.

Program arguments follow the file on the runner command line and are exposed
through the `args()` builtin (section 14.6).

### 17.2 Termination and exit status

A program run terminates in one of four ways:

| Outcome | Exit status | Diagnostics |
|---------|-------------|-------------|
| `main` returns (no error) | 0 | none |
| compile error (lex, parse, or typecheck) | nonzero | each diagnostic as `line:col: message` on standard error; the program does not run at all |
| `main` returns a non-none error | nonzero | the error's `msg` on standard error |
| runtime fault (chapter 12) | nonzero | the fault message and a rikki stack trace on standard error |

Program output written by `print`/`printf` up to the point of termination is
delivered to standard output in all cases. A program that typechecks must
never terminate by crashing the host process.

### 17.3 The two binaries

- `rikki` is the toolchain: `rikki run [file]` typechecks and runs
  (defaulting to the enclosing project's `src/main.rk`); `rikki check
  [file]` typechecks only and never runs code or provisions an environment;
  `rikki new <name>` scaffolds a project; `rikki py add <pkg>` declares
  a Python dependency and syncs the environment; `rikki repl` starts the
  REPL.
- `tk` is the runner: `tk file.rk` typechecks and runs the file; bare `tk`
  starts the REPL.

Bare `rikki run` and `rikki check` outside any project fail with a
diagnostic. `rikki check` on a valid program produces no output and exits
0.

### 17.4 Projects

A project is a directory tree rooted at a `rikki.toml` manifest, found by
walking upward from the file being operated on (or the working directory).
The layout:

- `rikki.toml`: project name, Python version pin, and declared Python
  dependencies (`[py-deps]`).
- `rikki.lock`: exact resolved Python package versions. Manifest and lock
  together fully determine the Python environment.
- `.rikki/`: the generated virtual environment and sync markers.
  Disposable; deleting it is always safe, it regenerates on the next run.
- `src/main.rk`: the default entry point for bare `rikki run`.

### 17.5 The manifest rule for py imports

When the compiled file lies inside a project, every `import py "m"` is
validated at compile time: the top-level segment of `m` (the part before the
first `.`) must either be declared under `[py-deps]` in `rikki.toml` or be
a module of the Python standard library. Declared names match import names
case-insensitively with `-` and `_` interchangeable, mirroring PyPI name
normalization (`sentence-transformers` satisfies
`import py "sentence_transformers"`). An undeclared import is a compile-time
error directing the user to `rikki py add`.

The manifest's `python` pin must match the interpreter embedded in the
running rikki (major.minor); a mismatch is a compile-time error naming
both versions. When the pin is omitted it defaults to the embedded version.
`rikki new` scaffolds with the embedded version.

Inside a project, `sys.executable` in the embedded interpreter refers to the
project venv's python, so Python libraries that spawn worker interpreters
(multiprocessing and similar) function normally.

Outside a project there is no manifest to check; py imports resolve at
program start against the embedded interpreter, and a missing module is a
runtime error (section 13.1). Running a project with declared Python
dependencies provisions the environment automatically before execution;
`rikki check` never provisions.

### 17.6 The REPL

Bare `tk` (or `rikki repl`) starts an interactive session. The v1 REPL is
unchecked: input goes to the evaluator without typechecking, and faults are
reported and survived rather than ending the session. A line whose first
word is `fn`, `struct`, or `import` is treated as a declaration and
registered; any other line is executed as a statement, and an expression
statement's value, if any, is printed in canonical rendering. Bindings and
declarations persist for the session. REPL behavior beyond this paragraph is
unspecified in v1.

## 18. Implementation limits

Limits a program may rely on, stated as minimum guarantees:

- Syntactic nesting: an implementation must accept at least 256 levels of
  combined expression, type, and block nesting. Input exceeding the
  implementation's limit must be rejected with a compile-time diagnostic
  ("expression too deeply nested"), never a crash. The reference
  implementation's limit is exactly 256.
- Call depth: an implementation must support at least 1000 simultaneously
  active rikki function calls. Exceeding the implementation's limit is a
  runtime fault ("recursion limit exceeded") carrying a (possibly truncated)
  stack trace, never a host stack overflow. The reference implementation's
  limit is exactly 1000.
- `int` is exactly 64-bit two's complement (section 5.1); this is not
  implementation-defined.

## 19. Conformance and maintenance

The golden tests under `tests/golden/` are the executable companion to this
specification: each `.rk` file paired with an `.out` (expected stdout of a
successful run) or `.err` (required substrings of the compile or runtime
diagnostic) fixes observable behavior. A conforming implementation must pass
them. Where this document and a golden test disagree, the golden test is
taken as the intended behavior and this document must be corrected.

Any change to language semantics must land together with a matching edit to
this document, in the same commit, alongside the golden tests that prove the
new behavior.
