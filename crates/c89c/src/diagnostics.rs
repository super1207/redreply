//! Unified compiler diagnostics with source locations.

use std::fmt;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub file: String,
    pub line: u32,
    pub column: u32,
}

impl Span {
    pub fn new(file: impl Into<String>, line: u32, column: u32) -> Self {
        Self {
            file: file.into(),
            line: line.max(1),
            column: column.max(1),
        }
    }

    pub fn unknown() -> Self {
        Self::new("<unknown>", 1, 1)
    }

    pub fn for_path(path: &Path) -> Self {
        Self::new(path.display().to_string(), 1, 1)
    }
}

impl Default for Span {
    fn default() -> Self {
        Self::unknown()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticStage {
    Lex,
    Preprocess,
    Parse,
    Semantic,
    Codegen,
    Io,
}

impl fmt::Display for DiagnosticStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Lex => "lex",
            Self::Preprocess => "preprocess",
            Self::Parse => "parse",
            Self::Semantic => "semantic",
            Self::Codegen => "codegen",
            Self::Io => "io",
        };
        f.write_str(name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    pub stage: DiagnosticStage,
    pub span: Span,
    pub message: String,
}

pub type CompileResult<T> = Result<T, CompileError>;

impl CompileError {
    pub fn new(stage: DiagnosticStage, span: Span, message: impl Into<String>) -> Self {
        Self {
            stage,
            span,
            message: strip_legacy_location(&message.into()),
        }
    }

    pub fn lex(span: Span, message: impl Into<String>) -> Self {
        Self::new(DiagnosticStage::Lex, span, message)
    }

    pub fn preprocess(span: Span, message: impl Into<String>) -> Self {
        Self::new(DiagnosticStage::Preprocess, span, message)
    }

    pub fn parse(span: Span, message: impl Into<String>) -> Self {
        Self::new(DiagnosticStage::Parse, span, message)
    }

    pub fn semantic(span: Span, message: impl Into<String>) -> Self {
        Self::new(DiagnosticStage::Semantic, span, message)
    }

    pub fn codegen(span: Span, message: impl Into<String>) -> Self {
        Self::new(DiagnosticStage::Codegen, span, message)
    }

    pub fn io(span: Span, message: impl Into<String>) -> Self {
        Self::new(DiagnosticStage::Io, span, message)
    }

    /// Compatibility helper for existing tests that previously received String errors.
    pub fn contains(&self, pattern: &str) -> bool {
        self.to_string().contains(pattern)
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: error[{}]: {}",
            self.span.file, self.span.line, self.span.column, self.stage, self.message
        )
    }
}

impl std::error::Error for CompileError {}

fn strip_legacy_location(message: &str) -> String {
    if let Some(rest) = message.strip_prefix("第 ") {
        if let Some((line, rest)) = rest.split_once(" 行") {
            if line.parse::<u32>().is_ok() {
                return rest.trim_start_matches([':', '：', ' ']).to_string();
            }
        }
    }
    message.to_string()
}

/// Best-effort extraction for legacy internal errors that already contain a line number.
pub(crate) fn legacy_span(file: impl Into<String>, message: &str) -> Span {
    let mut digits = String::new();
    let mut started = false;
    for ch in message.chars() {
        if ch.is_ascii_digit() {
            started = true;
            digits.push(ch);
        } else if started {
            break;
        }
    }
    let line = digits.parse().unwrap_or(1);
    Span::new(file, line, 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_has_stable_machine_readable_location() {
        let err = CompileError::parse(Span::new("test.c", 3, 7), "expected ';'");
        assert_eq!(err.to_string(), "test.c:3:7: error[parse]: expected ';'");
    }

    #[test]
    fn legacy_line_is_preserved() {
        assert_eq!(legacy_span("test.c", "第 12 行: bad").line, 12);
    }
    #[test]
    fn legacy_prefix_is_not_duplicated() {
        let error = CompileError::parse(Span::new("test.c", 9, 2), "第 9 行: expected ';'");
        assert_eq!(error.to_string(), "test.c:9:2: error[parse]: expected ';'");
    }
}
