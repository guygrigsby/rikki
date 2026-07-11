pub mod ctx;
pub mod file;
pub mod flag;
pub mod gpu;
pub mod http;
pub mod math;
pub mod os;
pub mod regex;
pub mod test;
pub mod time;

use crate::interp::{Fault, Interp};
use crate::value::Value;

pub fn call(
    interp: &mut Interp,
    module: &str,
    name: &str,
    args: Vec<Value>,
) -> Result<Value, Fault> {
    match module {
        "math" => math::call(interp, name, args),
        "file" => file::call(interp, name, args),
        "ctx" => ctx::call(interp, name, args),
        "gpu" => gpu::call(interp, name, args),
        "http" => http::call(interp, name, args),
        "test" => test::call(interp, name, args),
        "time" => time::call(interp, name, args),
        "os" => os::call(interp, name, args),
        "regex" => regex::call(interp, name, args),
        "flag" => flag::call(interp, name, args),
        _ => Err(interp.fault(format!("{module}.{name} is not implemented yet"))),
    }
}

pub fn constant(interp: &Interp, module: &str, name: &str) -> Result<Value, Fault> {
    match (module, name) {
        ("math", "pi") => Ok(Value::Float(std::f64::consts::PI)),
        ("math", "e") => Ok(Value::Float(std::f64::consts::E)),
        ("time", _) => time::constant(name)
            .ok_or_else(|| interp.fault(format!("{module}.{name} is not implemented yet"))),
        _ => Err(interp.fault(format!("{module}.{name} is not implemented yet"))),
    }
}
