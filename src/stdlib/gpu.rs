//! GPU sharing: nevla speaks the gputex lock protocol directly
//! (gputex docs/PROTOCOL.md), so a .nv program takes a card without
//! wrapping itself in the gputex CLI. Each lock is an flock on
//! $GPUTEX_DIR/<card>.lock (default ~/.gputex); it releases when this
//! process exits, however it exits, so a crash never strands a card.
//! Holding is per-interpreter: one program run (or one REPL session)
//! is one holder. A program may hold several cards at once (train on
//! one while embedding on another), one hold per card.

use std::fs::File;

use crate::interp::{Fault, Interp};
use crate::value::{ErrVal, Value};

/// An acquired card: the flock lives exactly as long as the fd.
pub struct Held {
    #[allow(dead_code)] // wasm builds never construct one
    file: File,
}

fn err(msg: impl Into<String>) -> Value {
    Value::Err(ErrVal {
        msg: msg.into(),
        ..Default::default()
    })
}

#[cfg(not(unix))]
pub fn call(interp: &mut Interp, name: &str, _args: Vec<Value>) -> Result<Value, Fault> {
    Err(interp.fault(format!("gpu.{name} is not available in this build")))
}

#[cfg(unix)]
pub fn call(interp: &mut Interp, name: &str, args: Vec<Value>) -> Result<Value, Fault> {
    let v = match (name, args.as_slice()) {
        ("lock", [Value::Str(card), Value::Str(label)]) => {
            match acquire(interp, card, label, libc::LOCK_EX, false) {
                Ok(None) => Value::NoneV,
                Ok(Some(busy)) => err(busy), // unreachable: blocking acquire never reports busy
                Err(e) => err(e),
            }
        }
        // a probe, not a demand: holding it ourselves is just "busy"
        ("trylock", [Value::Str(card), Value::Str(_)])
            if interp.gpu_holds.contains_key(card.as_str()) =>
        {
            Value::Tuple(vec![Value::Bool(false), Value::NoneV])
        }
        ("trylock", [Value::Str(card), Value::Str(label)]) => {
            match acquire(interp, card, label, libc::LOCK_EX | libc::LOCK_NB, false) {
                Ok(None) => Value::Tuple(vec![Value::Bool(true), Value::NoneV]),
                Ok(Some(_)) => Value::Tuple(vec![Value::Bool(false), Value::NoneV]),
                Err(e) => Value::Tuple(vec![Value::Bool(false), err(e)]),
            }
        }
        ("shared", [Value::Str(card), Value::Str(label)]) => {
            match acquire(interp, card, label, libc::LOCK_SH, true) {
                Ok(None) => Value::NoneV,
                Ok(Some(busy)) => err(busy),
                Err(e) => err(e),
            }
        }
        ("unlock", [Value::Str(card)]) => match interp.gpu_holds.remove(card.as_str()) {
            None => err(format!("gpu: not holding {card}")),
            Some(_h) => {
                let _ = std::fs::remove_file(holder_file(card));
                // dropping the hold closes the fd, which releases the flock
                Value::NoneV
            }
        },
        _ => return Err(interp.fault(format!("gpu.{name}: bad arguments"))),
    };
    Ok(v)
}

/// Take a card. Ok(None) = acquired; Ok(Some(msg)) = busy (only under
/// LOCK_NB); Err = a real failure worth an error value.
#[cfg(unix)]
fn acquire(
    interp: &mut Interp,
    card: &str,
    label: &str,
    op: i32,
    preemptible: bool,
) -> Result<Option<String>, String> {
    // the card id becomes a filename in the shared state dir
    if card.is_empty() || card.contains(['/', '\0']) {
        return Err(format!("gpu: bad card id {card:?}"));
    }
    if interp.gpu_holds.contains_key(card) {
        return Err(format!("gpu: already holding {card}; unlock first"));
    }
    let dir = state_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("gpu: {}: {e}", dir.display()))?;
    let path = dir.join(format!("{card}.lock"));
    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|e| format!("gpu: {}: {e}", path.display()))?;
    // exclusive blocking acquires evict what the protocol marks evictable
    // before settling into the kernel wait (spec 15.7: shared holders all
    // yield to an exclusive acquirer, which may terminate them)
    let preempted = op == libc::LOCK_EX && preempt_for_exclusive(&file, card);
    if !preempted {
        loop {
            use std::os::unix::io::AsRawFd;
            if unsafe { libc::flock(file.as_raw_fd(), op) } == 0 {
                break;
            }
            let e = std::io::Error::last_os_error();
            match e.raw_os_error() {
                Some(libc::EWOULDBLOCK) => return Ok(Some(format!("gpu: {card} busy"))),
                Some(libc::EINTR) => continue, // a signal woke the wait; keep waiting
                _ => return Err(format!("gpu: lock {card}: {e}")),
            }
        }
    }
    // self-report who holds it; advisory, so a failed write is not fatal
    let _ = write_holder(card, label, preemptible);
    inject_env();
    interp.gpu_holds.insert(card.to_string(), Held { file });
    Ok(None)
}

