use iced::widget::markdown;
use pulldown_cmark::{html, Options, Parser};

pub fn parse_items(input: &str) -> Vec<markdown::Item> {
    markdown::parse(input).collect()
}

pub fn render_html(input: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(input, options);
    let mut output = String::new();
    html::push_html(&mut output, parser);
    output
}

pub fn is_markdown_file(path: Option<&std::path::Path>) -> bool {
    path.and_then(|value| value.extension()).and_then(|ext| ext.to_str()) == Some("md")
}

#[cfg(test)]
mod tests {
    use super::{is_markdown_file, render_html};
    use std::path::Path;

    #[test]
    fn renders_common_markdown_blocks() {
        let html = render_html("# Title\n\n- one\n- two\n\n```rs\nfn main() {}\n```");

        assert!(html.contains("<h1>Title</h1>"));
        assert!(html.contains("<li>one</li>"));
        assert!(html.contains("<code class=\"language-rs\">"));
    }

    #[test]
    fn detects_markdown_paths() {
        assert!(is_markdown_file(Some(Path::new("notes.md"))));
        assert!(!is_markdown_file(Some(Path::new("main.rs"))));
    }
}