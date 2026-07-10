//! The test module: helpers that return `error?`, built to sit behind
//! `check` (book: testing.md; spec 15.6). Failures are ordinary error
//! values with origins; nothing here faults on a failed expectation.

use crate::interp::{Fault, Interp};
use crate::value::{render, ErrVal, Value};

/// The sentinel pytype the runner reports as skipped rather than failed.
pub const SKIP_MARKER: &str = "test.skip";

pub fn call(interp: &mut Interp, name: &str, args: Vec<Value>) -> Result<Value, Fault> {
    let fail = |interp: &Interp, msg: String| {
        Value::Err(ErrVal {
            msg,
            origin: interp.origin(),
            ..Default::default()
        })
    };
    match (name, args.as_slice()) {
        ("eq", [got, want]) => match got.eq_value(want, 0) {
            Some(true) => Ok(Value::NoneV),
            Some(false) => Ok(fail(
                interp,
                format!("expected {}, got {}", render(want), render(got)),
            )),
            None => Err(interp.fault("value too deep or cyclic")),
        },
        ("neq", [got, unwanted]) => match got.eq_value(unwanted, 0) {
            Some(false) => Ok(Value::NoneV),
            Some(true) => Ok(fail(interp, format!("both sides equal {}", render(got)))),
            None => Err(interp.fault("value too deep or cyclic")),
        },
        ("err", [v]) => match v {
            Value::Err(_) => Ok(Value::NoneV),
            Value::NoneV => Ok(fail(interp, "expected an error, got none".into())),
            _ => Err(interp.fault("test.err needs an error?")),
        },
        ("skip", [Value::Str(reason)]) => Ok(Value::Err(ErrVal {
            msg: reason.clone(),
            pytype: SKIP_MARKER.into(),
            origin: interp.origin(),
            ..Default::default()
        })),
        _ => Err(interp.fault(format!("test.{name}: bad arguments"))),
    }
}
