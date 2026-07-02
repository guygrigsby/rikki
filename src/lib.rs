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
    compile_and(path, true)
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
/// given, otherwise the enclosing project's src/main.mg.
pub fn resolve_entry(file: Option<std::path::PathBuf>) -> Result<std::path::PathBuf, String> {
    if let Some(f) = file {
        return Ok(f);
    }
    let cwd = std::env::current_dir().map_err(|e| format!("cannot read cwd: {e}"))?;
    let Some(root) = project::Project::find(&cwd) else {
        return Err(
            "no file given and no mongoose project found (no mongoose.toml in this or any parent directory)"
                .into(),
        );
    };
    let main = root.join("src").join("main.mg");
    if !main.exists() {
        return Err(format!("no file given and project entrypoint {} is missing", main.display()));
    }
    Ok(main)
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
