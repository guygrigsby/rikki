use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "mongoose", version, about = "the mongoose language")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Typecheck and run a program
    Run { file: PathBuf },
    /// Typecheck only
    Check { file: PathBuf },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Run { file } | Cmd::Check { file } => {
            let res = mongoose::run_source(&file);
            print!("{}", res.stdout);
            match res.exit {
                mongoose::ExitKind::Ok => ExitCode::SUCCESS,
                mongoose::ExitKind::CompileError(m) | mongoose::ExitKind::RuntimeError(m) => {
                    eprintln!("error: {m}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}
