//! `mg` is the runner (python parity): `mg file.mg` runs a program,
//! bare `mg` starts the REPL. Toolchain work lives in `mongoose`.
use std::process::ExitCode;

fn main() -> ExitCode {
    match std::env::args_os().nth(1) {
        Some(file) => mongoose::report(mongoose::run_source(std::path::Path::new(&file))),
        None => {
            mongoose::repl::run();
            ExitCode::SUCCESS
        }
    }
}
