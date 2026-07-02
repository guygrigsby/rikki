use crate::diag::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    NoneLit,
    Ident(String),
    List(Vec<Expr>),
    ListLit {
        elem: TypeExpr,
        items: Vec<Expr>,
    },
    MapLit {
        key: TypeExpr,
        val: TypeExpr,
        entries: Vec<(Expr, Expr)>,
    },
    StructLit {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    Unary {
        op: UnOp,
        rhs: Box<Expr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        kwargs: Vec<(String, Expr)>,
    },
    Method {
        recv: Box<Expr>,
        name: String,
        args: Vec<Expr>,
        kwargs: Vec<(String, Expr)>,
    },
    Field {
        recv: Box<Expr>,
        name: String,
    },
    Index {
        recv: Box<Expr>,
        idx: Box<Expr>,
    },
    Slice {
        recv: Box<Expr>,
        lo: Box<Expr>,
        hi: Box<Expr>,
    },
    Lambda {
        params: Vec<Param>,
        ret: Option<Vec<TypeExpr>>,
        body: Block,
    },
    Check(Box<Expr>),
    Conv {
        target: TypeExpr,
        arg: Box<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnOp {
    Not,
    Neg,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    /// `@`: matrix multiplication, defined only through the py bridge.
    MatMul,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    Named(String), // int, float, bool, str, error, py, struct names
    List(Box<TypeExpr>),
    Map(Box<TypeExpr>, Box<TypeExpr>),
    Opt(Box<TypeExpr>),
    Fn(Vec<TypeExpr>, Vec<TypeExpr>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Option<TypeExpr>, // None only in lambdas (contextual inference)
}

pub type Block = Vec<Stmt>;

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    Let {
        names: Vec<String>,
        expr: Expr,
    },
    Assign {
        target: Expr,
        expr: Expr,
    },
    Expr(Expr),
    Return(Vec<Expr>),
    If {
        cond: Expr,
        then: Block,
        elifs: Vec<(Expr, Block)>,
        els: Option<Block>,
    },
    ForRange {
        names: Vec<String>,
        iter: Expr,
        body: Block,
    },
    ForCond {
        cond: Option<Expr>,
        body: Block,
    },
    Break,
    Continue,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FnDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub ret: Vec<TypeExpr>,
    pub body: Block,
    pub span: Span,
    pub file: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Decl {
    Fn(FnDecl),
    Struct {
        name: String,
        fields: Vec<(String, TypeExpr)>,
        span: Span,
        file: Option<String>,
    },
    Import {
        path: String,
        py: bool,
        span: Span,
        file: Option<String>,
    },
}

impl Decl {
    /// Source file name, stamped by the loader after parsing.
    pub fn file(&self) -> Option<&str> {
        match self {
            Decl::Fn(f) => f.file.as_deref(),
            Decl::Struct { file, .. } | Decl::Import { file, .. } => file.as_deref(),
        }
    }

    pub fn set_file(&mut self, name: Option<String>) {
        match self {
            Decl::Fn(f) => f.file = name,
            Decl::Struct { file, .. } | Decl::Import { file, .. } => *file = name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Program {
    pub decls: Vec<Decl>,
}
