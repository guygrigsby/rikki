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
        Cmd::Run { file, args } => with_entry(file, |f| {
            rikki::report(rikki::run_with(f, args.clone(), true))
        }),
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
        // agents writing rikki: the primer, loaded by anything that reads
        // AGENTS.md or CLAUDE.md, plus a check-after-every-edit hook so the
        // checker's diagnostics land in the agent loop
        std::fs::write(root.join("AGENTS.md"), PRIMER)?;
        std::fs::write(root.join("CLAUDE.md"), "@AGENTS.md\n")?;
        std::fs::create_dir_all(root.join(".claude/hooks"))?;
        std::fs::write(root.join(".claude/settings.json"), HOOK_SETTINGS)?;
        std::fs::write(root.join(".claude/hooks/rikki-check.py"), HOOK_CHECK)?;
        Ok(())
    };
    match make() {
        Ok(()) => {
            println!("created {name}/ (rikki.toml, src/main.rk, AGENTS.md, .claude/)");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The agent primer, kept in the repo as the single source and baked into
/// the binary so `rikki new` can scaffold it.
const PRIMER: &str = include_str!("../docs/rikki-primer.md");

const HOOK_SETTINGS: &str = r#"{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Write|Edit",
        "hooks": [
          {
            "type": "command",
            "command": "python3 \"$CLAUDE_PROJECT_DIR/.claude/hooks/rikki-check.py\""
          }
        ]
      }
    ]
  }
}
"#;

const HOOK_CHECK: &str = r#"#!/usr/bin/env python3
"""PostToolUse hook: typecheck after every .rk edit, feed diagnostics back."""
import json
import os
import subprocess
import sys

d = json.load(sys.stdin)
path = d.get("tool_input", {}).get("file_path", "")
if not path.endswith(".rk"):
    sys.exit(0)
# inside a project, check the whole program from its entrypoint; a lone
# module file has no main and cannot be checked standalone
workdir = os.path.dirname(os.path.abspath(path))
cmd = ["rikki", "check", path]
probe = workdir
while True:
    if os.path.exists(os.path.join(probe, "rikki.toml")):
        cmd = ["rikki", "check"]
        break
    parent = os.path.dirname(probe)
    if parent == probe:
        break
    probe = parent
try:
    r = subprocess.run(cmd, cwd=workdir, capture_output=True, text=True, timeout=30)
except (FileNotFoundError, subprocess.TimeoutExpired):
    sys.exit(0)  # no rikki on PATH or a wedged check: stay out of the way
if r.returncode != 0:
    sys.stderr.write(r.stdout + r.stderr)
    sys.exit(2)  # exit 2 returns stderr to the agent as feedback
"#;

fn py_add(package: &str) -> ExitCode {
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: cannot read cwd: {e}");
            return ExitCode::FAILURE;
        }
    };
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
