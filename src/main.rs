use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "rikki", version, about = "the rikki language")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Typecheck and run a program (defaults to the project's src/main.rk)
    Run {
        file: Option<PathBuf>,
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Typecheck only (defaults to the project's src/main.rk)
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
        Cmd::Run { file, args } => {
            with_entry(file, |f| rikki::report(rikki::run_with(f, args.clone(), true)))
        }
        Cmd::Check { file } => with_entry(file, |f| rikki::report(rikki::check_source(f))),
        Cmd::Py {
            cmd: PyCmd::Add { package },
        } => py_add(&package),
        Cmd::New { name } => new_project(&name),
        Cmd::Repl => {
            rikki::repl::run();
            ExitCode::SUCCESS
        }
    }
}

/// Resolve the file to operate on: the explicit arg if given, otherwise the
/// enclosing project's src/main.rk; then run `f` on it.
fn with_entry(file: Option<PathBuf>, f: impl FnOnce(&std::path::Path) -> ExitCode) -> ExitCode {
    match rikki::resolve_entry(file) {
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
            root.join("rikki.toml"),
            format!(
                "[project]\nname = {name:?}\npython = {:?}\n",
                rikki::bridge::embedded_python()
            ),
        )?;
        std::fs::write(
            root.join("src").join("main.rk"),
            "fn main() {\n    print(\"hello, rikki\")\n}\n",
        )?;
        std::fs::write(root.join(".gitignore"), ".rikki/\n")?;
        Ok(())
    };
    match make() {
        Ok(()) => {
            println!("created {name}/ (rikki.toml, src/main.rk)");
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
    let Some(root) = rikki::project::Project::find(&cwd) else {
        eprintln!("error: no rikki.toml found; run: rikki new <name>");
        return ExitCode::FAILURE;
    };
    let mut proj = match rikki::project::Project::load(&root) {
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
