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
    // the interpreter's recursion cap (1000 mongoose frames, several Rust
    // frames each) needs more stack than a default thread has in debug
    // builds; run on a dedicated big-stack thread.
    let path = path.to_path_buf();
    std::thread::Builder::new()
        .name("mongoose-interp".into())
        .stack_size(64 * 1024 * 1024)
        .spawn(move || compile_and(&path, true))
        .expect("spawn interpreter thread")
        .join()
        .unwrap_or_else(|_| RunResult {
            stdout: String::new(),
            exit: ExitKind::RuntimeError("internal interpreter panic".into()),
        })
}

/// Typecheck only; never provisions an environment or runs code.
pub fn check_source(path: &Path) -> RunResult {
    compile_and(path, false)
}

fn compile_err(msg: impl Into<String>) -> RunResult {
    RunResult { stdout: String::new(), exit: ExitKind::CompileError(msg.into()) }
}

fn compile_and(path: &Path, run: bool) -> RunResult {
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
            let provision =
                run && (!proj.py_deps.is_empty() || root.join("mongoose.lock").exists());
            if provision {
                if let Err(e) = proj.ensure_env("uv") {
                    return compile_err(e);
                }
                bridge::init(Some(&proj.venv()));
            }
            for m in &py_imports {
                let top = m.split('.').next().unwrap_or(m);
                if !proj.py_deps.contains_key(top) && !bridge::is_stdlib(top) {
                    return compile_err(format!(
                        "import py {m:?}: not declared in mongoose.toml; run: mongoose py add {top}"
                    ));
                }
            }
        }
        // no project: bare interpreter, stdlib python only
    }
    if let Err(diags) = typecheck::check(&prog) {
        let msg = diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n");
        return compile_err(msg);
    }
    if !run {
        return RunResult { stdout: String::new(), exit: ExitKind::Ok };
    }
    let mut interp = interp::Interp::new(&prog);
    let result = interp.run_main();
    let stdout = std::mem::take(&mut interp.out);
    match result {
        Ok(None) => RunResult { stdout, exit: ExitKind::Ok },
        Ok(Some(e)) => RunResult { stdout, exit: ExitKind::RuntimeError(e.msg) },
        Err(f) => {
            let mut msg = f.msg;
            for frame in f.stack.iter().rev() {
                msg.push_str(&format!("\n  at {frame}"));
            }
            RunResult { stdout, exit: ExitKind::RuntimeError(msg) }
        }
    }
}
