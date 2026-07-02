use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_mongoose")
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
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(d.join("hello/mongoose.toml").exists());
    let out = Command::new(bin())
        .args(["run", "src/main.mg"])
        .current_dir(d.join("hello"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello, mongoose\n");
}

#[test]
fn check_reports_and_fails() {
    let d = tempdir("check");
    let bad = d.join("bad.mg");
    std::fs::write(&bad, "fn main() {\n    if 1 {\n        print(\"x\")\n    }\n}\n").unwrap();
    let out = Command::new(bin()).args(["check"]).arg(&bad).output().unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("condition must be bool"), "{err}");
    // check never runs the program
    let ok = d.join("ok.mg");
    std::fs::write(&ok, "fn main() {\n    print(\"ran\")\n}\n").unwrap();
    let out = Command::new(bin()).args(["check"]).arg(&ok).output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
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
        .write_all(b"1 + 2\nx := 40\nx + 2\nfn d(n: int) int { return n * 2 }\nd(5)\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("3\n"), "{stdout}");
    assert!(stdout.contains("42\n"), "{stdout}");
    assert!(stdout.contains("10\n"), "{stdout}");
}
