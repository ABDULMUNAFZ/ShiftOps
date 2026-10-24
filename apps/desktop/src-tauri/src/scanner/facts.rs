//! Facts produced by the scanner. Every fact carries its evidence: the file,
//! the line and/or structured pointer, how it was detected and a confidence.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Language,
    Framework,
    Runtime,
    PackageManager,
    Workspace,
    Manifest,
    Lockfile,
    Docker,
    Ci,
    Script,
    Dependency,
    Metadata,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Language => "language",
            Self::Framework => "framework",
            Self::Runtime => "runtime",
            Self::PackageManager => "package_manager",
            Self::Workspace => "workspace",
            Self::Manifest => "manifest",
            Self::Lockfile => "lockfile",
            Self::Docker => "docker",
            Self::Ci => "ci",
            Self::Script => "script",
            Self::Dependency => "dependency",
            Self::Metadata => "metadata",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "language" => Self::Language,
            "framework" => Self::Framework,
            "runtime" => Self::Runtime,
            "package_manager" => Self::PackageManager,
            "workspace" => Self::Workspace,
            "manifest" => Self::Manifest,
            "lockfile" => Self::Lockfile,
            "docker" => Self::Docker,
            "ci" => Self::Ci,
            "script" => Self::Script,
            "dependency" => Self::Dependency,
            "metadata" => Self::Metadata,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fact {
    pub category: Category,
    pub key: String,
    pub value: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    /// JSON-pointer-like path inside structured files, e.g. `/engines/node`.
    pub pointer: Option<String>,
    pub method: String,
    pub confidence: f64,
}

impl Fact {
    pub fn new(
        category: Category,
        key: impl Into<String>,
        value: impl Into<String>,
        method: impl Into<String>,
        confidence: f64,
    ) -> Self {
        Self {
            category,
            key: key.into(),
            value: crate::redact::redact(&value.into()),
            file: None,
            line: None,
            pointer: None,
            method: method.into(),
            confidence,
        }
    }

    pub fn at(mut self, file: &str, line: Option<u32>) -> Self {
        self.file = Some(file.to_string());
        self.line = line;
        self
    }

    pub fn pointer(mut self, pointer: impl Into<String>) -> Self {
        self.pointer = Some(pointer.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictValue {
    pub value: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub method: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conflict {
    pub subject: String,
    pub message: String,
    pub values: Vec<ConflictValue>,
}

impl ConflictValue {
    pub fn from_fact(f: &Fact) -> Self {
        Self {
            value: f.value.clone(),
            file: f.file.clone(),
            line: f.line,
            method: f.method.clone(),
        }
    }
}

/// 1-based line of the first occurrence of `needle` at or after `from_line`.
pub fn line_of(text: &str, needle: &str, from_line: u32) -> Option<u32> {
    text.lines()
        .enumerate()
        .skip(from_line.saturating_sub(1) as usize)
        .find(|(_, l)| l.contains(needle))
        .map(|(i, _)| (i + 1) as u32)
}

/// Line of a JSON key, preferring an occurrence after its parent key.
pub fn json_key_line(text: &str, parent: Option<&str>, key: &str) -> Option<u32> {
    let quoted = format!("\"{key}\"");
    match parent {
        Some(p) => {
            let start = line_of(text, &format!("\"{p}\""), 1).unwrap_or(1);
            line_of(text, &quoted, start)
        }
        None => line_of(text, &quoted, 1),
    }
}