/// Evict preemptible holders ahead of an exclusive acquire. Returns
/// true when the lock landed here; false settles the caller into the
/// plain blocking wait (nothing evictable, or a non-preemptible holder
/// is present). Bounded rounds: flock has no fairness, so a new shared
/// holder can barge in between the kills and the lock; retrying a few
/// times beats looping forever.
#[cfg(unix)]
fn preempt_for_exclusive(file: &std::fs::File, card: &str) -> bool {
    use std::os::unix::io::AsRawFd;
    let try_nb = || unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
    let me = std::process::id() as i64;
    let host = hostname();
    for _round in 0..3 {
        if try_nb() {
            return true;
        }
        let mut signaled: Vec<i64> = vec![];
        for (path, pid, holder_host, preemptible) in read_holders(card) {
            if pid == me {
                continue;
            }
            // the registry is advisory and readers prune the dead
            let alive = unsafe { libc::kill(pid as libc::pid_t, 0) } == 0
                || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
            if !alive {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            // never signal across hosts; never touch the non-preemptible
            if holder_host != host || !preemptible {
                continue;
            }
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
            signaled.push(pid);
        }
        if signaled.is_empty() {
            return false;
        }
        // give the terminated ~10s to exit, polling for the lock
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if try_nb() {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        // survivors get the hard version; their holder files prune as
        // dead entries on the next round's registry read
        for pid in signaled {
            if unsafe { libc::kill(pid as libc::pid_t, 0) } == 0 {
                unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    false
}

/// Every registry entry for a card: (file, pid, host, preemptible).
/// Minimal field extraction; nevla and the gputex CLI write the schema.
#[cfg(unix)]
fn read_holders(card: &str) -> Vec<(std::path::PathBuf, i64, String, bool)> {
    let dir = state_dir().join(format!("{card}.holders"));
    let Ok(rd) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut out = vec![];
    for e in rd.flatten() {
        let path = e.path();
        if path.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(pid) = json_i64(&text, "pid") else {
            continue;
        };
        let host = json_str(&text, "host").unwrap_or_default();
        // absent means evictable: gputex --low writers always set it, and
        // an exclusive CLI holder writes preemptible:false
        let preemptible = json_bool(&text, "preemptible").unwrap_or(false);
        out.push((path, pid, host, preemptible));
    }
    out
}

fn json_field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let at = text.find(&format!("\"{key}\":"))? + key.len() + 3;
    Some(text[at..].trim_start())
}

fn json_i64(text: &str, key: &str) -> Option<i64> {
    let rest = json_field(text, key)?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn json_bool(text: &str, key: &str) -> Option<bool> {
    let rest = json_field(text, key)?;
    if rest.starts_with("true") {
        Some(true)
    } else if rest.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

/// A JSON string value, unescaping only what jstr escapes.
fn json_str(text: &str, key: &str) -> Option<String> {
    let rest = json_field(text, key)?.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    let n = u32::from_str_radix(&hex, 16).ok()?;
                    out.push(char::from_u32(n)?);
                }
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

fn state_dir() -> std::path::PathBuf {
    match std::env::var("GPUTEX_DIR") {
        Ok(d) if !d.is_empty() => d.into(),
        _ => {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            std::path::PathBuf::from(home).join(".gputex")
        }
    }
}

fn holder_file(card: &str) -> std::path::PathBuf {
    state_dir()
        .join(format!("{card}.holders"))
        .join(format!("{}.json", std::process::id()))
}

#[cfg(unix)]
fn write_holder(card: &str, label: &str, preemptible: bool) -> std::io::Result<()> {
    let f = holder_file(card);
    std::fs::create_dir_all(f.parent().unwrap())?;
    let cmd: Vec<String> = std::env::args().collect();
    let json = format!(
        "{{\"label\":{},\"framework\":\"nevla\",\"pid\":{},\"host\":{},\"started\":{},\"cmd\":{},\"preemptible\":{}}}\n",
        jstr(label),
        std::process::id(),
        jstr(&hostname()),
        jstr(&rfc3339_utc(std::time::SystemTime::now())),
        jstr(&cmd.join(" ")),
        preemptible,
    );
    std::fs::write(f, json)
}

/// The managed environment (PROTOCOL.md): every acquirer injects
/// /etc/gputex/env into its environment, existing values winning. This is
/// how the metrics contract holds even when no gputex CLI is involved.
fn inject_env() {
    let path = std::env::var("GPUTEX_ENV_FILE").unwrap_or_else(|_| "/etc/gputex/env".into());
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        if std::env::var_os(k).is_none() {
            std::env::set_var(k, v);
        }
    }
}

fn jstr(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(unix)]
fn hostname() -> String {
    let mut buf = [0u8; 256];
    let r = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
    if r != 0 {
        return "unknown".into();
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

/// UTC RFC 3339 without a date crate (civil-from-days, Hinnant's algorithm).
fn rfc3339_utc(t: std::time::SystemTime) -> String {
    let secs = t
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn rfc3339_known_instants() {
        let at = |s: u64| rfc3339_utc(UNIX_EPOCH + Duration::from_secs(s));
        assert_eq!(at(0), "1970-01-01T00:00:00Z");
        assert_eq!(at(86_399), "1970-01-01T23:59:59Z");
        assert_eq!(at(86_400), "1970-01-02T00:00:00Z");
        assert_eq!(at(951_782_400), "2000-02-29T00:00:00Z"); // leap day
        assert_eq!(at(1_767_225_599), "2025-12-31T23:59:59Z");
    }

    #[test]
    fn jstr_escapes() {
        assert_eq!(jstr(r#"a"b\c"#), r#""a\"b\\c""#);
        assert_eq!(jstr("x\ny"), "\"x\\u000ay\"");
    }

    // one test, not several: GPUTEX_DIR is process-global and lib tests run
    // in parallel, so everything that pins it lives in a single sequence
    #[test]
    fn lock_registers_conflicts_and_releases() {
        use crate::ast::Program;
        let dir = std::env::temp_dir().join(format!("nevla-gpu-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("GPUTEX_DIR", &dir);

        let card = || Value::Str("cardX".into());
        let prog = Program::default();
        let mut interp = Interp::new(&prog);
        let ok = call(
            &mut interp,
            "lock",
            vec![card(), Value::Str("unit \"test\"".into())],
        )
        .map_err(|f| f.msg)
        .unwrap();
        assert!(matches!(ok, Value::NoneV));

        let hf = holder_file("cardX");
        let json = std::fs::read_to_string(&hf).unwrap();
        assert!(json.contains(r#""framework":"nevla""#));
        assert!(json.contains(r#""label":"unit \"test\"""#));
        assert!(json.contains(&format!(r#""pid":{}"#, std::process::id())));

        // a card id may not escape the state dir
        match call(
            &mut interp,
            "lock",
            vec![Value::Str("../oops".into()), Value::Str("escape".into())],
        )
        .map_err(|f| f.msg)
        .unwrap()
        {
            Value::Err(e) => assert!(e.msg.contains("bad card id")),
            v => panic!("{v:?}"),
        }

        // a second interpreter (a would-be second holder) sees the card busy
        // through the kernel, not through our in-interp bookkeeping
        let mut other = Interp::new(&prog);
        match call(
            &mut other,
            "trylock",
            vec![card(), Value::Str("rival".into())],
        )
        .map_err(|f| f.msg)
        .unwrap()
        {
            Value::Tuple(ts) => {
                assert!(matches!(ts[0], Value::Bool(false)), "flock must conflict");
                assert!(matches!(ts[1], Value::NoneV));
            }
            v => panic!("{v:?}"),
        }

        let ok = call(&mut interp, "unlock", vec![card()])
            .map_err(|f| f.msg)
            .unwrap();
        assert!(matches!(ok, Value::NoneV));
        assert!(!hf.exists(), "unlock must remove the holder record");

        // released: the rival can now take it. Poll briefly: on macOS a child
        // posix_spawned concurrently (other tests spawn uv) transiently
        // inherits our flock'd fd, so one probe can spuriously see busy.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            match call(
                &mut other,
                "trylock",
                vec![card(), Value::Str("rival".into())],
            )
            .map_err(|f| f.msg)
            .unwrap()
            {
                Value::Tuple(ts) if matches!(ts[0], Value::Bool(true)) => break,
                Value::Tuple(_) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                v => panic!("card still busy after release: {v:?}"),
            }
        }
        let ok = call(&mut other, "unlock", vec![card()])
            .map_err(|f| f.msg)
            .unwrap();
        assert!(matches!(ok, Value::NoneV));

        // an exclusive acquirer preempts a shared, preemptible holder
        // (spec 15.7: all yield to an exclusive acquirer, which may
        // terminate them). The holder is a forked child so the flock
        // conflict is real; its post-fork body is pure syscalls.
        let lock_path = state_dir().join("cardX.lock");
        let cpath = std::ffi::CString::new(lock_path.to_str().unwrap()).unwrap();
        let mut fds = [0i32; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        let child = unsafe { libc::fork() };
        assert!(child >= 0, "fork failed");
        if child == 0 {
            // child: SH-hold the card, report readiness, wait to be killed
            unsafe {
                let fd = libc::open(cpath.as_ptr(), libc::O_RDWR);
                if fd < 0 || libc::flock(fd, libc::LOCK_SH) != 0 {
                    libc::_exit(3);
                }
                let byte = [b'r'];
                libc::write(fds[1], byte.as_ptr().cast(), 1);
                loop {
                    libc::pause(); // SIGTERM's default disposition ends us
                }
            }
        }
        // parent: write the child's holder record (registry is advisory,
        // any writer will do), wait for the SH hold, then demand the card
        let holders = state_dir().join("cardX.holders");
        std::fs::create_dir_all(&holders).unwrap();
        std::fs::write(
            holders.join(format!("{child}.json")),
            format!(
                "{{\"label\":\"kid\",\"framework\":\"nevla\",\"pid\":{child},\"host\":{},\"preemptible\":true}}\n",
                jstr(&hostname()),
            ),
        )
        .unwrap();
        let mut ready = [0u8; 1];
        assert_eq!(
            unsafe { libc::read(fds[0], ready.as_mut_ptr().cast(), 1) },
            1,
            "child never signaled readiness"
        );
        let t0 = std::time::Instant::now();
        let ok = call(
            &mut interp,
            "lock",
            vec![card(), Value::Str("preemptor".into())],
        )
        .map_err(|f| f.msg)
        .unwrap();
        assert!(matches!(ok, Value::NoneV), "{ok:?}");
        assert!(
            t0.elapsed() < Duration::from_secs(15),
            "preemption took {:?}",
            t0.elapsed()
        );
        let mut status = 0i32;
        assert_eq!(unsafe { libc::waitpid(child, &mut status, 0) }, child);
        assert!(
            libc::WIFSIGNALED(status) && libc::WTERMSIG(status) == libc::SIGTERM,
            "child must die by SIGTERM, status {status}"
        );
        let ok = call(&mut interp, "unlock", vec![card()])
            .map_err(|f| f.msg)
            .unwrap();
        assert!(matches!(ok, Value::NoneV));
        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
        }
    }

    #[test]
    fn env_injection_fills_gaps_only() {
        let f = std::env::temp_dir().join(format!("nevla-gpu-env-{}", std::process::id()));
        std::fs::write(
            &f,
            "# managed\nNEVLA_GPU_TEST_FILL=from_file\nNEVLA_GPU_TEST_KEEP=from_file\n",
        )
        .unwrap();
        std::env::set_var("GPUTEX_ENV_FILE", &f);
        std::env::set_var("NEVLA_GPU_TEST_KEEP", "already_set");
        std::env::remove_var("NEVLA_GPU_TEST_FILL");

        inject_env();
        assert_eq!(std::env::var("NEVLA_GPU_TEST_FILL").unwrap(), "from_file");
        assert_eq!(std::env::var("NEVLA_GPU_TEST_KEEP").unwrap(), "already_set");
    }
}
