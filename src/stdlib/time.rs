//! Clocks, sleeping, and civil time (spec 15.8). One time currency:
//! int nanoseconds. Sleep is ctx-aware: it wakes promptly when the ctx
//! ends and returns the ctx error, which is what lets poll loops die
//! on Ctrl-C without threads.

use crate::interp::{Fault, Interp};
use crate::value::Value;

pub const CONSTANTS: &[(&str, i64)] = &[
    ("nanosecond", 1),
    ("microsecond", 1_000),
    ("millisecond", 1_000_000),
    ("second", 1_000_000_000),
    ("minute", 60 * 1_000_000_000),
    ("hour", 3_600 * 1_000_000_000),
];

const PARTS_FIELDS: &[&str] = &["year", "month", "day", "hour", "minute", "second"];

pub(crate) fn struct_types() -> Vec<(String, Vec<(String, crate::types::Type)>)> {
    vec![(
        "Parts".into(),
        PARTS_FIELDS
            .iter()
            .map(|f| (f.to_string(), crate::types::Type::Int))
            .collect(),
    )]
}

pub(crate) fn struct_exprs() -> Vec<(String, Vec<(String, crate::ast::TypeExpr)>)> {
    vec![(
        "Parts".into(),
        PARTS_FIELDS
            .iter()
            .map(|f| (f.to_string(), crate::ast::TypeExpr::Named("int".into())))
            .collect(),
    )]
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::sync::OnceLock;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use chrono::{Datelike, LocalResult, TimeZone, Timelike};
    use indexmap::IndexMap;

    use crate::interp::{Fault, Interp};
    use crate::stdlib::ctx::CtxInner;
    use crate::value::Value;

    pub fn now(interp: &Interp) -> Result<i64, Fault> {
        let d = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| interp.fault("time.now: system clock is before the epoch"))?;
        i64::try_from(d.as_nanos()).map_err(|_| interp.fault("time.now: clock out of range"))
    }

    pub fn clock() -> i64 {
        static ORIGIN: OnceLock<Instant> = OnceLock::new();
        // saturate rather than overflow; a process alive 292 years has
        // other problems
        i64::try_from(ORIGIN.get_or_init(Instant::now).elapsed().as_nanos()).unwrap_or(i64::MAX)
    }

    /// Sleep in slices, checking the ctx between them, so SIGINT and
    /// deadlines wake it without threads. None on a full sleep; the ctx
    /// error on early wake.
    pub fn sleep(c: &CtxInner, d: i64) -> Value {
        let deadline = Instant::now() + Duration::from_nanos(d.max(0) as u64);
        loop {
            if let Some(e) = c.err() {
                return Value::Err(e);
            }
            let now = Instant::now();
            if now >= deadline {
                return Value::NoneV;
            }
            std::thread::sleep((deadline - now).min(Duration::from_millis(50)));
        }
    }

    pub fn parts(interp: &Interp, epoch: i64) -> Result<Value, Fault> {
        let secs = epoch.div_euclid(1_000_000_000);
        let sub = epoch.rem_euclid(1_000_000_000) as u32;
        let dt = match chrono::Local.timestamp_opt(secs, sub) {
            LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => dt,
            LocalResult::None => {
                return Err(interp.fault("time.parts: epoch outside the civil range"))
            }
        };
        let mut fields = IndexMap::new();
        for (name, v) in [
            ("year", i64::from(dt.year())),
            ("month", i64::from(dt.month())),
            ("day", i64::from(dt.day())),
            ("hour", i64::from(dt.hour())),
            ("minute", i64::from(dt.minute())),
            ("second", i64::from(dt.second())),
        ] {
            fields.insert(name.to_string(), Value::Int(v));
        }
        Ok(Value::Struct {
            name: "Parts".into(),
            fields,
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn call(interp: &mut Interp, name: &str, args: Vec<Value>) -> Result<Value, Fault> {
    let v = match (name, args.as_slice()) {
        ("now", []) => Value::Int(native::now(interp)?),
        ("clock", []) => Value::Int(native::clock()),
        ("sleep", [Value::Ctx(c), Value::Int(d)]) => native::sleep(c, *d),
        ("parts", [Value::Int(epoch)]) => native::parts(interp, *epoch)?,
        _ => return Err(interp.fault(format!("time.{name}: bad arguments"))),
    };
    Ok(v)
}

/// No usable clock in the browser; the constants work, the clocks report
/// their absence, the ctx.timeout contract (15.4).
#[cfg(target_arch = "wasm32")]
pub fn call(interp: &mut Interp, name: &str, _args: Vec<Value>) -> Result<Value, Fault> {
    Err(interp.fault(format!("time.{name} is not available in this build")))
}

pub fn constant(name: &str) -> Option<Value> {
    CONSTANTS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, v)| Value::Int(*v))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::stdlib::ctx::CtxInner;

    #[test]
    fn constants_relate_exactly() {
        let get = |n| constant(n).unwrap();
        let int = |v: Value| match v {
            Value::Int(i) => i,
            _ => panic!(),
        };
        assert_eq!(int(get("second")) / int(get("millisecond")), 1000);
        assert_eq!(int(get("hour")) / int(get("minute")), 60);
        assert_eq!(int(get("minute")) / int(get("second")), 60);
    }

    #[test]
    fn expired_ctx_wakes_sleep_immediately() {
        let c = CtxInner {
            deadline: Some(std::time::Instant::now()),
            interrupted: None,
        };
        let t0 = std::time::Instant::now();
        let v = native::sleep(&c, 3_600 * 1_000_000_000);
        assert!(t0.elapsed().as_millis() < 200, "must not sleep the hour");
        let Value::Err(e) = v else {
            panic!("expected the ctx error")
        };
        assert!(e.msg.contains("deadline"));
    }

    #[test]
    fn negative_duration_sleeps_zero() {
        let c = CtxInner {
            deadline: None,
            interrupted: None,
        };
        let v = native::sleep(&c, -5);
        assert!(matches!(v, Value::NoneV));
    }

    #[test]
    fn epoch_zero_is_the_seventies() {
        let prog = crate::ast::Program::default();
        let i = crate::interp::Interp::new(&prog);
        let Value::Struct { fields, .. } = native::parts(&i, 0).unwrap() else {
            panic!()
        };
        let Value::Int(y) = fields["year"] else { panic!() };
        assert!((1969..=1970).contains(&y), "epoch zero local year: {y}");
    }
}
