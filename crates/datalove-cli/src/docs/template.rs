use rmx::prelude::*;
use rmx::std::path::Path;
use rmx::std::fs;

pub struct PageData {
    pub title: String,
    pub content: String,
    pub nav_html: String,
}

/// Load and render a page using the template from disk.
pub fn render_page(template_path: &Path, data: PageData) -> AnyResult<String> {
    let template = fs::read_to_string(template_path)
        .with_context(|| format!("failed to read template: {}", template_path.display()))?;

    let html = template
        .replace("{{title}}", &escape_html(&data.title))
        .replace("{{nav}}", &data.nav_html)
        .replace("{{content}}", &data.content);

    Ok(html)
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
