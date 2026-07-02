//! `tk` is the runner (python parity): `tk file.rk` runs a program,
//! bare `tk` starts the REPL. Toolchain work lives in `rikki`.
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut argv = std::env::args_os().skip(1);
    match argv.next() {
        Some(file) => {
            let args: Vec<String> = argv.map(|a| a.to_string_lossy().to_string()).collect();
            rikki::report(rikki::run_with(std::path::Path::new(&file), args, true))
        }
        None => {
            rikki::repl::run();
            ExitCode::SUCCESS
        }
    }
}
