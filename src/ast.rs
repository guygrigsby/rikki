#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub line: u32,
    pub col: u32,
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
    pub line: u32,
    pub col: u32,
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
    ForIn {
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
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Decl {
    Fn(FnDecl),
    Struct {
        name: String,
        fields: Vec<(String, TypeExpr)>,
        line: u32,
        col: u32,
    },
    Import {
        path: String,
        py: bool,
        line: u32,
        col: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Program {
    pub decls: Vec<Decl>,
}
