//! Subprocesses with runtime-pumped pipes (spec 15.12, ADR 0016). The
//! concurrency lives here, not in the language: pump threads move child
//! output into buffers (or a log file) the moment it exists, and only
//! Strings ever cross a thread.
//!
//! Three waiting shapes, only one of them a poll:
//! - lines are PUSHED: readline parks on a condvar the pump signals;
//! - process exit is polled (try_wait per 20ms slice): a blocking wait
//!   costs a thread per child, SIGCHLD fights ctrlc for the process's
//!   one signal handler, and pidfd/kqueue is per-platform code;
//! - cancellation is polled everywhere, because a Ctx (deadline +
//!   SIGINT flag) has no event source to park on. The 20ms slice is
//!   the bound on noticing Ctrl-C, not on data latency. A waitable
//!   Ctx would delete the slicing; see docs/proposals/concurrency.md.

const CMD_FIELDS: &[(&str, FieldKind)] = &[
    ("argv", FieldKind::ListStr),
    ("dir", FieldKind::Str),
    ("env", FieldKind::MapStrStr),
    ("stdin", FieldKind::Str),
    ("log", FieldKind::Str),
];
const RESULT_FIELDS: &[(&str, FieldKind)] = &[
    ("status", FieldKind::Int),
    ("stdout", FieldKind::Str),
    ("stderr", FieldKind::Str),
];

enum FieldKind {
    Str,
    Int,
    ListStr,
    MapStrStr,
}

pub(crate) fn struct_types() -> Vec<(String, Vec<(String, crate::types::Type)>)> {
    use crate::types::Type;
    let ty = |k: &FieldKind| match k {
        FieldKind::Str => Type::Str,
        FieldKind::Int => Type::Int,
        FieldKind::ListStr => Type::List(Box::new(Type::Str)),
        FieldKind::MapStrStr => Type::Map(Box::new(Type::Str), Box::new(Type::Str)),
    };
    let fields = |fs: &[(&str, FieldKind)]| fs.iter().map(|(f, k)| (f.to_string(), ty(k))).collect();
    vec![
        ("Proc".into(), vec![]),
        ("Cmd".into(), fields(CMD_FIELDS)),
        ("Result".into(), fields(RESULT_FIELDS)),
    ]
}

