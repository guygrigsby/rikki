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
    /// Typecheck and run a program (defaults to the project's src/main.mg)
    Run { file: Option<PathBuf> },
    /// Typecheck only (defaults to the project's src/main.mg)
    Check { file: Option<PathBuf> },
    /// Scaffold a new project
    New { name: String },
    /// Interactive session
    Repl,
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
        Cmd::Run { file } => with_entry(file, |f| mongoose::report(mongoose::run_source(f))),
        Cmd::Check { file } => with_entry(file, |f| mongoose::report(mongoose::check_source(f))),
        Cmd::Py { cmd: PyCmd::Add { package } } => py_add(&package),
        Cmd::New { name } => new_project(&name),
        Cmd::Repl => {
            mongoose::repl::run();
            ExitCode::SUCCESS
        }
    }
}

/// Resolve the file to operate on: the explicit arg if given, otherwise the
/// enclosing project's src/main.mg; then run `f` on it.
fn with_entry(file: Option<PathBuf>, f: impl FnOnce(&std::path::Path) -> ExitCode) -> ExitCode {
    match mongoose::resolve_entry(file) {
        Ok(path) => f(&path),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn new_project(name: &str) -> ExitCode {
    let root = std::path::Path::new(name);
    if root.exists() {
        eprintln!("error: {name} already exists");
        return ExitCode::FAILURE;
    }
    let make = || -> std::io::Result<()> {
        std::fs::create_dir_all(root.join("src"))?;
        std::fs::write(
            root.join("mongoose.toml"),
            format!(
                "[project]\nname = {name:?}\npython = {:?}\n",
                mongoose::project::DEFAULT_PYTHON
            ),
        )?;
        std::fs::write(
            root.join("src").join("main.mg"),
            "fn main() {\n    print(\"hello, mongoose\")\n}\n",
        )?;
        std::fs::write(root.join(".gitignore"), ".mongoose/\n")?;
        Ok(())
    };
    match make() {
        Ok(()) => {
            println!("created {name}/ (mongoose.toml, src/main.mg)");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
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
