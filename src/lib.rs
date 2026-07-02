pub mod diag;
pub mod lexer;
pub mod token;

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

pub fn run_source(_path: &Path) -> RunResult {
    RunResult {
        stdout: String::new(),
        exit: ExitKind::CompileError("unimplemented".into()),
    }
}
