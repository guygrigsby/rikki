use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use crate::interp::{Fault, Interp};
use crate::value::{ErrVal, Value};

#[derive(Debug)]
pub struct CtxInner {
    pub deadline: Option<Instant>,
    pub interrupted: Option<Arc<AtomicBool>>,
}

impl CtxInner {
    pub fn err(&self) -> Option<ErrVal> {
        if let Some(flag) = &self.interrupted {
            if flag.load(Ordering::SeqCst) {
                return Some(ErrVal {
                    msg: "interrupted".into(),
                    ..Default::default()
                });
            }
        }
        if let Some(d) = self.deadline {
            if Instant::now() >= d {
                return Some(ErrVal {
                    msg: "deadline exceeded".into(),
                    ..Default::default()
                });
            }
        }
        None
    }

    /// Remaining time budget, if a deadline is set.
    pub fn remaining(&self) -> Option<Duration> {
        self.deadline
            .map(|d| d.saturating_duration_since(Instant::now()))
    }
}

static SIGINT: OnceLock<Arc<AtomicBool>> = OnceLock::new();

fn sigint_flag() -> Arc<AtomicBool> {
    SIGINT
        .get_or_init(|| {
            let flag = Arc::new(AtomicBool::new(false));
            let f = Arc::clone(&flag);
            // ignore failure: a second handler registration (tests) is fine
            let _ = ctrlc::set_handler(move || f.store(true, Ordering::SeqCst));
            flag
        })
        .clone()
}

fn parent(
    interp: &Interp,
    v: Option<&Value>,
) -> Result<(Option<Instant>, Option<Arc<AtomicBool>>), Fault> {
    match v {
        Some(Value::Ctx(c)) => Ok((c.deadline, c.interrupted.clone())),
        _ => Err(interp.fault("ctx: bad arguments")),
    }
}

pub fn call(interp: &mut Interp, name: &str, args: Vec<Value>) -> Result<Value, Fault> {
    let v = match (name, args.as_slice()) {
        ("background", []) => Value::Ctx(Arc::new(CtxInner {
            deadline: None,
            interrupted: None,
        })),
        ("timeout", [p, Value::Float(secs)]) => {
            let (deadline, interrupted) = parent(interp, Some(p))?;
            // from_secs_f64 panics on inf or seconds past u64::MAX, and
            // Instant addition can overflow; both are expressible from
            // user source (1.0 / 0.0 is inf), so both fault.
            let out_of_range = || interp.fault("ctx.timeout: seconds out of range");
            let d = Duration::try_from_secs_f64(secs.max(0.0)).map_err(|_| out_of_range())?;
            let new = Instant::now().checked_add(d).ok_or_else(out_of_range)?;
            let deadline = Some(deadline.map_or(new, |d| d.min(new)));
            Value::Ctx(Arc::new(CtxInner {
                deadline,
                interrupted,
            }))
        }
        ("interrupt", [p]) => {
            let (deadline, _) = parent(interp, Some(p))?;
            Value::Ctx(Arc::new(CtxInner {
                deadline,
                interrupted: Some(sigint_flag()),
            }))
        }
        _ => return Err(interp.fault(format!("ctx.{name}: bad arguments"))),
    };
    Ok(v)
}

pub fn method(interp: &mut Interp, c: &CtxInner, name: &str) -> Result<Value, Fault> {
    match name {
        "done" => Ok(Value::Bool(c.err().is_some())),
        "err" => Ok(match c.err() {
            Some(e) => Value::Err(e),
            None => Value::NoneV,
        }),
        _ => Err(interp.fault(format!("Ctx has no method {name}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Program;

    #[test]
    fn background_is_never_done() {
        let prog = Program::default();
        let mut i = Interp::new(&prog);
        let bg = call(&mut i, "background", vec![])
            .map_err(|f| f.msg)
            .unwrap();
        let Value::Ctx(c) = &bg else { panic!() };
        assert!(c.err().is_none());
    }

    #[test]
    fn zero_timeout_is_immediately_done() {
        let prog = Program::default();
        let mut i = Interp::new(&prog);
        let bg = call(&mut i, "background", vec![]).unwrap_or_else(|_| panic!());
        let t = call(&mut i, "timeout", vec![bg, Value::Float(0.0)])
            .map_err(|f| f.msg)
            .unwrap();
        let Value::Ctx(c) = &t else { panic!() };
        let e = c.err().expect("expired ctx must report an error");
        assert!(e.msg.contains("deadline"));
    }

    #[test]
    fn child_timeout_cannot_extend_parent() {
        let prog = Program::default();
        let mut i = Interp::new(&prog);
        let bg = call(&mut i, "background", vec![]).unwrap_or_else(|_| panic!());
        let short =
            call(&mut i, "timeout", vec![bg, Value::Float(0.0)]).unwrap_or_else(|_| panic!());
        let long =
            call(&mut i, "timeout", vec![short, Value::Float(100.0)]).unwrap_or_else(|_| panic!());
        let Value::Ctx(c) = &long else { panic!() };
        assert!(c.err().is_some(), "child deadline must clamp to parent");
    }
}
