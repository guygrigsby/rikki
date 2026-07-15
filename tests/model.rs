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
    assert_eq!(double.ty.as_deref(), Some("(int) (int)"));
}

#[test]
fn extract_pins_import_symbol_from_the_real_loader() {
    // The loader rewrites `.nv` import paths to the bare file stem before
    // the model sees them (src/loader.rs); the Import symbol carries that
    // rewritten form as both name and qualified.
    let prog = load_fixture("model_basic");
    let syms = nevla::model::symbols::extract(&prog);
    let imports: Vec<&nevla::model::Symbol> = syms
        .iter()
        .filter(|s| s.kind == nevla::model::SymbolKind::Import)
        .collect();
    assert_eq!(imports.len(), 1, "got {imports:?}");
    let util = imports[0];
    assert_eq!(util.name, "util");
    assert_eq!(util.qualified, "util");
    assert_eq!(util.file, "main.nv");
    assert_eq!(util.id.0, "import:main.nv:util");
    assert!(!util.is_py);
}
