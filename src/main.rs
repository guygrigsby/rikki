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
    /// Python dependency management
    Py {
        #[command(subcommand)]
        cmd: PyCmd,
    },
}

#[derive(Subcommand)]
enum PyCmd {
    /// Declare a Python dependency and sync the environment
    Add { package: String },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Run { file } => report(mongoose::run_source(&file)),
        Cmd::Check { file } => report(mongoose::check_source(&file)),
        Cmd::Py { cmd: PyCmd::Add { package } } => py_add(&package),
    }
}

fn report(res: mongoose::RunResult) -> ExitCode {
    print!("{}", res.stdout);
    match res.exit {
        mongoose::ExitKind::Ok => ExitCode::SUCCESS,
        mongoose::ExitKind::CompileError(m) | mongoose::ExitKind::RuntimeError(m) => {
            eprintln!("error: {m}");
            ExitCode::FAILURE
        }
    }
}

fn py_add(package: &str) -> ExitCode {
    let cwd = std::env::current_dir().expect("cwd");
    let Some(root) = mongoose::project::Project::find(&cwd) else {
        eprintln!("error: no mongoose.toml found; run: mongoose new <name>");
        return ExitCode::FAILURE;
    };
    let mut proj = match mongoose::project::Project::load(&root) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    match proj.py_add(package, "uv") {
        Ok(()) => {
            println!("added {package}; lock and env updated");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
