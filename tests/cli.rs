use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_rikki")
}

fn tk() -> &'static str {
    env!("CARGO_BIN_EXE_tk")
}

fn tempdir(tag: &str) -> PathBuf {
    rikki::testutil::tempdir(&format!("cli-{tag}"))
}

/// Wait with a deadline so a child that stops exiting fails the test instead
/// of hanging the suite. Drops stdin first so the child sees EOF.
fn wait_within(mut child: std::process::Child, secs: u64) -> std::process::Output {
    drop(child.stdin.take());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            let out = child.wait_with_output().unwrap();
            panic!(
                "child still running after {secs}s; stdout so far: {:?}",
                String::from_utf8_lossy(&out.stdout)
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    child.wait_with_output().unwrap()
}

#[test]
fn new_then_run() {
    let d = tempdir("new");
    let out = Command::new(bin())
        .args(["new", "hello"])
        .current_dir(&d)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(d.join("hello/rikki.toml").exists());
    // agent docs scaffold by default; the executable hook is opt-in
    let primer = std::fs::read_to_string(d.join("hello/AGENTS.md")).unwrap();
    assert!(primer.contains("py bridge"), "primer content missing");
    let claude = std::fs::read_to_string(d.join("hello/CLAUDE.md")).unwrap();
    assert!(claude.contains("@AGENTS.md"), "{claude}");
    assert!(!d.join("hello/.claude").exists(), "hook must be opt-in");
    // the flag is discoverable from the success message
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("--claude-hook"),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let out = Command::new(bin())
        .args(["new", "hooked", "--claude-hook"])
        .current_dir(&d)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let settings = std::fs::read_to_string(d.join("hooked/.claude/settings.json")).unwrap();
    assert!(settings.contains("rikki-check.py"), "{settings}");
    assert!(d.join("hooked/.claude/hooks/rikki-check.py").exists());
    let out = Command::new(bin())
        .args(["run", "src/main.rk"])
        .current_dir(d.join("hello"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello, rikki\n");
}

#[test]
fn new_refuses_existing_dir_and_touches_nothing() {
    let d = tempdir("new-exists");
    let proj = d.join("mine");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(proj.join("CLAUDE.md"), "precious\n").unwrap();
    std::fs::write(proj.join("AGENTS.md"), "mine\n").unwrap();
    for args in [vec!["new", "mine"], vec!["new", "mine", "--claude-hook"]] {
        let out = Command::new(bin()).args(&args).current_dir(&d).output().unwrap();
        assert!(!out.status.success(), "{args:?} must refuse an existing dir");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("already exists"),
            "{args:?}"
        );
    }
    // nothing inside was rewritten
    assert_eq!(
        std::fs::read_to_string(proj.join("CLAUDE.md")).unwrap(),
        "precious\n"
    );
    assert_eq!(
        std::fs::read_to_string(proj.join("AGENTS.md")).unwrap(),
        "mine\n"
    );
    assert!(!proj.join(".claude").exists());
    assert!(!proj.join("rikki.toml").exists());
}

#[test]
fn claude_hook_feeds_diagnostics_back() {
    let d = tempdir("hook");
    let out = Command::new(bin())
        .args(["new", "h", "--claude-hook"])
        .current_dir(&d)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let proj = d.join("h");
    let hook = proj.join(".claude/hooks/rikki-check.py");
    let bin_dir = PathBuf::from(bin()).parent().unwrap().to_path_buf();
    let run_hook = |file: PathBuf| {
        let mut child = Command::new("python3")
            .arg(&hook)
            .env(
                "PATH",
                format!("{}:{}", bin_dir.display(), std::env::var("PATH").unwrap()),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let payload = format!(r#"{{"tool_input": {{"file_path": "{}"}}}}"#, file.display());
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(payload.as_bytes())
            .unwrap();
        wait_within(child, 30)
    };
    // a bad edit comes back as diagnostics on stderr with exit 2
    std::fs::write(
        proj.join("src/main.rk"),
        "fn main() {\n    if 1 {\n        print(\"x\")\n    }\n}\n",
    )
    .unwrap();
    let out = run_hook(proj.join("src/main.rk"));
    assert_eq!(out.status.code(), Some(2), "bad edit must exit 2");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("condition must be bool"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // a clean edit is silent success
    std::fs::write(proj.join("src/main.rk"), "fn main() {\n    print(\"ok\")\n}\n").unwrap();
    let out = run_hook(proj.join("src/main.rk"));
    assert_eq!(out.status.code(), Some(0), "clean edit must exit 0");
    // non-.rk files are ignored
    let out = run_hook(proj.join("notes.md"));
    assert_eq!(out.status.code(), Some(0), "non-rk file must exit 0");
}

#[test]
fn check_reports_and_fails() {
    let d = tempdir("check");
    let bad = d.join("bad.rk");
    std::fs::write(
        &bad,
        "fn main() {\n    if 1 {\n        print(\"x\")\n    }\n}\n",
    )
    .unwrap();
    let out = Command::new(bin())
        .args(["check"])
        .arg(&bad)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("condition must be bool"), "{err}");
    // check never runs the program
    let ok = d.join("ok.rk");
    std::fs::write(&ok, "fn main() {\n    print(\"ran\")\n}\n").unwrap();
    let out = Command::new(bin())
        .args(["check"])
        .arg(&ok)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn bare_run_resolves_project_main() {
    let d = tempdir("bare-run");
    let out = Command::new(bin())
        .args(["new", "hello"])
        .current_dir(&d)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // from the project root
    let out = Command::new(bin())
        .args(["run"])
        .current_dir(d.join("hello"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello, rikki\n");
    // and from a subdirectory, walking up to rikki.toml
    let out = Command::new(bin())
        .args(["run"])
        .current_dir(d.join("hello/src"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello, rikki\n");
    // bare check works the same way and runs nothing
    let out = Command::new(bin())
        .args(["check"])
        .current_dir(d.join("hello"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn bare_run_outside_project_errors() {
    let d = tempdir("bare-none");
    for cmd in ["run", "check"] {
        let out = Command::new(bin())
            .args([cmd])
            .current_dir(&d)
            .output()
            .unwrap();
        assert!(
            !out.status.success(),
            "bare {cmd} should fail outside a project"
        );
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("no file given and no rikki project found"),
            "{cmd}: {err}"
        );
    }
}

#[test]
fn tk_runs_file() {
    let d = tempdir("tk-file");
    let f = d.join("hi.rk");
    std::fs::write(&f, "fn main() {\n    print(\"hi from tk\")\n}\n").unwrap();
    let out = Command::new(tk()).arg(&f).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hi from tk\n");
}

#[test]
fn tk_version_flag() {
    let out = Command::new(tk()).arg("--version").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.starts_with("tk "), "{stdout}");
    assert!(stdout.contains("python"), "{stdout}");
}

#[test]
fn tk_bare_is_repl() {
    let mut child = Command::new(tk())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"1 + 2\nx := 40\nx + 2\n")
        .unwrap();
    let out = wait_within(child, 30);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("3\n"), "{stdout}");
    assert!(stdout.contains("42\n"), "{stdout}");
}

#[test]
fn repl_survives_a_failing_line() {
    // the first line faults (integer overflow); the repl must report it and
    // keep evaluating, never die. Pins the repl's panic net too: before the
    // net existed, a panicking builtin killed the whole process here.
    let mut child = Command::new(bin())
        .arg("repl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"[9223372036854775807, 1].sum()\n1 + 2\n")
        .unwrap();
    let out = wait_within(child, 30);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("3\n"), "{stdout}");
}

#[test]
fn tk_shebang_script_executes_directly() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempdir("tk-shebang");
    let script = d.join("greet");
    std::fs::write(
        &script,
        format!(
            "#!{}\nfn main() {{\n    print(\"hi from script\")\n}}\n",
            tk()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = Command::new(&script).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hi from script\n");
}

#[test]
fn repl_evaluates() {
    let mut child = Command::new(bin())
        .arg("repl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"1 + 2\nx := 40\nx + 2\nfn d(n int) int { return n * 2 }\nd(5)\n")
        .unwrap();
    let out = wait_within(child, 30);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("3\n"), "{stdout}");
    assert!(stdout.contains("42\n"), "{stdout}");
    assert!(stdout.contains("10\n"), "{stdout}");
}

#[test]
fn program_args_and_input() {
    let d = tempdir("argsin");
    let f = d.join("echo.rk");
    std::fs::write(
        &f,
        "fn main() {\n    for _, a := range args() {\n        print(a)\n    }\n    for {\n        line, err := input(\"> \")\n        if err != none {\n            break\n        }\n        print(\"got: \" + line)\n    }\n}\n",
    )
    .unwrap();
    let mut child = Command::new(tk())
        .arg(&f)
        .args([":8080", "llama"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"hello\nworld\n")
        .unwrap();
    let out = wait_within(child, 30);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(":8080\n"), "{stdout}");
    assert!(stdout.contains("llama\n"), "{stdout}");
    assert!(stdout.contains("got: hello\n"), "{stdout}");
    assert!(stdout.contains("got: world\n"), "{stdout}");
}

#[test]
fn http_get_post_request_from_rikki() {
    use std::io::Read;
    // echoes the request line and whether the custom header and body arrived
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for _ in 0..3 {
            let Ok((mut sock, _)) = listener.accept() else {
                return;
            };
            sock.set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            // real framing: headers to \r\n\r\n, then exactly content-length
            let mut req = Vec::new();
            let mut buf = [0u8; 4096];
            let hdr_end = loop {
                match sock.read(&mut buf) {
                    Ok(0) | Err(_) => break None,
                    Ok(n) => {
                        req.extend_from_slice(&buf[..n]);
                        if let Some(p) = req.windows(4).position(|w| w == b"\r\n\r\n") {
                            break Some(p + 4);
                        }
                    }
                }
            };
            let clen: usize = hdr_end
                .map(|end| {
                    String::from_utf8_lossy(&req[..end])
                        .lines()
                        .find_map(|l| {
                            let (k, v) = l.split_once(':')?;
                            k.eq_ignore_ascii_case("content-length")
                                .then(|| v.trim().parse().ok())?
                        })
                        .unwrap_or(0)
                })
                .unwrap_or(0);
            while req.len() < hdr_end.unwrap_or(0) + clen {
                match sock.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => req.extend_from_slice(&buf[..n]),
                }
            }
            let req = String::from_utf8_lossy(&req).to_string();
            let first = req.lines().next().unwrap_or("").to_string();
            let body = format!(
                "{first} hdr={} body={}",
                req.contains("x-k: v"),
                req.contains("hello") || req.contains("data"),
            );
            let resp = format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = std::io::Write::write_all(&mut sock, resp.as_bytes());
        }
    });
    let d = tempdir("http-req");
    let f = d.join("h.rk");
    std::fs::write(
        &f,
        r#"import "http"
import "ctx"

fn main() (error?) {
    base := args()[0]
    c := ctx.background()
    r1 := check http.get(c, base + "one")
    print(r1.status)
    print(r1.body)
    r2 := check http.post(c, base + "two", "hello")
    print(r2.body)
    req := Request{method: "PUT", url: base + "three", body: "data", headers: map[str]str{"x-k": "v"}}
    r3 := check http.request(c, req)
    print(r3.body)
    return none
}
"#,
    )
    .unwrap();
    let out = Command::new(tk())
        .arg(&f)
        .arg(format!("http://{addr}/"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("200\n"), "{stdout}");
    assert!(
        stdout.contains("GET /one HTTP/1.1 hdr=false body=false\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains("POST /two HTTP/1.1 hdr=false body=true\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains("PUT /three HTTP/1.1 hdr=true body=true\n"),
        "{stdout}"
    );
}

#[test]
fn http_stream_lines_reach_handler() {
    use std::io::Read;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            let mut buf = [0u8; 4096];
            let _ = sock.read(&mut buf);
            let body = "data: one\n\ndata: two\n\ndata: [DONE]\n";
            let resp = format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = std::io::Write::write_all(&mut sock, resp.as_bytes());
        }
    });
    let d = tempdir("stream");
    let f = d.join("s.rk");
    std::fs::write(
        &f,
        "import \"http\"\nimport \"ctx\"\n\nfn main() (error?) {\n    resp := check http.stream(ctx.background(), args()[0], \"{}\", fn(line str) {\n        if line.starts_with(\"data: \") {\n            print(\"got \" + line[6:len(line)])\n        }\n    })\n    print(resp.status)\n    return none\n}\n",
    )
    .unwrap();
    let out = Command::new(tk())
        .arg(&f)
        .arg(format!("http://{addr}/"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("got one\n"), "{stdout}");
    assert!(stdout.contains("got two\n"), "{stdout}");
    assert!(stdout.contains("got [DONE]\n"), "{stdout}");
    assert!(stdout.contains("200\n"), "{stdout}");
}
