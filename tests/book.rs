//! The book's examples stay honest: every ```rikki block parses, and
//! every complete program (fn main) typechecks. Rotten docs examples are
//! the classic documentation failure; this makes them a test failure.

use std::{fs, path::PathBuf};

#[test]
fn book_examples_compile() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/book/src");
    let mut blocks = 0;
    let mut failures = vec![];
    for entry in fs::read_dir(&dir).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().is_none_or(|e| e != "md") {
            continue;
        }
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let text = fs::read_to_string(&p).unwrap();
        let mut rest = text.as_str();
        while let Some(start) = rest.find("```rikki\n") {
            rest = &rest[start + "```rikki\n".len()..];
            let Some(end) = rest.find("```") else { break };
            let code = &rest[..end];
            rest = &rest[end..];
            blocks += 1;
            let prog = match rikki::parser::parse(code) {
                Ok(p) => p,
                Err(d) => {
                    failures.push(format!("{name}: does not parse: {d}\n---\n{code}"));
                    continue;
                }
            };
            if code.contains("fn main") {
                if let Err(ds) = rikki::typecheck::check(&prog) {
                    let msgs: Vec<String> = ds.iter().map(|d| d.to_string()).collect();
                    failures.push(format!(
                        "{name}: does not typecheck:\n{}\n---\n{code}",
                        msgs.join("\n")
                    ));
                }
            }
        }
    }
    assert!(blocks >= 8, "book corpus too small: {blocks}");
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
