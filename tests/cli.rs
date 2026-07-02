use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_mongoose")
}

fn mg() -> &'static str {
    env!("CARGO_BIN_EXE_mg")
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
fn bare_run_resolves_project_main() {
    let d = tempdir("bare-run");
    let out = Command::new(bin())
        .args(["new", "hello"])
        .current_dir(&d)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    // from the project root
    let out = Command::new(bin())
        .args(["run"])
        .current_dir(d.join("hello"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello, mongoose\n");
    // and from a subdirectory, walking up to mongoose.toml
    let out = Command::new(bin())
        .args(["run"])
        .current_dir(d.join("hello/src"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello, mongoose\n");
    // bare check works the same way and runs nothing
    let out = Command::new(bin())
        .args(["check"])
        .current_dir(d.join("hello"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn bare_run_outside_project_errors() {
    let d = tempdir("bare-none");
    for cmd in ["run", "check"] {
        let out = Command::new(bin()).args([cmd]).current_dir(&d).output().unwrap();
        assert!(!out.status.success(), "bare {cmd} should fail outside a project");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("no file given and no mongoose project found"), "{cmd}: {err}");
    }
}

#[test]
fn mg_runs_file() {
    let d = tempdir("mg-file");
    let f = d.join("hi.mg");
    std::fs::write(&f, "fn main() {\n    print(\"hi from mg\")\n}\n").unwrap();
    let out = Command::new(mg()).arg(&f).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hi from mg\n");
}

#[test]
fn mg_bare_is_repl() {
    let mut child = Command::new(mg())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child.stdin.as_mut().unwrap().write_all(b"1 + 2\nx := 40\nx + 2\n").unwrap();
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("3\n"), "{stdout}");
    assert!(stdout.contains("42\n"), "{stdout}");
}

#[test]
fn mg_shebang_script_executes_directly() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempdir("mg-shebang");
    let script = d.join("greet");
    std::fs::write(
        &script,
        format!("#!{}\nfn main() {{\n    print(\"hi from script\")\n}}\n", mg()),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = Command::new(&script).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
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
        .write_all(b"1 + 2\nx := 40\nx + 2\nfn d(n: int) int { return n * 2 }\nd(5)\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("3\n"), "{stdout}");
    assert!(stdout.contains("42\n"), "{stdout}");
    assert!(stdout.contains("10\n"), "{stdout}");
}
