use crate::interp::{Fault, Interp};
use crate::value::Value;

pub fn call(interp: &mut Interp, name: &str, args: Vec<Value>) -> Result<Value, Fault> {
    use Value::{Float, Int};
    let v = match (name, args.as_slice()) {
        ("abs", [Int(i)]) => Int(i
            .checked_abs()
            .ok_or_else(|| interp.fault("integer overflow"))?),
        ("abs", [Float(f)]) => Float(f.abs()),
        ("min", [Int(a), Int(b)]) => Int(*a.min(b)),
        ("min", [Float(a), Float(b)]) => Float(a.min(*b)),
        ("max", [Int(a), Int(b)]) => Int(*a.max(b)),
        ("max", [Float(a), Float(b)]) => Float(a.max(*b)),
        ("sqrt", [Float(f)]) => Float(f.sqrt()),
        ("cos", [Float(f)]) => Float(f.cos()),
        ("sin", [Float(f)]) => Float(f.sin()),
        ("tan", [Float(f)]) => Float(f.tan()),
        ("pow", [Float(a), Float(b)]) => Float(a.powf(*b)),
        ("exp", [Float(f)]) => Float(f.exp()),
        ("ln", [Float(f)]) => Float(f.ln()),
        ("log", [Float(base), Float(num)]) => Float(num.log(*base)),
        ("floor", [Float(f)]) => Int(f.floor() as i64),
        ("ceil", [Float(f)]) => Int(f.ceil() as i64),
        // half-away-from-zero, like Go and Python's round-half-even is NOT
        ("round", [Float(f)]) => Int(f.round() as i64),
        _ => return Err(interp.fault(format!("math.{name}: bad arguments"))),
    };
    Ok(v)
}
