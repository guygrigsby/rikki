#[derive(Debug, Clone)]
pub struct Diag {
    pub msg: String,
    pub line: u32,
    pub col: u32,
}

impl std::fmt::Display for Diag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.msg)
    }
}
