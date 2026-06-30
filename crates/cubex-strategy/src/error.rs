use crate::ast::SourceSpan;
use std::fmt;
use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, StrategyError>;

#[derive(Debug, thiserror::Error)]
pub enum StrategyError {
    #[error("failed to read strategy file `{path}`: {source}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read current directory: {0}")]
    CurrentDir(#[source] std::io::Error),
    #[error("strategy parse error: {0}")]
    Parse(String),
    #[error("strategy compile error: {0}")]
    Compile(String),
    #[error("{0}")]
    Diagnostic(SourceDiagnostic),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDiagnostic {
    pub kind: DiagnosticKind,
    pub source_name: Option<String>,
    pub source: String,
    pub span: SourceSpan,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticKind {
    Parse,
    Compile,
}

impl SourceDiagnostic {
    pub fn new(
        kind: DiagnosticKind,
        source: impl Into<String>,
        source_name: Option<&str>,
        span: SourceSpan,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            source_name: source_name.map(str::to_string),
            source: source.into(),
            span,
            message: message.into(),
        }
    }

    fn kind_label(&self) -> &'static str {
        match self.kind {
            DiagnosticKind::Parse => "parse",
            DiagnosticKind::Compile => "compile",
        }
    }

    fn source_label(&self) -> &str {
        self.source_name.as_deref().unwrap_or("<input>")
    }

    fn primary_line(&self) -> SourceLine<'_> {
        SourceLine::new(&self.source, self.span.start)
    }
}

impl fmt::Display for SourceDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let line = self.primary_line();
        let span_start = self.span.start.min(self.source.len());
        let span_end = self.span.end.min(line.end);
        let caret_len = if span_end > span_start {
            self.source[span_start..span_end].chars().count().max(1)
        } else {
            1
        };
        let mut caret_prefix = String::new();
        for ch in line.text.chars().take(line.column.saturating_sub(1)) {
            caret_prefix.push(if ch == '\t' { '\t' } else { ' ' });
        }

        writeln!(
            f,
            "strategy {} error at {}:{}:{}",
            self.kind_label(),
            self.source_label(),
            line.number,
            line.column
        )?;
        writeln!(f, "  {}", line.text)?;
        writeln!(f, "  {}{}", caret_prefix, "^".repeat(caret_len))?;
        write!(f, "  {}", self.message)
    }
}

impl StrategyError {
    pub fn parse_diagnostic(
        source: &str,
        source_name: Option<&str>,
        span: SourceSpan,
        message: impl Into<String>,
    ) -> Self {
        Self::Diagnostic(SourceDiagnostic::new(
            DiagnosticKind::Parse,
            source,
            source_name,
            span,
            message,
        ))
    }

    pub fn compile_diagnostic(
        source: &str,
        source_name: Option<&str>,
        span: SourceSpan,
        message: impl Into<String>,
    ) -> Self {
        Self::Diagnostic(SourceDiagnostic::new(
            DiagnosticKind::Compile,
            source,
            source_name,
            span,
            message,
        ))
    }
}

#[derive(Debug)]
struct SourceLine<'a> {
    number: usize,
    column: usize,
    text: &'a str,
    end: usize,
}

impl<'a> SourceLine<'a> {
    fn new(source: &'a str, byte_offset: usize) -> Self {
        let byte_offset = byte_offset.min(source.len());
        let prefix = &source[..byte_offset];
        let number = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let start = prefix.rfind('\n').map(|index| index + 1).unwrap_or(0);
        let end = source[byte_offset..]
            .find('\n')
            .map(|index| byte_offset + index)
            .unwrap_or(source.len());
        let column = source[start..byte_offset].chars().count() + 1;
        Self {
            number,
            column,
            text: &source[start..end],
            end,
        }
    }
}
