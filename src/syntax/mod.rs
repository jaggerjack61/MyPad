use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;
use std::sync::Mutex;
use syntect::parsing::SyntaxSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxProfile {
    pub syntax_name: String,
    pub extension: String,
    pub highlight_token: String,
}

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static PROFILE_CACHE: LazyLock<Mutex<HashMap<String, SyntaxProfile>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn detect(path: Option<&Path>) -> SyntaxProfile {
    let extension = normalized_extension(path);

    if let Some(profile) = PROFILE_CACHE
        .lock()
        .expect("syntax profile cache")
        .get(&extension)
        .cloned()
    {
        return profile;
    }

    let profile = build_profile(extension.clone());
    PROFILE_CACHE
        .lock()
        .expect("syntax profile cache")
        .insert(extension, profile.clone());

    profile
}

fn normalized_extension(path: Option<&Path>) -> String {
    path.and_then(|value| value.extension())
        .and_then(|ext| ext.to_str())
        .unwrap_or("txt")
        .to_lowercase()
}

fn build_profile(extension: String) -> SyntaxProfile {
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
fn clear_profile_cache_for_tests() {
    PROFILE_CACHE.lock().expect("syntax profile cache").clear();
}

#[cfg(test)]
fn cached_profile_count_for_tests() -> usize {
    PROFILE_CACHE.lock().expect("syntax profile cache").len()
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

    #[test]
    fn caches_profiles_by_normalized_extension() {
        super::clear_profile_cache_for_tests();

        let first = detect(Some(Path::new("src/main.rs")));
        let second = detect(Some(Path::new("lib.RS")));

        assert_eq!(first, second);
        assert_eq!(super::cached_profile_count_for_tests(), 1);
    }
}
