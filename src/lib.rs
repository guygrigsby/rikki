pub mod ast;
pub mod bridge;
pub mod builtins;
pub mod diag;
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

#[derive(Debug)]
pub enum ExitKind {
    Ok,
    CompileError(String),
    RuntimeError(String),
}

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
    // the interpreter's recursion cap (1000 rikki frames, several Rust
    // frames each) needs more stack than a default thread has in debug
    // builds; run on a dedicated big-stack thread.
    let path = path.to_path_buf();
    std::thread::Builder::new()
        .name("rikki-interp".into())
        .stack_size(64 * 1024 * 1024)
        .spawn(move || compile_and(&path, true, args, stream))
        .expect("spawn interpreter thread")
        .join()
        .unwrap_or_else(|_| RunResult {
            stdout: String::new(),
            exit: ExitKind::RuntimeError("internal interpreter panic".into()),
        })
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
    compile_and(path, false, vec![], false)
}

fn compile_err(msg: impl Into<String>) -> RunResult {
    RunResult {
        stdout: String::new(),
        exit: ExitKind::CompileError(msg.into()),
    }
}

fn compile_and(path: &Path, run: bool, args: Vec<String>, stream: bool) -> RunResult {
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
                run && (!proj.py_deps.is_empty() || root.join("rikki.lock").exists());
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
    if let Err(diags) = typecheck::check(&prog) {
        let msg = diags
            .iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        return compile_err(msg);
    }
    if !run {
        return RunResult {
            stdout: String::new(),
            exit: ExitKind::Ok,
        };
    }
    let mut interp = interp::Interp::new(&prog);
    interp.set_args(args);
    if stream {
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

/// PyPI treats `-` and `_` as interchangeable in package names; imports use
/// the module name. Match declared deps against import names accordingly.
fn dep_declared(proj: &project::Project, module: &str) -> bool {
    let norm = |s: &str| s.replace('-', "_").to_lowercase();
    let m = norm(module);
    proj.py_deps.keys().any(|k| norm(k) == m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashed_dep_satisfies_underscored_import() {
        let proj = project::Project {
            root: std::path::PathBuf::new(),
            name: "x".into(),
            python: "3.12".into(),
            py_deps: [("sentence-transformers".to_string(), "*".to_string())].into(),
        };
        assert!(dep_declared(&proj, "sentence_transformers"));
        assert!(dep_declared(&proj, "sentence-transformers"));
        assert!(!dep_declared(&proj, "torch"));
    }
}
