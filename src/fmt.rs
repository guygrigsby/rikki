//! The printf/sprintf format grammar: `%[width][.precision]verb`, `%%` for a
//! literal percent. One scanner shared by the checker (compile-time
//! validation of literal formats) and the runtime (rendering), so the two
//! can never drift.

/// Pads past this limit are rejected: an absurd pad is an allocation abort
/// the panic net cannot catch.
pub const MAX_PAD: usize = 1 << 20;

pub enum Piece {
    /// Literal text between verbs; `%%` collapses into it as `%`.
    Lit(String),
    Verb {
        width: usize,
        prec: Option<usize>,
        verb: char,
    },
}

pub enum FmtError {
    EndsInsideVerb,
    WidthTooLarge,
    PrecisionTooLarge,
}

impl FmtError {
    /// `name` is the builtin being checked or run: "printf" or "sprintf".
    pub fn msg(&self, name: &str) -> String {
        match self {
            FmtError::EndsInsideVerb => format!("{name}: format ends inside a verb"),
            FmtError::WidthTooLarge => format!("{name}: width too large"),
            FmtError::PrecisionTooLarge => format!("{name}: precision too large"),
        }
    }
}

pub fn parse(fmt: &str) -> Result<Vec<Piece>, FmtError> {
    let mut out = vec![];
    let mut lit = String::new();
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            lit.push(c);
            continue;
        }
        if chars.peek() == Some(&'%') {
            chars.next();
            lit.push('%');
            continue;
        }
        if !lit.is_empty() {
            out.push(Piece::Lit(std::mem::take(&mut lit)));
        }
        let mut digits = String::new();
        while chars.peek().is_some_and(|d| d.is_ascii_digit()) {
            digits.push(chars.next().unwrap());
        }
        let width: usize = match digits.parse() {
            _ if digits.is_empty() => 0,
            Ok(w) if w <= MAX_PAD => w,
            _ => return Err(FmtError::WidthTooLarge),
        };
        let mut prec = None;
        if chars.peek() == Some(&'.') {
            chars.next();
            let mut digits = String::new();
            while chars.peek().is_some_and(|d| d.is_ascii_digit()) {
                digits.push(chars.next().unwrap());
            }
            prec = match digits.parse() {
                _ if digits.is_empty() => None,
                Ok(p) if p <= MAX_PAD => Some(p),
                _ => return Err(FmtError::PrecisionTooLarge),
            };
        }
        let verb = chars.next().ok_or(FmtError::EndsInsideVerb)?;
        out.push(Piece::Verb { width, prec, verb });
    }
    if !lit.is_empty() {
        out.push(Piece::Lit(lit));
    }
    Ok(out)
}
