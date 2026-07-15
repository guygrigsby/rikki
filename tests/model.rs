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

#[test]
fn resolve_finds_cross_file_module_member_call() {
    let prog = load_fixture("model_basic");
    let syms = nevla::model::symbols::extract(&prog);
    let double = syms.iter().find(|s| s.name == "Double").expect("Double");
    let (refs, calls, _py) = nevla::model::resolve::resolve(&prog);

    // compute() calls util.Double(x): Method form.
    let hits: Vec<_> = refs.iter().filter(|r| r.target == double.id).collect();
    assert_eq!(hits.len(), 1, "refs to double: {refs:?}");
    assert_eq!(hits[0].form, nevla::model::ReferenceForm::ModuleMemberCall);
    assert_eq!(hits[0].file, "main.nv");
    let call_hits: Vec<_> = calls.iter().filter(|c| c.callee == double.id).collect();
    assert_eq!(call_hits.len(), 1);
    let compute = syms.iter().find(|s| s.name == "compute").unwrap();
    assert_eq!(call_hits[0].caller, compute.id);
}

#[test]
fn resolve_finds_same_file_ident_call_and_skips_shadowed() {
    let prog = load_fixture("model_basic");
    let syms = nevla::model::symbols::extract(&prog);
    let compute = syms.iter().find(|s| s.name == "compute").unwrap();
    let (refs, _calls, _py) = nevla::model::resolve::resolve(&prog);
    // main() calls compute(20): Ident form. shadowedCall's `compute` param
    // (tests/fixtures/model_basic/src/main.nv) shadows the module fn, so its
    // `return compute` must not add a second hit here.
    let hits: Vec<_> = refs.iter().filter(|r| r.target == compute.id).collect();
    assert_eq!(hits.len(), 1, "{refs:?}");
    assert_eq!(hits[0].form, nevla::model::ReferenceForm::Ident);
}

#[test]
fn resolve_records_py_import_boundary() {
    let entry = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/model_py/src/main.nv");
    let prog = nevla::loader::load(&entry).expect("load model_py");
    let (_r, _c, py) = nevla::model::resolve::resolve(&prog);
    assert!(py.iter().any(|b| b.note.contains("math")), "{py:?}");
}
