pub mod ast;
#[cfg(not(target_arch = "wasm32"))]
pub mod bridge;
#[cfg(target_arch = "wasm32")]
#[path = "bridge_wasm.rs"]
pub mod bridge;
pub mod builtins;
pub mod diag;
mod fmt;
pub mod interp;
pub mod lexer;
pub mod loader;
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
        let d = std::env::temp_dir().join(format!("rikki-test-{}-{tag}", std::process::id()));
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
        .name("rikki-interp".into())
        .stack_size(64 * 1024 * 1024)
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
/// given, otherwise the enclosing project's src/main.rk.
pub fn resolve_entry(file: Option<std::path::PathBuf>) -> Result<std::path::PathBuf, String> {
    if let Some(f) = file {
        return Ok(f);
    }
    let cwd = std::env::current_dir().map_err(|e| format!("cannot read cwd: {e}"))?;
    let Some(root) = project::Project::find(&cwd) else {
        return Err(
            "no file given and no rikki project found (no rikki.toml in this or any parent directory)"
                .into(),
        );
    };
    let main = root.join("src").join("main.rk");
    if !main.exists() {
        return Err(format!(
            "no file given and project entrypoint {} is missing",
            main.display()
        ));
    }
    Ok(main)
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
    // python imports: validate against the project manifest, provision the env
    let py_imports: Vec<String> = prog
        .decls
        .iter()
        .filter_map(|d| match d {
            ast::Decl::Import { path, py: true, .. } => Some(path.clone()),
            _ => None,
        })
        .collect();
    if !py_imports.is_empty() {
        if let Some(root) = project::Project::find(path) {
            let proj = match project::Project::load(&root) {
                Ok(p) => p,
                Err(e) => return compile_err(e),
            };
            let embedded = bridge::embedded_python();
            if proj.python != embedded {
                return compile_err(format!(
                    "project pins python {} but this rikki embeds {embedded}; set python = {embedded:?} in rikki.toml and rerun rikki py add",
                    proj.python
                ));
            }
            let provision =
                mode == Mode::Run && (!proj.py_deps.is_empty() || root.join("rikki.lock").exists());
            if provision {
                if let Err(e) = proj.ensure_env("uv") {
                    return compile_err(e);
                }
                bridge::init(Some(&proj.venv()));
            }
            for m in &py_imports {
                let top = m.split('.').next().unwrap_or(m);
                if !dep_declared(&proj, top) && !bridge::is_stdlib(top) {
                    return compile_err(format!(
                        "import py {m:?}: not declared in rikki.toml; run: rikki py add {top}"
                    ));
                }
            }
        }
        // no project: bare interpreter, stdlib python only
    }
    check_then_run(&prog, mode, args, out)
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
            if let ast::Decl::Import { path, py: false, .. } = d {
                if path.ends_with(".rk") {
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
        let r = run_snippet("import \"util.rk\"\n\nfn main() {\n    print(1)\n}\n");
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
            py_deps: [(
                "sentence-transformers".to_string(),
                project::PyDep::any(),
            )]
            .into(),
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
