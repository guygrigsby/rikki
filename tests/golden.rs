use std::{fs, path::Path, path::PathBuf};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    // a directory with a main.rk is one multi-file case; siblings are modules
    let main = dir.join("main.rk");
    if main.exists() {
        out.push(main);
        return;
    }
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "rk") {
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
    for rk in &cases {
        let rel = rk
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .to_string();
        if skip.contains(&rel) {
            continue;
        }
        if rel.starts_with("py/") && std::env::var("RIKKI_TEST_PY").is_err() {
            continue;
        }
        ran += 1;
        let res = rikki::run_source(rk);
        let out_f = rk.with_extension("out");
        let err_f = rk.with_extension("err");
        let ok = if out_f.exists() {
            let want = fs::read_to_string(&out_f).unwrap();
            matches!(res.exit, rikki::ExitKind::Ok) && res.stdout == want
        } else if err_f.exists() {
            let want = fs::read_to_string(&err_f).unwrap();
            let got = match &res.exit {
                rikki::ExitKind::CompileError(m) | rikki::ExitKind::RuntimeError(m) => {
                    m.clone()
                }
                rikki::ExitKind::Ok => String::new(),
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
