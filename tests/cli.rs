use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_mongoose")
}

fn tk() -> &'static str {
    env!("CARGO_BIN_EXE_tk")
}

fn tempdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mongoose-cli-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
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
    assert!(d.join("hello/mongoose.toml").exists());
    let out = Command::new(bin())
        .args(["run", "src/main.mg"])
        .current_dir(d.join("hello"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello, mongoose\n");
}

#[test]
fn check_reports_and_fails() {
    let d = tempdir("check");
    let bad = d.join("bad.mg");
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
    let ok = d.join("ok.mg");
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
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello, mongoose\n");
    // and from a subdirectory, walking up to mongoose.toml
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
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello, mongoose\n");
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
            err.contains("no file given and no mongoose project found"),
            "{cmd}: {err}"
        );
    }
}

#[test]
fn tk_runs_file() {
    let d = tempdir("tk-file");
    let f = d.join("hi.mg");
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
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("3\n"), "{stdout}");
    assert!(stdout.contains("42\n"), "{stdout}");
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
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("3\n"), "{stdout}");
    assert!(stdout.contains("42\n"), "{stdout}");
    assert!(stdout.contains("10\n"), "{stdout}");
}

#[test]
fn program_args_and_input() {
    let d = tempdir("argsin");
    let f = d.join("echo.mg");
    std::fs::write(
        &f,
        "fn main() {\n    for a in args() {\n        print(a)\n    }\n    for {\n        line, err := input(\"> \")\n        if err != none {\n            break\n        }\n        print(\"got: \" + line)\n    }\n}\n",
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
    child.stdin.as_mut().unwrap().write_all(b"hello\nworld\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(":8080\n"), "{stdout}");
    assert!(stdout.contains("llama\n"), "{stdout}");
    assert!(stdout.contains("got: hello\n"), "{stdout}");
    assert!(stdout.contains("got: world\n"), "{stdout}");
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
    let f = d.join("s.mg");
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
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("got one\n"), "{stdout}");
    assert!(stdout.contains("got two\n"), "{stdout}");
    assert!(stdout.contains("got [DONE]\n"), "{stdout}");
    assert!(stdout.contains("200\n"), "{stdout}");
}
