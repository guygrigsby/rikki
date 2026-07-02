use std::{fs, path::Path, path::PathBuf};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    // a directory with a main.mg is one multi-file case; siblings are modules
    let main = dir.join("main.mg");
    if main.exists() {
        out.push(main);
        return;
    }
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "mg") {
            out.push(p);
        }
    }
}

fn pending(root: &Path) -> Vec<String> {
    let f = root.join("PENDING");
    if !f.exists() {
        return vec![];
    }
    fs::read_to_string(f)
        .unwrap()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

#[test]
fn golden() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let skip = pending(&root);
    let mut cases = vec![];
    collect(&root, &mut cases);
    cases.sort();
    let mut failures = vec![];
    let mut ran = 0;
    for mg in &cases {
        let rel = mg.strip_prefix(&root).unwrap().to_string_lossy().to_string();
        if skip.contains(&rel) {
            continue;
        }
        if rel.starts_with("py/") && std::env::var("MONGOOSE_TEST_PY").is_err() {
            continue;
        }
        ran += 1;
        let res = mongoose::run_source(mg);
        let out_f = mg.with_extension("out");
        let err_f = mg.with_extension("err");
        let ok = if out_f.exists() {
            let want = fs::read_to_string(&out_f).unwrap();
            matches!(res.exit, mongoose::ExitKind::Ok) && res.stdout == want
        } else if err_f.exists() {
            let want = fs::read_to_string(&err_f).unwrap();
            let got = match &res.exit {
                mongoose::ExitKind::CompileError(m) | mongoose::ExitKind::RuntimeError(m) => {
                    m.clone()
                }
                mongoose::ExitKind::Ok => String::new(),
            };
            want.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .all(|l| got.contains(l))
        } else {
            failures.push(format!("{rel}: no .out or .err file"));
            continue;
        };
        if !ok {
            failures.push(format!(
                "{rel}\n  exit: {:?}\n  stdout: {:?}",
                res.exit, res.stdout
            ));
        }
    }
    assert!(ran > 0 || !skip.is_empty(), "no golden cases found");
    assert!(
        failures.is_empty(),
        "golden failures:\n{}",
        failures.join("\n")
    );
}
