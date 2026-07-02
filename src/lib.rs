pub mod ast;
pub mod builtins;
pub mod diag;
pub mod interp;
pub mod lexer;
pub mod loader;
pub mod parser;
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
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return RunResult {
                stdout: String::new(),
                exit: ExitKind::CompileError(format!("{}: {e}", path.display())),
            }
        }
    };
    drop(src);
    let prog = match loader::load(path) {
        Ok(p) => p,
        Err(d) => {
            return RunResult { stdout: String::new(), exit: ExitKind::CompileError(d.to_string()) }
        }
    };
    if let Err(diags) = typecheck::check(&prog) {
        let msg = diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n");
        return RunResult { stdout: String::new(), exit: ExitKind::CompileError(msg) };
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
