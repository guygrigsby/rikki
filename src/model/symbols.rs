//! Structural symbol extraction: walks a loaded `Program`'s top-level
//! declarations (functions, structs, imports) into `Symbol`s. No reference
//! resolution or call-graph work happens here; see the model contract for
//! the full picture.

use crate::ast::{Decl, FnDecl, Param, Program, TypeExpr};

use super::{Pos, Symbol, SymbolId, SymbolKind};

/// Extract one `Symbol` per top-level declaration in `prog`: functions,
/// structs, and imports. Field and param symbols are out of scope here
/// (skeleton extraction only).
pub fn extract(prog: &Program) -> Vec<Symbol> {
    prog.decls.iter().map(symbol_for).collect()
}

fn symbol_for(decl: &Decl) -> Symbol {
    match decl {
        Decl::Fn(f) => fn_symbol(f),
        Decl::Struct {
            name,
            fields,
            span,
            file,
        } => {
            let unqualified = unqualified_name(name);
            let file = file.clone().unwrap_or_default();
            // py-typed struct fields are legal (spec 7.11 reads them as
            // plain py values), so a struct crosses the py boundary when
            // any field does, mirroring the params/ret rule for fns.
            let is_py = fields.iter().any(|(_, ty)| is_py_type(ty));
            Symbol {
                id: SymbolId(symbol_id(SymbolKind::Struct, &file, name)),
                kind: SymbolKind::Struct,
                name: unqualified,
                qualified: name.clone(),
                file,
                def: Pos {
                    line: span.line,
                    col: span.col,
                },
                ty: None,
                is_py,
            }
        }
        Decl::Import {
            path,
            py,
            span,
            file,
        } => {
            let file = file.clone().unwrap_or_default();
            Symbol {
                id: SymbolId(symbol_id(SymbolKind::Import, &file, path)),
                kind: SymbolKind::Import,
                name: path.clone(),
                qualified: path.clone(),
                file,
                def: Pos {
                    line: span.line,
                    col: span.col,
                },
                ty: None,
                is_py: *py,
            }
        }
    }
}

fn fn_symbol(f: &FnDecl) -> Symbol {
    let unqualified = unqualified_name(&f.name);
    let file = f.file.clone().unwrap_or_default();
    let is_py = f.params.iter().any(|p| p.ty.as_ref().is_some_and(is_py_type))
        || f.ret.iter().any(is_py_type);
    Symbol {
        id: SymbolId(symbol_id(SymbolKind::Function, &file, &f.name)),
        kind: SymbolKind::Function,
        name: unqualified,
        qualified: f.name.clone(),
        file,
        def: Pos {
            line: f.span.line,
            col: f.span.col,
        },
        ty: Some(render_fn_type(&f.params, &f.ret)),
        is_py,
    }
}

/// The last dotted segment of a loader-qualified name: `util.Double` ->
/// `Double`, `compute` (root files stay bare) -> `compute`.
fn unqualified_name(qualified: &str) -> String {
    qualified
        .rsplit_once('.')
        .map(|(_, last)| last)
        .unwrap_or(qualified)
        .to_string()
}

fn symbol_id(kind: SymbolKind, file: &str, qualified: &str) -> String {
    format!("{}:{}:{}", kind.tag(), file, qualified)
}

fn is_py_type(ty: &TypeExpr) -> bool {
    matches!(ty, TypeExpr::Named(n) if n == "py")
}

/// A function's signature as `(param types) (return types)`, e.g.
/// `(int) (int)` for `fn Double(n int) int`. Shares `render_signature` with
/// `TypeExpr::Fn`, which wraps the same shape in a `fn` prefix since it
/// appears in type position.
fn render_fn_type(params: &[Param], ret: &[TypeExpr]) -> String {
    let param_types: Vec<String> = params
        .iter()
        .map(|p| p.ty.as_ref().map(render_type).unwrap_or_else(|| "?".to_string()))
        .collect();
    let ret_types: Vec<String> = ret.iter().map(render_type).collect();
    render_signature(&param_types, &ret_types)
}

