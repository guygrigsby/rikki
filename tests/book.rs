//! Documentation examples stay honest: every rikki snippet in the book,
//! the README, the agent primer, and the playground's dropdown parses,
//! and every complete program (fn main) typechecks. Nothing is
//! "remembered" to be kept in sync; drift is a red test.

use std::{fs, path::PathBuf};

#[test]
fn book_examples_compile() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = root.join("docs/book/src");
    let mut blocks = 0;
    let mut failures = vec![];
    let mut sources: Vec<(String, String)> = vec![];
    for entry in fs::read_dir(&dir).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().is_none_or(|e| e != "md") {
            continue;
        }
        sources.push((
            p.file_name().unwrap().to_string_lossy().to_string(),
            fs::read_to_string(&p).unwrap(),
        ));
    }
    for extra in ["README.md", "docs/rikki-primer.md"] {
        sources.push((extra.to_string(), fs::read_to_string(root.join(extra)).unwrap()));
    }
    for (name, text) in &sources {
        let (name, text) = (name.clone(), text.clone());
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
    // the playground's dropdown examples are template literals in main.js
    let js = fs::read_to_string(root.join("playground/site/main.js")).unwrap();
    for (i, part) in js.split('`').enumerate() {
        // odd indices are inside template literals
        if i % 2 == 1 && part.contains("fn main") {
            blocks += 1;
            match rikki::parser::parse(part) {
                Ok(prog) => {
                    if let Err(ds) = rikki::typecheck::check(&prog) {
                        let msgs: Vec<String> = ds.iter().map(|d| d.to_string()).collect();
                        failures.push(format!(
                            "main.js example does not typecheck:\n{}\n---\n{part}",
                            msgs.join("\n")
                        ));
                    }
                }
                Err(d) => failures.push(format!("main.js example does not parse: {d}\n---\n{part}")),
            }
        }
    }
    assert!(blocks >= 12, "docs corpus too small: {blocks}");
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
