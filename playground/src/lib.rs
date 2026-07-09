//! wasm-bindgen surface for the playground: run a snippet, get back
//! stdout and whatever went wrong, typed by phase.

use serde::Serialize;
use wasm_bindgen::prelude::*;

#[derive(Serialize)]
struct RunOutput {
    stdout: String,
    /// "ok" | "compile" | "runtime"
    status: &'static str,
    error: String,
}

#[wasm_bindgen]
pub fn run(source: &str) -> JsValue {
    let r = rikki::run_snippet(source);
    let (status, error) = match r.exit {
        rikki::ExitKind::Ok => ("ok", String::new()),
        rikki::ExitKind::CompileError(m) => ("compile", m),
        rikki::ExitKind::RuntimeError(m) => ("runtime", m),
    };
    let out = RunOutput {
        stdout: r.stdout,
        status,
        error,
    };
    serde_wasm_bindgen::to_value(&out).unwrap_or(JsValue::NULL)
}

#[wasm_bindgen]
pub fn version() -> String {
    // the interpreter crate's version, not this shim's
    rikki::PKG_VERSION.to_string()
}
