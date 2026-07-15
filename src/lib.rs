pub mod ast;
#[cfg(not(target_arch = "wasm32"))]
pub mod bridge;
#[cfg(target_arch = "wasm32")]
#[path = "bridge_wasm.rs"]
pub mod bridge;
pub mod builtins;
pub mod diag;
mod fmt;
pub mod imports;
pub mod format;
pub mod interp;
pub mod lexer;
pub mod loader;
pub mod model;
pub mod parser;
pub mod project;
pub mod repl;
pub mod stdlib;
pub mod token;
pub mod typecheck;
pub mod types;
pub mod value;

use std::path::Path;

/// The interpreter's version, for embedders (the playground shows it).
pub const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Shared by the unit and integration test suites; not API.
#[doc(hidden)]
pub mod testutil {
    /// A fresh, empty scratch directory keyed by pid and tag.
    pub fn tempdir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("nevla-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
}

/// Run the program after checking, or stop at the check.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Check,
    Run,
}

/// Buffer stdout into RunResult (tests, run_source) or stream it live
/// (interactive CLI).
#[derive(Clone, Copy, PartialEq)]
enum Output {
    Buffered,
    Streamed,
}

#[derive(Debug)]
pub enum ExitKind {
    Ok,
    CompileError(String),
    RuntimeError(String),
}

#[derive(Debug)]
pub struct RunResult {
    pub stdout: String,
    pub exit: ExitKind,
}

pub fn run_source(path: &Path) -> RunResult {
    run_with(path, vec![], false)
}

/// Run with program arguments; `stream` writes stdout live (interactive CLI)
/// instead of buffering into RunResult.
pub fn run_with(path: &Path, args: Vec<String>, stream: bool) -> RunResult {
    let out = if stream {
        Output::Streamed
    } else {
        Output::Buffered
    };
    let path = path.to_path_buf();
    on_interp_thread(move || compile_and(&path, Mode::Run, args, out))
}

/// Run under the panic net: a dedicated big-stack thread (the interpreter's
/// recursion cap needs more than a default debug-build stack) whose panic
/// becomes a RuntimeError instead of aborting the process.
#[cfg(not(target_arch = "wasm32"))]
fn on_interp_thread(f: impl FnOnce() -> RunResult + Send + 'static) -> RunResult {
    let internal_panic = || RunResult {
        stdout: String::new(),
        exit: ExitKind::RuntimeError("internal interpreter panic".into()),
    };
    let Ok(handle) = std::thread::Builder::new()
        .name("nevla-interp".into())
        // headroom over the recursion cap: 1000 nevla frames nest deep
        // Rust frames in debug builds, and the reserve is virtual memory
        .stack_size(128 * 1024 * 1024)
        .spawn(f)
    else {
        return RunResult {
            stdout: String::new(),
            exit: ExitKind::RuntimeError("cannot spawn interpreter thread".into()),
        };
    };
    handle.join().unwrap_or_else(|_| internal_panic())
}

/// wasm has no threads and panics trap; run inline and let the browser's
/// wasm sandbox be the net.
#[cfg(target_arch = "wasm32")]
fn on_interp_thread(f: impl FnOnce() -> RunResult + Send + 'static) -> RunResult {
    f()
}

