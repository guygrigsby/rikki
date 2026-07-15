use std::path::Path;

fn load_fixture(name: &str) -> nevla::ast::Program {
    let entry = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
        .join("src/main.nv");
    nevla::loader::load(&entry).expect("load fixture")
}

#[test]
fn extract_finds_functions() {
    let prog = load_fixture("model_basic");
    let syms = nevla::model::symbols::extract(&prog);
    let functions: Vec<&nevla::model::Symbol> = syms
        .iter()
        .filter(|s| s.kind == nevla::model::SymbolKind::Function)
        .collect();
    let names: Vec<&str> = functions.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"Double"), "got {names:?}");
    assert!(names.contains(&"compute"), "got {names:?}");
    assert!(names.contains(&"main"), "got {names:?}");
    let double = functions.iter().find(|s| s.name == "Double").unwrap();
    assert_eq!(double.qualified, "util.Double");
    assert_eq!(double.file, "util.nv");
    assert_eq!(double.id.0, "function:util.nv:util.Double");
}