/// Render a `TypeExpr` as nevla source syntax: `int`, `[]int`, `map[str]int`,
/// `int?`, `fn(int, int) (int)`.
fn render_type(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Named(n) => n.clone(),
        TypeExpr::List(elem) => format!("[]{}", render_type(elem)),
        TypeExpr::Map(k, v) => format!("map[{}]{}", render_type(k), render_type(v)),
        TypeExpr::Opt(inner) => format!("{}?", render_type(inner)),
        TypeExpr::Fn(args, rets) => {
            let arg_types: Vec<String> = args.iter().map(render_type).collect();
            let ret_types: Vec<String> = rets.iter().map(render_type).collect();
            format!("fn{}", render_signature(&arg_types, &ret_types))
        }
    }
}

/// `(a, b) (c, d)`: the comma-joined, paren-wrapped shape shared by a
/// declaration's full signature (`render_fn_type`) and `TypeExpr::Fn`.
fn render_signature(params: &[String], rets: &[String]) -> String {
    format!("({}) ({})", params.join(", "), rets.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::Span;

    fn named(n: &str) -> TypeExpr {
        TypeExpr::Named(n.to_string())
    }

    #[test]
    fn unqualified_name_strips_module_prefix() {
        assert_eq!(unqualified_name("util.Double"), "Double");
        assert_eq!(unqualified_name("compute"), "compute");
    }

    #[test]
    fn render_type_covers_every_variant() {
        assert_eq!(render_type(&named("int")), "int");
        assert_eq!(
            render_type(&TypeExpr::List(Box::new(named("int")))),
            "[]int"
        );
        assert_eq!(
            render_type(&TypeExpr::Map(Box::new(named("str")), Box::new(named("int")))),
            "map[str]int"
        );
        assert_eq!(render_type(&TypeExpr::Opt(Box::new(named("int")))), "int?");
        assert_eq!(
            render_type(&TypeExpr::Fn(vec![named("int")], vec![named("bool")])),
            "fn(int) (bool)"
        );
    }

    #[test]
    fn extract_struct_and_import() {
        let prog = Program {
            decls: vec![
                Decl::Struct {
                    name: "util.Point".to_string(),
                    fields: vec![("X".to_string(), named("int"))],
                    span: Span::new(5, 1),
                    file: Some("util.nv".to_string()),
                },
                Decl::Import {
                    path: "util".to_string(),
                    py: false,
                    span: Span::new(1, 1),
                    file: Some("main.nv".to_string()),
                },
                Decl::Import {
                    path: "numpy".to_string(),
                    py: true,
                    span: Span::new(2, 1),
                    file: Some("main.nv".to_string()),
                },
            ],
        };
        let syms = extract(&prog);
        let point = syms.iter().find(|s| s.kind == SymbolKind::Struct).unwrap();
        assert_eq!(point.name, "Point");
        assert_eq!(point.qualified, "util.Point");
        assert_eq!(point.file, "util.nv");
        assert_eq!(point.id.0, "struct:util.nv:util.Point");
        assert_eq!(point.ty, None);
        assert!(!point.is_py);

        let imports: Vec<&Symbol> = syms
            .iter()
            .filter(|s| s.kind == SymbolKind::Import)
            .collect();
        let util_import = imports.iter().find(|s| s.name == "util").unwrap();
        assert!(!util_import.is_py);
        assert_eq!(util_import.id.0, "import:main.nv:util");
        let numpy_import = imports.iter().find(|s| s.name == "numpy").unwrap();
        assert!(numpy_import.is_py);
    }

    #[test]
    fn fn_symbol_marks_py_boundary() {
        let f = FnDecl {
            name: "callModel".to_string(),
            params: vec![Param {
                name: "m".to_string(),
                ty: Some(named("py")),
            }],
            ret: vec![],
            body: vec![],
            span: Span::new(3, 1),
            file: Some("main.nv".to_string()),
        };
        let sym = fn_symbol(&f);
        assert!(sym.is_py);
        assert_eq!(sym.ty, Some("(py) ()".to_string()));
    }
}
