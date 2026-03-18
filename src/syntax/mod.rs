use std::sync::LazyLock;
use std::path::Path;
use syntect::parsing::SyntaxSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxProfile {
    pub syntax_name: String,
    pub extension: String,
    pub highlight_token: String,
}

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);

pub fn detect(path: Option<&Path>) -> SyntaxProfile {
    let extension = path
        .and_then(|value| value.extension())
        .and_then(|ext| ext.to_str())
        .unwrap_or("txt")
        .to_lowercase();

    let syntax = SYNTAX_SET
        .find_syntax_by_extension(&extension)
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());

    let highlight_token = match extension.as_str() {
        "rs" => "rust",
        "md" => "markdown",
        "txt" => "txt",
        value => value,
    }
    .to_string();

    SyntaxProfile {
        syntax_name: syntax.name.to_string(),
        extension,
        highlight_token,
    }
}

#[cfg(test)]
mod tests {
    use super::detect;
    use std::path::Path;

    #[test]
    fn detects_rust_files() {
        let profile = detect(Some(Path::new("src/main.rs")));

        assert_eq!(profile.extension, "rs");
        assert_eq!(profile.syntax_name, "Rust");
    }

    #[test]
    fn falls_back_to_plain_text() {
        let profile = detect(Some(Path::new("README.unknown")));

        assert_eq!(profile.syntax_name, "Plain Text");
    }
}