use serde::{Deserialize, Serialize};

pub mod resolve;
pub mod symbols;

/// Position in source, 1-based line and column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Pos {
    pub line: u32,
    pub col: u32,
}

/// Symbol identifier in the form "kind:file:qualified".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SymbolId(pub String);

/// The kind of symbol in the resolved model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SymbolKind {
    Module,
    Function,
    Struct,
    Field,
    Param,
    Import,
}

impl SymbolKind {
    /// Tag returns the string form of the SymbolKind for use in SymbolId.
    pub fn tag(&self) -> &'static str {
        match self {
            SymbolKind::Module => "module",
            SymbolKind::Function => "function",
            SymbolKind::Struct => "struct",
            SymbolKind::Field => "field",
            SymbolKind::Param => "param",
            SymbolKind::Import => "import",
        }
    }
}

/// A resolved symbol in the program.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Symbol {
    pub id: SymbolId,
    pub kind: SymbolKind,
    /// Source (unqualified) name, e.g. "double".
    pub name: String,
    /// Loader-qualified name, e.g. "util.double"; root-file symbols are unqualified.
    pub qualified: String,
    /// Filename with extension as stamped by the loader, e.g. "util.nv".
    pub file: String,
    /// Position of the definition.
    pub def: Pos,
    /// Rendered type for functions, None otherwise (skeleton).
    pub ty: Option<String>,
    /// Whether this symbol is from a Python boundary.
    pub is_py: bool,
}

/// The form in which a symbol is referenced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReferenceForm {
    /// An ExprKind::Ident whose string names a module-level fn or struct.
    Ident,
    /// Cross-module calls parsed as ExprKind::Method.
    ModuleMemberCall,
    /// ExprKind::StructLit where name names a known struct.
    StructLiteral,
}

/// A reference to a symbol in the program.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reference {
    /// The symbol being referenced.
    pub target: SymbolId,
    /// The file where the reference occurs.
    pub file: String,
    /// The position of the reference (the span of the AST node to rewrite).
    pub at: Pos,
    /// The form in which the reference appears.
    pub form: ReferenceForm,
}

/// An edge in the call graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallEdge {
    /// The caller's symbol ID.
    pub caller: SymbolId,
    /// The callee's symbol ID.
    pub callee: SymbolId,
    /// The position of the call.
    pub at: Pos,
}

/// A boundary between nevla and Python code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PyBoundary {
    /// The file where the boundary occurs.
    pub file: String,
    /// The position of the boundary.
    pub at: Pos,
    /// A note describing the boundary.
    pub note: String,
}

/// The resolved model of a program: symbols, references, and call edges.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Model {
    pub symbols: Vec<Symbol>,
    pub references: Vec<Reference>,
    pub calls: Vec<CallEdge>,
    pub py_boundaries: Vec<PyBoundary>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_serialize_deserialize() {
        let model = Model {
            symbols: vec![
                Symbol {
                    id: SymbolId("function:main.nv:double".to_string()),
                    kind: SymbolKind::Function,
                    name: "double".to_string(),
                    qualified: "double".to_string(),
                    file: "main.nv".to_string(),
                    def: Pos { line: 1, col: 1 },
                    ty: Some("(n: i64) i64".to_string()),
                    is_py: false,
                },
                Symbol {
                    id: SymbolId("struct:util.nv:Point".to_string()),
                    kind: SymbolKind::Struct,
                    name: "Point".to_string(),
                    qualified: "util.Point".to_string(),
                    file: "util.nv".to_string(),
                    def: Pos { line: 5, col: 1 },
                    ty: None,
                    is_py: false,
                },
            ],
            references: vec![
                Reference {
                    target: SymbolId("function:main.nv:double".to_string()),
                    file: "main.nv".to_string(),
                    at: Pos { line: 10, col: 5 },
                    form: ReferenceForm::Ident,
                },
                Reference {
                    target: SymbolId("struct:util.nv:Point".to_string()),
                    file: "main.nv".to_string(),
                    at: Pos { line: 15, col: 10 },
                    form: ReferenceForm::StructLiteral,
                },
            ],
            calls: vec![CallEdge {
                caller: SymbolId("function:main.nv:main".to_string()),
                callee: SymbolId("function:main.nv:double".to_string()),
                at: Pos { line: 12, col: 5 },
            }],
            py_boundaries: vec![PyBoundary {
                file: "main.nv".to_string(),
                at: Pos { line: 20, col: 1 },
                note: "import py numpy".to_string(),
            }],
        };

        // Serialize to JSON
        let json = serde_json::to_string(&model).expect("failed to serialize");

        // Deserialize back from JSON
        let deserialized: Model = serde_json::from_str(&json).expect("failed to deserialize");

        // Verify roundtrip
        assert_eq!(model, deserialized);
    }

    #[test]
    fn symbol_kind_tag() {
        assert_eq!(SymbolKind::Module.tag(), "module");
        assert_eq!(SymbolKind::Function.tag(), "function");
        assert_eq!(SymbolKind::Struct.tag(), "struct");
        assert_eq!(SymbolKind::Field.tag(), "field");
        assert_eq!(SymbolKind::Param.tag(), "param");
        assert_eq!(SymbolKind::Import.tag(), "import");
    }

    #[test]
    fn pos_roundtrip() {
        let pos = Pos { line: 42, col: 13 };
        let json = serde_json::to_string(&pos).expect("failed to serialize");
        let deserialized: Pos = serde_json::from_str(&json).expect("failed to deserialize");
        assert_eq!(pos, deserialized);
    }

    #[test]
    fn symbol_id_roundtrip() {
        let id = SymbolId("function:util.nv:helper".to_string());
        let json = serde_json::to_string(&id).expect("failed to serialize");
        let deserialized: SymbolId =
            serde_json::from_str(&json).expect("failed to deserialize");
        assert_eq!(id, deserialized);
    }

    #[test]
    fn model_default() {
        let model = Model::default();
        assert_eq!(model.symbols.len(), 0);
        assert_eq!(model.references.len(), 0);
        assert_eq!(model.calls.len(), 0);
        assert_eq!(model.py_boundaries.len(), 0);
    }
}
