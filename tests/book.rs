//! Documentation examples stay honest: every nevla snippet in the book,
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
    for extra in ["README.md", "docs/nevla-primer.md", "language-spec.md"] {
        sources.push((
            extra.to_string(),
            fs::read_to_string(root.join(extra)).unwrap(),
        ));
    }
    for (name, text) in &sources {
        let (name, text) = (name.clone(), text.clone());
        let mut rest = text.as_str();
        while let Some(start) = rest.find("```nevla\n") {
            rest = &rest[start + "```nevla\n".len()..];
            let Some(end) = rest.find("```") else { break };
            let code = &rest[..end];
            rest = &rest[end..];
            blocks += 1;
            let prog = match nevla::parser::parse(code) {
                Ok(p) => p,
                Err(d) => {
                    failures.push(format!("{name}: does not parse: {d}\n---\n{code}"));
                    continue;
                }
            };
            if code.contains("fn main") {
                if let Err(ds) = nevla::typecheck::check(&prog) {
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
            match nevla::parser::parse(part) {
                Ok(prog) => {
                    if let Err(ds) = nevla::typecheck::check(&prog) {
                        let msgs: Vec<String> = ds.iter().map(|d| d.to_string()).collect();
                        failures.push(format!(
                            "main.js example does not typecheck:\n{}\n---\n{part}",
                            msgs.join("\n")
                        ));
                    }
                }
                Err(d) => {
                    failures.push(format!("main.js example does not parse: {d}\n---\n{part}"))
                }
            }
        }
    }
    assert!(blocks >= 12, "docs corpus too small: {blocks}");
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn example_programs_compile() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut count = 0;
    let mut failures = vec![];
    // every .nv the repo RUNS lives here: examples, the release
    // pipeline's wheel repair, the book's generator and preprocessor.
    // The goldens have their own runner; a language change that breaks
    // any of these is a red test, not a broken release (v0.2.1 learned
    // this the hard way: repair-wheel.nv still said .find).
    let mut stack = vec![
        root.join("examples"),
        root.join("packaging"),
        root.join("docs/book"),
    ];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n == "src" || n == "theme") {
                    continue; // book chapters and css, not programs
                }
                stack.push(p);
                continue;
            }
            if p.extension().is_none_or(|e| e != "nv") {
                continue;
            }
            count += 1;
            let name = p.strip_prefix(&root).unwrap().display().to_string();
            let code = fs::read_to_string(&p).unwrap();
            let prog = match nevla::parser::parse(&code) {
                Ok(prog) => prog,
                Err(d) => {
                    failures.push(format!("{name}: does not parse: {d}"));
                    continue;
                }
            };
            if let Err(ds) = nevla::typecheck::check(&prog) {
                let msgs: Vec<String> = ds.iter().map(|d| d.to_string()).collect();
                failures.push(format!("{name}: does not typecheck:\n{}", msgs.join("\n")));
            }
        }
    }
    assert!(count >= 4, "examples corpus too small: {count}");
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
