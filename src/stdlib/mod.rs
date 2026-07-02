pub mod math;

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
        _ => Err(interp.fault(format!("{module}.{name} is not implemented yet"))),
    }
}

pub fn constant(interp: &mut Interp, module: &str, name: &str) -> Result<Value, Fault> {
    match (module, name) {
        ("math", "pi") => Ok(Value::Float(std::f64::consts::PI)),
        ("math", "e") => Ok(Value::Float(std::f64::consts::E)),
        _ => Err(interp.fault(format!("{module}.{name} is not implemented yet"))),
    }
}
