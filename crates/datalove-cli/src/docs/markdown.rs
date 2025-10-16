use rmx::prelude::*;
use comrak::{markdown_to_html, ComrakOptions};

/// Convert markdown to HTML with syntax highlighting.
pub fn markdown_to_html_with_highlighting(markdown: &str) -> String {
    let mut options = ComrakOptions::default();

    // Enable common extensions.
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.extension.header_ids = Some("".to_string());
    options.extension.footnotes = true;
    options.extension.description_lists = true;

    // Enable GitHub-style pre lang.
    options.render.github_pre_lang = true;
    options.render.unsafe_ = true;

    markdown_to_html(markdown, &options)
}

/// Extract title from markdown (first h1).
pub fn extract_title(markdown: &str) -> Option<String> {
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") {
            return Some(trimmed[2..].trim().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_title() {
        let md = "# Hello World\n\nSome content";
        assert_eq!(extract_title(md), Some("Hello World".to_string()));
    }

    #[test]
    fn test_markdown_conversion() {
        let md = "# Title\n\nSome **bold** text.";
        let html = markdown_to_html_with_highlighting(md);
        assert!(html.contains("<h1"));
        assert!(html.contains("<strong>bold</strong>"));
    }
}
