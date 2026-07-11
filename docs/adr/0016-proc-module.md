# 16. proc: subprocesses with runtime-pumped pipes

Status: accepted 2026-07-10

## Context

Subprocesses are the second half of the scripting story (ADR 0015) and
the first stdlib surface where concurrency is unavoidable: a child
process is concurrent with the program by nature, and the classic
os/exec deadlock (the child fills the stderr pipe while the parent
reads stdout) cannot be fixed by API shape alone. nevla has no
user-level concurrency and is not getting it as a side effect of proc;
`Value` is Rc/RefCell (nothing is Send), the GIL is a global choke
point for parallel py work, and the copy model has no story for
reference types crossing threads. Those questions get their own future
ADR; the ledger at docs/proposals/concurrency.md collects what we learn
until then.

## Decision

Go os/exec semantics, with the concurrency inside the runtime.

Types: `struct Cmd { argv []str, dir str, env map[str]str, stdin str,
log str }`, `struct Result { status int, stdout str, stderr str }`,
and an opaque handle `Proc` with reference semantics, like Ctx.

- `proc.run(c Ctx, argv []str) (Result, error?)` shorthand and
  `proc.exec(c Ctx, cmd Cmd) (Result, error?)` full form, both running
  to completion with output captured (stdout and stderr separate).
- Exit semantics follow Go, a deliberate deviation from ADR 0012: a
  nonzero exit sets the error ("exit status 3") AND fills Result, so
  handling is mandatory but the output is still data. Pure data is
  reserved for status 0. Only failure to run at all (missing binary,
  ctx already done, spawn failure) returns a zero Result.
- `proc.start(cmd Cmd) (Proc, error?)` for long-running children.
  stderr merges into stdout, one ordered stream; `cmd.log` routes that
  stream to a file (append) instead, for the watcher case where nobody
  reads it in-language. Handle methods: `pid() int`, `running() bool`,
  `readline(c Ctx) (str, error?)` (eof is an error value, the same
  contract as `input()`), `wait(c Ctx) (int, error?)`,
  `stop(grace float) error?` (terminate, wait grace seconds, kill).
- The runtime owns the pipes: background stdlib threads pump child
  output into buffers (or the log file) the moment it exists. No
  Value crosses a thread; the interpreter stays single-threaded. The
  deadlock class dies in the runtime where it belongs.

## Consequences

- Killed on sight: the builtins.open hack in examples/scripts
  (a py file handle existed only to give Popen somewhere to write).
- The handle-plus-blocking-reads shape is deliberately wrappable by a
  future concurrency design: `readline` can become a channel receive
  without breaking anyone. proc must never grow API that a channel
  world could not absorb.
- Every blocking method takes a Ctx (the 0015 pattern), so SIGINT and
  deadlines compose with child processes for free.
- Runtime threads now exist inside the stdlib. The invariant that no
  Value crosses them is load-bearing and goes in the concurrency
  ledger as constraint one.
