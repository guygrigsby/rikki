/// A source position: 1-based line and column. Grows a file and byte range
/// when the tooling needs them; today the file rides on Diag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub line: u32,
    pub col: u32,
}

impl Span {
    pub fn new(line: u32, col: u32) -> Span {
        Span { line, col }
    }
}

impl std::fmt::Display for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

#[derive(Debug, Clone)]
pub struct Diag {
    pub msg: String,
    /// None for errors with no source position (an unreadable import, an
    /// import cycle); no more fabricated 1:1.
    pub span: Option<Span>,
    /// Source file name, stamped by the loader; None in the repl.
    pub file: Option<String>,
}

impl std::fmt::Display for Diag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(file) = &self.file {
            write!(f, "{file}:")?;
        }
        if let Some(span) = &self.span {
            write!(f, "{span}:")?;
        }
        if self.file.is_some() || self.span.is_some() {
            write!(f, " ")?;
        }
        write!(f, "{}", self.msg)
    }
}