pub(crate) fn struct_exprs() -> Vec<(String, Vec<(String, crate::ast::TypeExpr)>)> {
    use crate::ast::TypeExpr;
    let named = |n: &str| TypeExpr::Named(n.into());
    let ty = |k: &FieldKind| match k {
        FieldKind::Str => named("str"),
        FieldKind::Int => named("int"),
        FieldKind::ListStr => TypeExpr::List(Box::new(named("str"))),
        FieldKind::MapStrStr => TypeExpr::Map(Box::new(named("str")), Box::new(named("str"))),
    };
    let fields = |fs: &[(&str, FieldKind)]| fs.iter().map(|(f, k)| (f.to_string(), ty(k))).collect();
    vec![
        ("Cmd".into(), fields(CMD_FIELDS)),
        ("Result".into(), fields(RESULT_FIELDS)),
    ]
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::ProcInner;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::collections::VecDeque;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::process::{Child, Command, Stdio};
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    use indexmap::IndexMap;

    use crate::interp::{Fault, Interp};
    use crate::stdlib::ctx::CtxInner;
    use crate::value::{ErrVal, MapKey, Value};

    const SLICE: Duration = Duration::from_millis(20);

    pub struct Stream {
        pub lines: VecDeque<String>,
        pub open_pumps: u8,
        /// set when the stream routes to a log file instead of the queue
        pub logged_to: Option<String>,
    }

    impl std::fmt::Debug for ProcInner {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "Proc(pid {})", self.pid)
        }
    }

    pub struct ProcInner {
        pub pid: i64,
        child: Mutex<Child>,
        /// cached exit status once reaped; try_wait errors after reaping
        status: Mutex<Option<i64>>,
        stream: Mutex<Stream>,
        wake: Condvar,
    }

    pub struct Spec {
        pub argv: Vec<String>,
        pub dir: String,
        pub env: Vec<(String, String)>,
        pub stdin: String,
        pub log: String,
    }

    fn err(msg: impl Into<String>) -> ErrVal {
        ErrVal {
            msg: msg.into(),
            ..Default::default()
        }
    }

    fn zero_result() -> Value {
        result_value(0, String::new(), String::new())
    }

    fn result_value(status: i64, stdout: String, stderr: String) -> Value {
        let mut fields = IndexMap::new();
        fields.insert("status".to_string(), Value::Int(status));
        fields.insert("stdout".to_string(), Value::Str(stdout));
        fields.insert("stderr".to_string(), Value::Str(stderr));
        Value::Struct {
            name: "Result".into(),
            fields,
        }
    }

    fn fallible(v: Value, e: Option<ErrVal>) -> Value {
        Value::Tuple(vec![
            v,
            match e {
                Some(e) => Value::Err(e),
                None => Value::NoneV,
            },
        ])
    }

    fn command(spec: &Spec) -> Result<Command, ErrVal> {
        let Some(bin) = spec.argv.first() else {
            return Err(err("proc: empty argv"));
        };
        let mut c = Command::new(bin);
        c.args(&spec.argv[1..]);
        if !spec.dir.is_empty() {
            c.current_dir(&spec.dir);
        }
        // env MERGES into the inherited environment (design doc): losing
        // PATH because you set one variable is the footgun, not this
        for (k, v) in &spec.env {
            c.env(k, v);
        }
        c.stdin(if spec.stdin.is_empty() {
            Stdio::null()
        } else {
            Stdio::piped()
        });
        Ok(c)
    }

    fn feed_stdin(child: &mut Child, stdin: &str) {
        if stdin.is_empty() {
            return;
        }
        if let Some(mut w) = child.stdin.take() {
            let body = stdin.to_string();
            // a writer thread so a child that never reads cannot wedge us
            std::thread::spawn(move || {
                let _ = w.write_all(body.as_bytes());
            });
        }
    }

    fn status_code(st: std::process::ExitStatus) -> (i64, Option<String>) {
        match st.code() {
            Some(0) => (0, None),
            Some(n) => (i64::from(n), Some(format!("exit status {n}"))),
            None => {
                #[cfg(unix)]
                let name = {
                    use std::os::unix::process::ExitStatusExt;
                    st.signal()
                        .map(|s| format!("terminated by signal {s}"))
                        .unwrap_or_else(|| "terminated by signal".into())
                };
                #[cfg(not(unix))]
                let name = "terminated".to_string();
                (-1, Some(name))
            }
        }
    }

    #[cfg(unix)]
    fn send_term(pid: i64) {
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
    }
    #[cfg(not(unix))]
    fn send_term(_pid: i64) {}

    /// terminate, wait up to grace, kill, reap. Best effort throughout;
    /// stopping the already-dead is fine.
    fn stop_child(child: &mut Child, pid: i64, grace: Duration) -> i64 {
        if let Ok(Some(st)) = child.try_wait() {
            return status_code(st).0;
        }
        send_term(pid);
        let deadline = Instant::now() + grace;
        loop {
            if let Ok(Some(st)) = child.try_wait() {
                return status_code(st).0;
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(SLICE);
        }
        let _ = child.kill();
        match child.wait() {
            Ok(st) => status_code(st).0,
            Err(_) => -1,
        }
    }

    /// run/exec: spawn, capture stdout and stderr separately on reader
    /// threads (the classic two-pipe deadlock dies here), slice-poll the
    /// child against the ctx, kill on ctx end.
    pub fn run(c: &CtxInner, spec: &Spec) -> Value {
        if let Some(e) = c.err() {
            return fallible(zero_result(), Some(e));
        }
        let mut cmd = match command(spec) {
            Ok(c) => c,
            Err(e) => return fallible(zero_result(), Some(e)),
        };
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                return fallible(
                    zero_result(),
                    Some(err(format!("proc: {}: {e}", spec.argv[0]))),
                )
            }
        };
        feed_stdin(&mut child, &spec.stdin);
        let slurp = |r: Option<Box<dyn Read + Send>>| {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                if let Some(mut r) = r {
                    let _ = r.read_to_end(&mut buf);
                }
                String::from_utf8_lossy(&buf).into_owned()
            })
        };
        let out = slurp(child.stdout.take().map(|s| Box::new(s) as _));
        let errs = slurp(child.stderr.take().map(|s| Box::new(s) as _));
        let pid = i64::from(child.id());

        let exit = loop {
            match child.try_wait() {
                Ok(Some(st)) => break Ok(st),
                Ok(None) => {}
                Err(e) => break Err(err(format!("proc: wait: {e}"))),
            }
            if let Some(ctx_err) = c.err() {
                stop_child(&mut child, pid, Duration::from_secs(2));
                break Err(ctx_err);
            }
            std::thread::sleep(SLICE);
        };
        let stdout = out.join().unwrap_or_default();
        let stderr = errs.join().unwrap_or_default();
        match exit {
            Ok(st) => {
                let (code, e) = status_code(st);
                fallible(result_value(code, stdout, stderr), e.map(err))
            }
            Err(e) => fallible(result_value(-1, stdout, stderr), Some(e)),
        }
    }

    /// attach: the child owns the terminal (stdin, stdout, stderr all
    /// inherited); block until it exits, ctx-bounded like run. For
    /// editors, REPLs, and anything else interactive.
    pub fn attach(c: &CtxInner, argv: &[String]) -> Value {
        if let Some(e) = c.err() {
            return fallible(Value::Int(0), Some(e));
        }
        let spec = Spec {
            argv: argv.to_vec(),
            dir: String::new(),
            env: vec![],
            stdin: String::new(),
            log: String::new(),
        };
        let mut cmd = match command(&spec) {
            Ok(c) => c,
            Err(e) => return fallible(Value::Int(0), Some(e)),
        };
        cmd.stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                return fallible(Value::Int(0), Some(err(format!("proc: {}: {e}", spec.argv[0]))))
            }
        };
        let pid = i64::from(child.id());
        loop {
            match child.try_wait() {
                Ok(Some(st)) => {
                    let (code, e) = status_code(st);
                    return fallible(Value::Int(code), e.map(err));
                }
                Ok(None) => {}
                Err(e) => return fallible(Value::Int(-1), Some(err(format!("proc: wait: {e}")))),
            }
            if let Some(ctx_err) = c.err() {
                stop_child(&mut child, pid, Duration::from_secs(2));
                return fallible(Value::Int(-1), Some(ctx_err));
            }
            std::thread::sleep(SLICE);
        }
    }

    /// start: merged stdout+stderr, one stream, line-granular interleave;
    /// cmd.log routes the stream to a file instead.
    pub fn start(spec: &Spec) -> Value {
        let none_proc = || Value::Int(0); // placeholder never reaching users
        let mut cmd = match command(spec) {
            Ok(c) => c,
            Err(e) => return Value::Tuple(vec![none_proc(), Value::Err(e)]),
        };
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                return Value::Tuple(vec![
                    none_proc(),
                    Value::Err(err(format!("proc: {}: {e}", spec.argv[0]))),
                ])
            }
        };
        feed_stdin(&mut child, &spec.stdin);
        let pid = i64::from(child.id());
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let inner = Arc::new(ProcInner {
            pid,
            child: Mutex::new(child),
            status: Mutex::new(None),
            stream: Mutex::new(Stream {
                lines: VecDeque::new(),
                open_pumps: 2,
                logged_to: if spec.log.is_empty() {
                    None
                } else {
                    Some(spec.log.clone())
                },
            }),
            wake: Condvar::new(),
        });
        for reader in [
            stdout.map(|s| Box::new(s) as Box<dyn Read + Send>),
            stderr.map(|s| Box::new(s) as Box<dyn Read + Send>),
        ] {
            let inner = Arc::clone(&inner);
            let log = spec.log.clone();
            std::thread::spawn(move || {
                if let Some(r) = reader {
                    for line in BufReader::new(r).lines() {
                        let Ok(line) = line else { break };
                        if log.is_empty() {
                            let mut s = inner.stream.lock().unwrap();
                            s.lines.push_back(line);
                            inner.wake.notify_all();
                        } else {
                            use std::fs::OpenOptions;
                            let _ = OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(&log)
                                .and_then(|mut f| writeln!(f, "{line}"));
                        }
                    }
                }
                let mut s = inner.stream.lock().unwrap();
                s.open_pumps -= 1;
                inner.wake.notify_all();
            });
        }
        Value::Tuple(vec![Value::Proc(inner), Value::NoneV])
    }

    impl ProcInner {
        fn cached_or_try_wait(&self) -> Option<i64> {
            if let Some(s) = *self.status.lock().unwrap() {
                return Some(s);
            }
            let mut child = self.child.lock().unwrap();
            if let Ok(Some(st)) = child.try_wait() {
                let code = super::native::status_code(st).0;
                *self.status.lock().unwrap() = Some(code);
                return Some(code);
            }
            None
        }

        pub fn running(&self) -> bool {
            self.cached_or_try_wait().is_none()
        }

        pub fn readline(&self, c: &CtxInner) -> Value {
            loop {
                {
                    let mut s = self.stream.lock().unwrap();
                    if let Some(to) = &s.logged_to {
                        return fallible(
                            Value::Str(String::new()),
                            Some(err(format!("stream is routed to {to}"))),
                        );
                    }
                    if let Some(line) = s.lines.pop_front() {
                        return fallible(Value::Str(line), None);
                    }
                    if s.open_pumps == 0 {
                        return fallible(Value::Str(String::new()), Some(err("eof")));
                    }
                    if c.err().is_none() {
                        let (guard, _) = self.wake.wait_timeout(s, SLICE).unwrap();
                        drop(guard);
                    }
                }
                if let Some(e) = c.err() {
                    return fallible(Value::Str(String::new()), Some(e));
                }
            }
        }

        pub fn wait(&self, c: &CtxInner) -> Value {
            loop {
                if let Some(code) = self.cached_or_try_wait() {
                    return fallible(Value::Int(code), None);
                }
                if let Some(e) = c.err() {
                    return fallible(Value::Int(0), Some(e));
                }
                std::thread::sleep(SLICE);
            }
        }

        pub fn stop(&self, grace: i64) -> Value {
            if self.status.lock().unwrap().is_some() {
                return Value::NoneV;
            }
            let mut child = self.child.lock().unwrap();
            let code = stop_child(
                &mut child,
                self.pid,
                Duration::from_nanos(grace.max(0) as u64),
            );
            *self.status.lock().unwrap() = Some(code);
            Value::NoneV
        }
    }

    pub fn spec_from(v: &Value) -> Result<Spec, String> {
        let Value::Struct { fields, .. } = v else {
            return Err("proc: expected a Cmd".into());
        };
        let str_of = |f: &str| match fields.get(f) {
            Some(Value::Str(s)) => Ok(s.clone()),
            _ => Err(format!("proc: Cmd.{f} must be str")),
        };
        let argv = match fields.get("argv") {
            Some(Value::List(items)) => items
                .borrow()
                .iter()
                .map(|v| match v {
                    Value::Str(s) => Ok(s.clone()),
                    _ => Err("proc: Cmd.argv must be []str".to_string()),
                })
                .collect::<Result<Vec<_>, _>>()?,
            _ => return Err("proc: Cmd.argv must be []str".into()),
        };
        let env = match fields.get("env") {
            Some(Value::Map(m)) => m
                .borrow()
                .iter()
                .map(|(k, v)| match (k, v) {
                    (MapKey::Str(k), Value::Str(v)) => Ok((k.clone(), v.clone())),
                    _ => Err("proc: Cmd.env must be map[str]str".to_string()),
                })
                .collect::<Result<Vec<_>, _>>()?,
            _ => return Err("proc: Cmd.env must be map[str]str".into()),
        };
        Ok(Spec {
            argv,
            dir: str_of("dir")?,
            env,
            stdin: str_of("stdin")?,
            log: str_of("log")?,
        })
    }

    pub fn call(interp: &mut Interp, name: &str, args: Vec<Value>) -> Result<Value, Fault> {
        let v = match (name, args.as_slice()) {
            ("run", [Value::Ctx(c), Value::List(argv)]) => {
                let argv: Vec<String> = argv
                    .borrow()
                    .iter()
                    .filter_map(|v| match v {
                        Value::Str(s) => Some(s.clone()),
                        _ => None,
                    })
                    .collect();
                run(
                    c,
                    &Spec {
                        argv,
                        dir: String::new(),
                        env: vec![],
                        stdin: String::new(),
                        log: String::new(),
                    },
                )
            }
            ("attach", [Value::Ctx(c), Value::List(argv)]) => {
                let argv: Vec<String> = argv
                    .borrow()
                    .iter()
                    .filter_map(|v| match v {
                        Value::Str(s) => Some(s.clone()),
                        _ => None,
                    })
                    .collect();
                attach(c, &argv)
            }
            ("exec", [Value::Ctx(c), cmd @ Value::Struct { .. }]) => match spec_from(cmd) {
                Ok(spec) => run(c, &spec),
                Err(m) => fallible(zero_result(), Some(err(m))),
            },
            ("start", [cmd @ Value::Struct { .. }]) => match spec_from(cmd) {
                Ok(spec) => start(&spec),
                Err(m) => Value::Tuple(vec![Value::Int(0), Value::Err(err(m))]),
            },
            _ => return Err(interp.fault(format!("proc.{name}: bad arguments"))),
        };
        Ok(v)
    }

    pub fn method(
        interp: &mut Interp,
        p: &ProcInner,
        name: &str,
        args: Vec<Value>,
    ) -> Result<Value, Fault> {
        let v = match (name, args.as_slice()) {
            ("pid", []) => Value::Int(p.pid),
            ("running", []) => Value::Bool(p.running()),
            ("readline", [Value::Ctx(c)]) => p.readline(c),
            ("wait", [Value::Ctx(c)]) => p.wait(c),
            ("stop", [Value::Int(grace)]) => p.stop(*grace),
            _ => {
                return Err(interp.fault(format!("Proc has no method {name} with those arguments")))
            }
        };
        Ok(v)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{call, method};

/// No processes in the browser; the module reports absence.
#[cfg(target_arch = "wasm32")]
pub fn call(
    interp: &mut crate::interp::Interp,
    name: &str,
    _args: Vec<crate::value::Value>,
) -> Result<crate::value::Value, crate::interp::Fault> {
    Err(interp.fault(format!("proc.{name} is not available in this build")))
}

#[cfg(target_arch = "wasm32")]
#[derive(Debug)]
pub struct ProcInner;

#[cfg(target_arch = "wasm32")]
pub fn method(
    interp: &mut crate::interp::Interp,
    _p: &ProcInner,
    name: &str,
    _args: Vec<crate::value::Value>,
) -> Result<crate::value::Value, crate::interp::Fault> {
    Err(interp.fault(format!("Proc.{name} is not available in this build")))
}
