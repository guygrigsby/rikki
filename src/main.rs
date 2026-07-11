use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "nevla", version, about = "the nevla language")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Typecheck and run a program (defaults to the project's src/main.nv)
    Run {
        file: Option<PathBuf>,
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Typecheck only (defaults to the project's src/main.nv)
    Check { file: Option<PathBuf> },
    /// Run Test functions in *_test.nv files (defaults to the project)
    Test {
        paths: Vec<PathBuf>,
        /// Parallel test workers (default: one per core; 1 serializes)
        #[arg(short, long)]
        jobs: Option<usize>,
    },
    /// Rewrite source in the one true style (defaults to the project's src/)
    Fmt {
        paths: Vec<PathBuf>,
        /// List unformatted files and exit nonzero instead of rewriting
        #[arg(long)]
        check: bool,
    },
    /// Scaffold a new project
    New {
        name: String,
        /// Also install a Claude Code hook that typechecks after every edit
        #[arg(long)]
        claude_hook: bool,
    },
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
    Add {
        package: String,
        /// Import name this package satisfies when the two differ
        /// (mlflow-skinny provides mlflow)
        #[arg(long)]
        module: Option<String>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Run { file, args } => with_entry(file, |f| {
            nevla::report(nevla::run_with(f, args.clone(), true))
        }),
        Cmd::Check { file } => with_entry(file, |f| nevla::report(nevla::check_source(f))),
        Cmd::Fmt { paths, check } => fmt_cmd(paths, check),
        Cmd::Test { paths, jobs } => test_cmd(paths, jobs.unwrap_or(0)),
        Cmd::Py {
            cmd: PyCmd::Add { package, module },
        } => py_add(&package, module.as_deref()),
        Cmd::New { name, claude_hook } => new_project(&name, claude_hook),
        Cmd::Repl => {
            nevla::repl::run();
            ExitCode::SUCCESS
        }
    }
}

/// Resolve the file to operate on: the explicit arg if given, otherwise the
/// enclosing project's src/main.nv; then run `f` on it.
fn with_entry(file: Option<PathBuf>, f: impl FnOnce(&std::path::Path) -> ExitCode) -> ExitCode {
    match nevla::resolve_entry(file) {
        Ok(path) => f(&path),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn new_project(name: &str, claude_hook: bool) -> ExitCode {
    let root = std::path::Path::new(name);
    if root.exists() {
        eprintln!("error: {name} already exists");
        return ExitCode::FAILURE;
    }
    let make = || -> std::io::Result<()> {
        std::fs::create_dir_all(root.join("src"))?;
        std::fs::write(
            root.join("nevla.toml"),
            format!(
                "[project]\nname = {name:?}\npython = {:?}\nnevla = {:?}\n",
                nevla::bridge::embedded_python(),
                nevla::PKG_VERSION
            ),
        )?;
        std::fs::write(
            root.join("src").join("main.nv"),
            "fn main() {\n    print(\"hello, nevla\")\n}\n",
        )?;
        std::fs::write(root.join(".gitignore"), ".nevla/\n")?;
        // agents writing nevla: the primer, loaded by anything that reads
        // AGENTS.md or CLAUDE.md; the executable check-after-every-edit
        // hook only lands when asked for
        std::fs::write(root.join("AGENTS.md"), PRIMER)?;
        std::fs::write(root.join("CLAUDE.md"), "@AGENTS.md\n")?;
        if claude_hook {
            std::fs::create_dir_all(root.join(".claude/hooks"))?;
            std::fs::write(root.join(".claude/settings.json"), HOOK_SETTINGS)?;
            std::fs::write(root.join(".claude/hooks/nevla-check.nv"), HOOK_CHECK)?;
        }
        Ok(())
    };
    match make() {
        Ok(()) => {
            if claude_hook {
                println!("created {name}/ (nevla.toml, src/main.nv, AGENTS.md, .claude/)");
            } else {
                println!("created {name}/ (nevla.toml, src/main.nv, AGENTS.md)");
                println!(
                    "tip: --claude-hook adds a Claude Code hook that typechecks after every edit"
                );
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The agent primer, kept in the repo as the single source and baked into
/// the binary so `nevla new` can scaffold it.
const PRIMER: &str = include_str!("../docs/nevla-primer.md");

const HOOK_SETTINGS: &str = r#"{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Write|Edit",
        "hooks": [
          {
            "type": "command",
            "command": "nv \"$CLAUDE_PROJECT_DIR/.claude/hooks/nevla-check.nv\""
          }
        ]
      }
    ]
  }
}
"#;

const HOOK_CHECK: &str = r#"// PostToolUse hook: typecheck after every .nv edit, feed diagnostics back.
import "file"
import py "sys"
import py "json"
import py "os"
import py "subprocess"

fn main() (error?) {
    raw := check str(sys.stdin.read())
    d := check json.loads(raw)
    ti, tierr := d["tool_input"]
    if tierr != none {
        return none
    }
    pathv, perr := ti["file_path"]
    if perr != none {
        return none
    }
    path := check str(pathv)
    if !path.has_suffix(".nv") {
        return none
    }
    // inside a project, check the whole program from its entrypoint; a
    // lone module file has no main and cannot be checked standalone
    workdir := check str(os.path.dirname(os.path.abspath(path)))
    cmd := ["nevla", "check", path]
    probe := workdir
    for {
        if file.exists(probe + "/nevla.toml") {
            cmd = ["nevla", "check"]
            break
        }
        parent := check str(os.path.dirname(probe))
        if parent == probe {
            break
        }
        probe = parent
    }
    // no nevla on PATH or a wedged check: stay out of the way
    r, rerr := subprocess.run(cmd, cwd: workdir, capture_output: true, text: true, timeout: 30)
    if rerr != none {
        return none
    }
    rc := check int(r.returncode)
    if rc != 0 {
        check sys.stderr.write(check str(r.stdout) + check str(r.stderr))
        check sys.stderr.flush()
        // exit 2 returns stderr to the agent as blocking feedback
        check os._exit(2)
    }
    return none
}
"#;

fn test_cmd(paths: Vec<PathBuf>, jobs: usize) -> ExitCode {
    let mut files = vec![];
    if paths.is_empty() {
        let cwd = std::env::current_dir().unwrap_or_default();
        let Some(root) = nevla::project::Project::find(&cwd) else {
            eprintln!("error: no paths given and no nevla project found");
            return ExitCode::FAILURE;
        };
        collect_rk(&root.join("src"), &mut files);
        files.retain(|f| {
            f.file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with("_test.nv"))
        });
    } else {
        for p in paths {
            if p.is_dir() {
                let mut all = vec![];
                collect_rk(&p, &mut all);
                all.retain(|f| {
                    f.file_name()
                        .is_some_and(|n| n.to_string_lossy().ends_with("_test.nv"))
                });
                files.extend(all);
            } else {
                files.push(p);
            }
        }
    }
    files.sort();
    if files.is_empty() {
        eprintln!("no *_test.nv files found");
        return ExitCode::FAILURE;
    }
    let (mut passed, mut failed, mut skipped) = (0u32, 0u32, 0u32);
    for f in &files {
        let short = f
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| f.display().to_string());
        match nevla::run_test_file(f, jobs) {
            Err(e) => {
                println!("FAIL {short}");
                println!("     {e}");
                failed += 1;
            }
            Ok(outcomes) => {
                for o in outcomes {
                    match o.status {
                        nevla::TestStatus::Pass => {
                            passed += 1;
                            println!("ok   {short}  {}", o.name);
                        }
                        nevla::TestStatus::Skip => {
                            skipped += 1;
                            println!("skip {short}  {}  ({})", o.name, o.message);
                        }
                        nevla::TestStatus::Fail => {
                            failed += 1;
                            println!("FAIL {short}  {}", o.name);
                            for line in o.message.lines() {
                                println!("     {line}");
                            }
                            for line in o.stdout.lines() {
                                println!("     | {line}");
                            }
                        }
                    }
                }
            }
        }
    }
    let mut summary = format!("{passed} passed, {failed} failed");
    if skipped > 0 {
        summary.push_str(&format!(", {skipped} skipped"));
    }
    println!("{summary}");
    if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn fmt_cmd(paths: Vec<PathBuf>, check: bool) -> ExitCode {
    let mut files = vec![];
    if paths.is_empty() {
        let cwd = std::env::current_dir().unwrap_or_default();
        let Some(root) = nevla::project::Project::find(&cwd) else {
            eprintln!("error: no paths given and no nevla project found");
            return ExitCode::FAILURE;
        };
        collect_rk(&root.join("src"), &mut files);
    } else {
        for p in paths {
            if p.is_dir() {
                collect_rk(&p, &mut files);
            } else {
                files.push(p);
            }
        }
    }
    files.sort();
    let mut unformatted = vec![];
    let mut failed = false;
    for f in &files {
        let src = match std::fs::read_to_string(f) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: {}: {e}", f.display());
                failed = true;
                continue;
            }
        };
        let formatted = match nevla::format::fmt_source(&src) {
            Ok(s) => s,
            Err(d) => {
                // never rewrite what we cannot parse
                eprintln!("error: {}:{}", f.display(), d);
                failed = true;
                continue;
            }
        };
        if formatted == src {
            continue;
        }
        if check {
            println!("{}", f.display());
            unformatted.push(f);
        } else if let Err(e) = std::fs::write(f, formatted) {
            eprintln!("error: {}: {e}", f.display());
            failed = true;
        }
    }
    if failed || (check && !unformatted.is_empty()) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn collect_rk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_rk(&p, out);
        } else if p.extension().is_some_and(|x| x == "nv") {
            out.push(p);
        }
    }
}

fn py_add(package: &str, module: Option<&str>) -> ExitCode {
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: cannot read cwd: {e}");
            return ExitCode::FAILURE;
        }
    };
    let Some(root) = nevla::project::Project::find(&cwd) else {
        eprintln!("error: no nevla.toml found; run: nevla new <name>");
        return ExitCode::FAILURE;
    };
    let mut proj = match nevla::project::Project::load(&root) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    match proj.py_add(package, module, "uv") {
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
