pub mod ast;
pub mod diag;
pub mod lexer;
pub mod parser;
pub mod token;
pub mod typecheck;
pub mod types;

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
    let prog = match parser::parse(&src) {
        Ok(p) => p,
        Err(d) => {
            return RunResult { stdout: String::new(), exit: ExitKind::CompileError(d.to_string()) }
        }
    };
    if let Err(diags) = typecheck::check(&prog) {
        let msg = diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n");
        return RunResult { stdout: String::new(), exit: ExitKind::CompileError(msg) };
    }
    RunResult {
        stdout: String::new(),
        exit: ExitKind::RuntimeError("no evaluator yet".into()),
    }
}