/// Print a run's output and map its exit kind to a process exit code.
pub fn report(res: RunResult) -> std::process::ExitCode {
    print!("{}", res.stdout);
    match res.exit {
        ExitKind::Ok => std::process::ExitCode::SUCCESS,
        ExitKind::CompileError(m) | ExitKind::RuntimeError(m) => {
            eprintln!("error: {m}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Resolve the file a CLI command should operate on: the explicit arg if
/// given, otherwise the enclosing project's src/main.nv.
pub fn resolve_entry(file: Option<std::path::PathBuf>) -> Result<std::path::PathBuf, String> {
    if let Some(f) = file {
        return Ok(f);
    }
    let cwd = std::env::current_dir().map_err(|e| format!("cannot read cwd: {e}"))?;
    let Some(root) = project::Project::find(&cwd) else {
        return Err(
            "no file given and no nevla project found (no nevla.toml in this or any parent directory)"
                .into(),
        );
    };
    let main = root.join("src").join("main.nv");
    if !main.exists() {
        return Err(format!(
            "no file given and project entrypoint {} is missing",
            main.display()
        ));
    }
    Ok(main)
}

#[derive(Debug, PartialEq)]
pub enum TestStatus {
    Pass,
    Fail,
    Skip,
}

#[derive(Debug)]
pub struct TestOutcome {
    pub name: String,
    pub status: TestStatus,
    /// failure or skip report: "file:line: msg" when the error has an origin
    pub message: String,
    /// captured per test; the CLI shows it only on failure
    pub stdout: String,
}

/// Run every Test function in one `_test.nv` file, each in a fresh
/// interpreter, `jobs` at a time (0 = one per core). Per the testing
/// chapter: pass = returned none, skip = the test.skip sentinel, fail =
/// any other error or a fault; the run always continues.
pub fn run_test_file(path: &Path, jobs: usize) -> Result<Vec<TestOutcome>, String> {
    let prog = loader::load(path).map_err(|d| d.to_string())?;
    provision_py(&prog, path, true)?;
    typecheck::check_no_main(&prog).map_err(|ds| {
        ds.iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    let root = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut tests = vec![];
    for d in &prog.decls {
        if let ast::Decl::Fn(f) = d {
            if f.file.as_deref() == Some(root.as_str()) && f.name.starts_with("Test") {
                let ret_ok = matches!(
                    f.ret.as_slice(),
                    [ast::TypeExpr::Opt(t)]
                        if matches!(&**t, ast::TypeExpr::Named(n) if n == "error")
                );
                if !f.params.is_empty() || !ret_ok {
                    return Err(format!(
                        "{root}: {} must have the shape fn {}() (error?)",
                        f.name, f.name
                    ));
                }
                tests.push(f.name.clone());
            }
        }
    }
    let jobs = if jobs == 0 {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    } else {
        jobs
    }
    .clamp(1, tests.len().max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<std::sync::Mutex<Option<TestOutcome>>> =
        tests.iter().map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..jobs {
            let next = &next;
            let results = &results;
            let tests = &tests;
            let prog = &prog;
            std::thread::Builder::new()
                .stack_size(128 * 1024 * 1024)
                .spawn_scoped(scope, move || loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if i >= tests.len() {
                        break;
                    }
                    let name = tests[i].clone();
                    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let mut interp = interp::Interp::new(prog);
                        let r = interp.run_named(&name);
                        (r, interp.take_out())
                    }));
                    let (status, message, stdout) = match run {
                        Err(_) => (
                            TestStatus::Fail,
                            "internal interpreter panic".to_string(),
                            String::new(),
                        ),
                        Ok((Ok(None), so)) => (TestStatus::Pass, String::new(), so),
                        Ok((Ok(Some(e)), so)) => {
                            if e.pytype == stdlib::test::SKIP_MARKER {
                                (TestStatus::Skip, e.msg, so)
                            } else {
                                (TestStatus::Fail, error_report(&e), so)
                            }
                        }
                        Ok((Err(f), so)) => {
                            let mut msg = f.msg;
                            for frame in f.stack.iter().rev() {
                                msg.push_str(&format!("\n  at {frame}"));
                            }
                            (TestStatus::Fail, msg, so)
                        }
                    };
                    *results[i].lock().unwrap() = Some(TestOutcome {
                        name,
                        status,
                        message,
                        stdout,
                    });
                })
                .expect("spawn test worker");
        }
    });
    Ok(results
        .into_iter()
        .map(|m| m.into_inner().unwrap().expect("worker filled every slot"))
        .collect())
}

/// "origin: msg", then the cause chain.
fn error_report(e: &value::ErrVal) -> String {
    let mut msg = if e.origin.is_empty() {
        e.msg.clone()
    } else {
        format!("{}: {}", e.origin, e.msg)
    };
    let mut cause = e.cause.as_deref();
    while let Some(c) = cause {
        msg.push_str(&format!("\n  caused by: {}", c.msg));
        cause = c.cause.as_deref();
    }
    msg
}

/// Typecheck only; never provisions an environment or runs code.
pub fn check_source(path: &Path) -> RunResult {
    let path = path.to_path_buf();
    on_interp_thread(move || compile_and(&path, Mode::Check, vec![], Output::Buffered))
}

fn compile_err(msg: impl Into<String>) -> RunResult {
    RunResult {
        stdout: String::new(),
        exit: ExitKind::CompileError(msg.into()),
    }
}

fn compile_and(path: &Path, mode: Mode, args: Vec<String>, out: Output) -> RunResult {
    if !path.exists() {
        return compile_err(format!("{}: no such file", path.display()));
    }
    let prog = match loader::load(path) {
        Ok(p) => p,
        Err(d) => return compile_err(d.to_string()),
    };
    if let Err(e) = provision_py(&prog, path, mode == Mode::Run) {
        return compile_err(e);
    }
    check_then_run(&prog, mode, args, out)
}

/// Python imports: validate against the project manifest and, when the
/// program will actually run, provision the env (spec 17.5).
fn provision_py(prog: &ast::Program, path: &Path, will_run: bool) -> Result<(), String> {
    let py_imports: Vec<String> = prog
        .decls
        .iter()
        .filter_map(|d| match d {
            ast::Decl::Import { path, py: true, .. } => Some(path.clone()),
            _ => None,
        })
        .collect();
    if py_imports.is_empty() {
        return Ok(());
    }
    let Some(root) = project::Project::find(path) else {
        // no project: bare interpreter, stdlib python only
        return Ok(());
    };
    let proj = project::Project::load(&root)?;
    if let Some(built) = &proj.nevla {
        if built != PKG_VERSION {
            eprintln!(
                "warning: project was built against nevla {built}; this is {PKG_VERSION} (update the nevla pin in nevla.toml after verifying)"
            );
        }
    }
    let embedded = bridge::embedded_python();
    if proj.python != embedded {
        return Err(format!(
            "project pins python {} but this nevla embeds {embedded}; set python = {embedded:?} in nevla.toml and rerun nevla py add",
            proj.python
        ));
    }
    if will_run && (!proj.py_deps.is_empty() || root.join("nevla.lock").exists()) {
        proj.ensure_env("uv")?;
        bridge::init(Some(&proj.venv()));
    }
    for m in &py_imports {
        let top = m.split('.').next().unwrap_or(m);
        if !dep_declared(&proj, top) && !bridge::is_stdlib(top) {
            return Err(format!(
                "import py {m:?}: not declared in nevla.toml; run: nevla py add {top}"
            ));
        }
    }
    Ok(())
}

/// The back half of every entry point: typecheck, then interpret.
fn check_then_run(prog: &ast::Program, mode: Mode, args: Vec<String>, out: Output) -> RunResult {
    if let Err(diags) = typecheck::check(prog) {
        let msg = diags
            .iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        return compile_err(msg);
    }
    if mode == Mode::Check {
        return RunResult {
            stdout: String::new(),
            exit: ExitKind::Ok,
        };
    }
    let mut interp = interp::Interp::new(prog);
    interp.set_args(args);
    if out == Output::Streamed {
        interp.stream_stdout();
    }
    let result = interp.run_main();
    let stdout = interp.take_out();
    // Drop the interpreter's values here, then flush any py decrefs they
    // deferred, so an escaped `bytesview` (unsendable pyclass) is released on
    // this thread rather than a later run's. `result` carries no py handles.
    drop(interp);
    bridge::release_pending();
    match result {
        Ok(None) => RunResult {
            stdout,
            exit: ExitKind::Ok,
        },
        Ok(Some(e)) => RunResult {
            stdout,
            exit: ExitKind::RuntimeError(e.msg),
        },
        Err(f) => {
            let mut msg = f.msg;
            for frame in f.stack.iter().rev() {
                msg.push_str(&format!("\n  at {frame}"));
            }
            RunResult {
                stdout,
                exit: ExitKind::RuntimeError(msg),
            }
        }
    }
}

/// Compile and run source directly, no filesystem involved: the playground
/// entry point. File imports need files and are rejected; stdlib and py
/// imports follow the build (a build without python reports the error at
/// program start).
pub fn run_snippet(src: &str) -> RunResult {
    let src = src.to_string();
    on_interp_thread(move || {
        let prog = match parser::parse(&src) {
            Ok(p) => p,
            Err(d) => return compile_err(d.to_string()),
        };
        for d in &prog.decls {
            if let ast::Decl::Import {
                path, py: false, ..
            } = d
            {
                if path.ends_with(".nv") {
                    return compile_err(format!(
                        "import {path:?}: file imports are not available here"
                    ));
                }
            }
        }
        check_then_run(&prog, Mode::Run, vec![], Output::Buffered)
    })
}

/// PyPI treats `-` and `_` as interchangeable in package names; imports use
/// the module name. Match declared deps against import names accordingly.
fn dep_declared(proj: &project::Project, module: &str) -> bool {
    let norm = |s: &str| s.replace('-', "_").to_lowercase();
    let m = norm(module);
    // a module override replaces the package-name match (spec 17.5)
    proj.py_deps.iter().any(|(k, d)| match &d.module {
        Some(declared) => norm(declared) == m,
        None => norm(k) == m,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_runs_without_files() {
        let r = run_snippet("fn main() {\n    print(\"hi\")\n}\n");
        assert!(matches!(r.exit, ExitKind::Ok), "{:?}", r.exit);
        assert_eq!(r.stdout, "hi\n");
    }

    #[test]
    fn snippet_reports_compile_errors() {
        let r = run_snippet("fn main() {\n    if 1 {\n        print(\"x\")\n    }\n}\n");
        let ExitKind::CompileError(m) = r.exit else {
            panic!("{:?}", r.exit)
        };
        assert!(m.contains("condition must be bool"), "{m}");
    }

    #[test]
    fn snippet_rejects_file_imports() {
        let r = run_snippet("import \"util.nv\"\n\nfn main() {\n    print(1)\n}\n");
        let ExitKind::CompileError(m) = r.exit else {
            panic!("{:?}", r.exit)
        };
        assert!(m.contains("file imports are not available"), "{m}");
    }

    #[test]
    fn dashed_dep_satisfies_underscored_import() {
        let proj = project::Project {
            root: std::path::PathBuf::new(),
            name: "x".into(),
            python: "3.12".into(),
            nevla: None,
            py_deps: [("sentence-transformers".to_string(), project::PyDep::any())].into(),
        };
        assert!(dep_declared(&proj, "sentence_transformers"));
        assert!(dep_declared(&proj, "sentence-transformers"));
        assert!(!dep_declared(&proj, "torch"));
    }

    #[test]
    fn module_override_satisfies_exactly_that_import() {
        // mlflow-skinny = { module = "mlflow" } (spec 17.5)
        let proj = project::Project {
            root: std::path::PathBuf::new(),
            name: "x".into(),
            python: "3.12".into(),
            nevla: None,
            py_deps: [(
                "mlflow-skinny".to_string(),
                project::PyDep {
                    version: "*".into(),
                    module: Some("mlflow".into()),
                },
            )]
            .into(),
        };
        assert!(dep_declared(&proj, "mlflow"));
        // the override replaces the package-name match
        assert!(!dep_declared(&proj, "mlflow_skinny"));
        assert!(!dep_declared(&proj, "torch"));
    }
}
