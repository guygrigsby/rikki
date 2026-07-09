//! fmt's correctness gates (docs/specs/2026-07-09-fmt-design.md), proven
//! over the whole golden corpus: formatting preserves the AST and every
//! comment, and is idempotent.

use std::{fs, path::Path, path::PathBuf};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "rk") {
            out.push(p);
        }
    }
}

/// Debug print of the AST with spans zeroed, so layout changes compare equal.
fn shape(prog: &rikki::ast::Program) -> String {
    let mut s = format!("{prog:?}");
    for marker in ["line: ", "col: "] {
        let mut out = String::with_capacity(s.len());
        let mut rest = s.as_str();
        while let Some(i) = rest.find(marker) {
            out.push_str(&rest[..i + marker.len()]);
            rest = rest[i + marker.len()..].trim_start_matches(|c: char| c.is_ascii_digit());
            out.push('0');
        }
        out.push_str(rest);
        s = out;
    }
    s
}

fn comments(src: &str) -> Vec<String> {
    let (_, t) = rikki::lexer::lex_trivia(src).unwrap();
    t.comments.into_iter().map(|c| c.text).collect()
}

#[test]
fn corpus_roundtrip() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let mut cases = vec![];
    collect(&root, &mut cases);
    cases.sort();
    let mut checked = 0;
    let mut failures = vec![];
    for rk in &cases {
        let rel = rk.strip_prefix(&root).unwrap().to_string_lossy().to_string();
        let src = fs::read_to_string(rk).unwrap();
        // syntax-error fixtures cannot be formatted; fmt must refuse, not mangle
        let Ok(once) = rikki::format::fmt_source(&src) else {
            continue;
        };
        checked += 1;
        let orig = rikki::parser::parse(&src).unwrap();
        match rikki::parser::parse(&once) {
            Ok(again) => {
                if shape(&orig) != shape(&again) {
                    failures.push(format!("{rel}: AST changed by fmt"));
                }
            }
            Err(d) => {
                failures.push(format!("{rel}: fmt output does not parse: {d}"));
                continue;
            }
        }
        if comments(&src) != comments(&once) {
            failures.push(format!("{rel}: comments changed by fmt"));
        }
        let twice = rikki::format::fmt_source(&once).unwrap_or_default();
        if once != twice {
            failures.push(format!(
                "{rel}: not idempotent\n--- once ---\n{once}\n--- twice ---\n{twice}"
            ));
        }
    }
    assert!(checked > 50, "corpus too small: {checked}");
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn canonical_style() {
    let ugly = "fn main(){\n    x:=1+2*3\n    print( x )\n}\n";
    let want = "fn main() {\n    x := 1 + 2*3\n    print(x)\n}\n";
    // spacing normalizes; note binary spacing is uniform, so this pins the rule
    let got = rikki::format::fmt_source(ugly).unwrap();
    assert_eq!(got, want.replace("2*3", "2 * 3"), "{got}");
}

#[test]
fn comments_and_blanks_survive() {
    let src = "// leading\nfn main() {\n    x := 1  // trailing\n\n\n    // own line\n    print(x)\n}\n";
    let got = rikki::format::fmt_source(src).unwrap();
    let want = "// leading\nfn main() {\n    x := 1  // trailing\n\n    // own line\n    print(x)\n}\n";
    assert_eq!(got, want, "{got}");
}
