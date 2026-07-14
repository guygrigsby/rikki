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
    // Handle readbytes and writebytes before string validation
    match (name, &args[..]) {
        ("readbytes", [Value::Str(path)]) => {
            return Ok(fallible(
                fs::read(path)
                    .map(Value::bytes)
                    .map_err(|e| format!("readbytes {path}: {e}")),
                Value::bytes(vec![]),
            ))
        }
        ("writebytes", [Value::Str(path), Value::Bytes(b)]) => {
            return Ok(match fs::write(path, &b.borrow().data) {
                Ok(()) => Value::NoneV,
                Err(e) => err(format!("writebytes {path}: {e}")),
            })
        }
        _ => {}
    }

    // Validate that all remaining args are strings
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
        // * stays in one directory and skips dotfiles (shell behavior);
        // ** crosses directories; unreadable entries are skipped, also
        // the shell's answer; results sorted
        ("glob", [pattern]) => fallible(
            glob::glob_with(
                pattern,
                glob::MatchOptions {
                    require_literal_leading_dot: true,
                    ..Default::default()
                },
            )
            .map_err(|e| format!("glob {pattern}: {e}"))
            .map(|paths| {
                let mut out: Vec<String> = paths
                    .filter_map(|p| p.ok())
                    .map(|p| p.to_string_lossy().to_string())
                    .collect();
                out.sort();
                Value::list(out.into_iter().map(Value::Str).collect())
            }),
            Value::list(vec![]),
        ),
        ("modified", [path]) => fallible(
            fs::metadata(path)
                .and_then(|m| m.modified())
                .map_err(|e| format!("modified {path}: {e}"))
                .and_then(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .map_err(|_| format!("modified {path}: before the epoch"))
                })
                .and_then(|d| {
                    i64::try_from(d.as_nanos())
                        .map(Value::Int)
                        .map_err(|_| format!("modified {path}: out of range"))
                }),
            Value::Int(0),
        ),
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
            std::env::temp_dir().join(format!("nevla-file-test-{}-{tag}", std::process::id()));
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

    #[test]
    fn readbytes_roundtrips_non_utf8() {
        let base = tempbase("bytes");
        let p = format!("{base}/x.bin");
        std::fs::write(&p, [0u8, 255, 137]).unwrap();
        match call("readbytes", vec![s(&p)]) {
            Value::Tuple(ts) => {
                match &ts[0] {
                    Value::Bytes(b) => {
                        assert_eq!(b.borrow().data, vec![0u8, 255, 137]);
                    }
                    v => panic!("{v:?}"),
                }
                assert!(matches!(ts[1], Value::NoneV));
            }
            v => panic!("{v:?}"),
        }
    }

    #[test]
    fn writebytes_creates_file() {
        let base = tempbase("writebytes");
        let p = format!("{base}/out.bin");
        let data = Value::Bytes(
            std::rc::Rc::new(std::cell::RefCell::new(crate::value::BytesBuf {
                data: vec![1u8, 2, 255, 254],
            })),
        );
        assert!(matches!(call("writebytes", vec![s(&p), data]), Value::NoneV));
        let read_back = std::fs::read(&p).unwrap();
        assert_eq!(read_back, vec![1u8, 2, 255, 254]);
    }

    #[test]
    fn readbytes_missing_is_error_value() {
        match call("readbytes", vec![s("/nonexistent/bin")]) {
            Value::Tuple(ts) => {
                match &ts[0] {
                    Value::Bytes(b) => {
                        assert_eq!(b.borrow().data.len(), 0);
                    }
                    v => panic!("{v:?}"),
                }
                match &ts[1] {
                    Value::Err(e) => assert!(e.msg.contains("nonexistent")),
                    v => panic!("{v:?}"),
                }
            }
            v => panic!("{v:?}"),
        }
    }
}
