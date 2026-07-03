use std::fs;

use crate::interp::{Fault, Interp};
use crate::value::{ErrVal, Value};

fn err(msg: String) -> Value {
    Value::Err(ErrVal {
        msg,
        ..Default::default()
    })
}

fn fallible(v: Result<Value, String>, zero: Value) -> Value {
    match v {
        Ok(val) => Value::Tuple(vec![val, Value::NoneV]),
        Err(m) => Value::Tuple(vec![zero, err(m)]),
    }
}

pub fn call(interp: &mut Interp, name: &str, args: Vec<Value>) -> Result<Value, Fault> {
    let mut strs = vec![];
    for a in &args {
        match a {
            Value::Str(s) => strs.push(s.clone()),
            _ => return Err(interp.fault(format!("file.{name}: bad arguments"))),
        }
    }
    let v = match (name, strs.as_slice()) {
        ("read", [path]) => fallible(
            fs::read_to_string(path)
                .map(Value::Str)
                .map_err(|e| format!("read {path}: {e}")),
            Value::Str(String::new()),
        ),
        ("write", [path, s]) => match fs::write(path, s) {
            Ok(()) => Value::NoneV,
            Err(e) => err(format!("write {path}: {e}")),
        },
        ("append", [path, s]) => {
            use std::io::Write;
            let r = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .and_then(|mut f| f.write_all(s.as_bytes()));
            match r {
                Ok(()) => Value::NoneV,
                Err(e) => err(format!("append {path}: {e}")),
            }
        }
        ("exists", [path]) => Value::Bool(fs::exists(path).unwrap_or(false)),
        ("list", [dir]) => fallible(
            fs::read_dir(dir)
                .map_err(|e| format!("list {dir}: {e}"))
                .map(|rd| {
                    let mut names: Vec<String> = rd
                        .filter_map(|e| e.ok())
                        .map(|e| e.file_name().to_string_lossy().to_string())
                        .collect();
                    names.sort();
                    Value::list(names.into_iter().map(Value::Str).collect())
                }),
            Value::list(vec![]),
        ),
        ("remove", [path]) => {
            let r = if fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false) {
                fs::remove_dir(path)
            } else {
                fs::remove_file(path)
            };
            match r {
                Ok(()) => Value::NoneV,
                Err(e) => err(format!("remove {path}: {e}")),
            }
        }
        ("mkdir", [path]) => match fs::create_dir_all(path) {
            Ok(()) => Value::NoneV,
            Err(e) => err(format!("mkdir {path}: {e}")),
        },
        _ => return Err(interp.fault(format!("file.{name}: bad arguments"))),
    };
    Ok(v)
}

#[cfg(test)]
mod tests {
    use crate::ast::Program;
    use crate::interp::Interp;
    use crate::value::Value;

    fn tempbase(tag: &str) -> String {
        let dir =
            std::env::temp_dir().join(format!("rikki-file-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().to_string()
    }

    fn call(name: &str, args: Vec<Value>) -> Value {
        let prog = Program::default();
        let mut interp = Interp::new(&prog);
        super::call(&mut interp, name, args)
            .map_err(|f| f.msg)
            .unwrap()
    }

    fn s(v: &str) -> Value {
        Value::Str(v.into())
    }

    #[test]
    fn write_read_append_roundtrip() {
        let base = tempbase("rw");
        let p = format!("{base}/f.txt");
        assert!(matches!(
            call("write", vec![s(&p), s("one\n")]),
            Value::NoneV
        ));
        assert!(matches!(
            call("append", vec![s(&p), s("two\n")]),
            Value::NoneV
        ));
        match call("read", vec![s(&p)]) {
            Value::Tuple(ts) => {
                assert!(matches!(&ts[0], Value::Str(x) if x == "one\ntwo\n"));
                assert!(matches!(ts[1], Value::NoneV));
            }
            v => panic!("{v:?}"),
        }
    }

    #[test]
    fn exists_list_remove_mkdir() {
        let base = tempbase("misc");
        let sub = format!("{base}/a/b");
        assert!(matches!(call("mkdir", vec![s(&sub)]), Value::NoneV));
        assert!(matches!(call("exists", vec![s(&sub)]), Value::Bool(true)));
        call("write", vec![s(&format!("{base}/z.txt")), s("x")]);
        match call("list", vec![s(&base)]) {
            Value::Tuple(ts) => match &ts[0] {
                Value::List(items) => {
                    let names: Vec<String> = items
                        .borrow()
                        .iter()
                        .map(|v| match v {
                            Value::Str(x) => x.clone(),
                            _ => panic!(),
                        })
                        .collect();
                    assert_eq!(names, vec!["a", "z.txt"]);
                }
                v => panic!("{v:?}"),
            },
            v => panic!("{v:?}"),
        }
        assert!(matches!(
            call("remove", vec![s(&format!("{base}/z.txt"))]),
            Value::NoneV
        ));
        assert!(matches!(
            call("exists", vec![s(&format!("{base}/z.txt"))]),
            Value::Bool(false)
        ));
        // removing a dir with contents fails as an error value, not a fault
        assert!(matches!(
            call("remove", vec![s(&format!("{base}/a"))]),
            Value::Err(_)
        ));
    }

    #[test]
    fn read_missing_is_error_value() {
        match call("read", vec![s("/nonexistent/xyz")]) {
            Value::Tuple(ts) => match &ts[1] {
                Value::Err(e) => assert!(e.msg.contains("nonexistent")),
                v => panic!("{v:?}"),
            },
            v => panic!("{v:?}"),
        }
    }
}
