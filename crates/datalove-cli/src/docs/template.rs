use rmx::prelude::*;

pub struct PageData {
    pub title: String,
    pub content: String,
    pub nav_html: String,
}

/// Generate the HTML template for a documentation page.
pub fn render_page(data: PageData) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{title} - Datalove Documentation</title>
    <style>{css}</style>
</head>
<body>
    <div class="container">
        <nav class="sidebar">
            <div class="logo">
                <h1>Datalove</h1>
                <p class="tagline">data|is·my·love|language</p>
            </div>
            {nav}
        </nav>
        <main class="content">
            {content}
        </main>
    </div>
</body>
</html>"#,
        title = escape_html(&data.title),
        css = CSS,
        nav = data.nav_html,
        content = data.content,
    )
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

const CSS: &str = r#"
* {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
}

:root {
    --bg-main: #ffffff;
    --bg-sidebar: #f8f9fa;
    --bg-code: #f5f5f5;
    --text-main: #2c3e50;
    --text-secondary: #6c757d;
    --border: #dee2e6;
    --link: #3498db;
    --link-hover: #2980b9;
    --code-text: #c7254e;
}

@media (prefers-color-scheme: dark) {
    :root {
        --bg-main: #1a1a1a;
        --bg-sidebar: #242424;
        --bg-code: #2d2d2d;
        --text-main: #e0e0e0;
        --text-secondary: #a0a0a0;
        --border: #404040;
        --link: #5dade2;
        --link-hover: #85c1e9;
        --code-text: #f92672;
    }
}

body {
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
    line-height: 1.6;
    color: var(--text-main);
    background-color: var(--bg-main);
}

.container {
    display: flex;
    min-height: 100vh;
}

.sidebar {
    width: 280px;
    background-color: var(--bg-sidebar);
    padding: 2rem 1.5rem;
    border-right: 1px solid var(--border);
    position: sticky;
    top: 0;
    height: 100vh;
    overflow-y: auto;
}

.logo h1 {
    font-size: 1.8rem;
    margin-bottom: 0.25rem;
    color: var(--text-main);
}

.logo .tagline {
    font-size: 0.9rem;
    color: var(--text-secondary);
    margin-bottom: 2rem;
    font-style: italic;
}

.sidebar nav ul {
    list-style: none;
}

.sidebar nav li {
    margin: 0.5rem 0;
}

.sidebar nav a {
    color: var(--text-main);
    text-decoration: none;
    display: block;
    padding: 0.5rem;
    border-radius: 4px;
    transition: background-color 0.2s;
}

.sidebar nav a:hover {
    background-color: var(--bg-code);
}

.content {
    flex: 1;
    padding: 3rem;
    max-width: 900px;
}

h1, h2, h3, h4, h5, h6 {
    margin-top: 2rem;
    margin-bottom: 1rem;
    line-height: 1.3;
}

h1 {
    font-size: 2.5rem;
    border-bottom: 2px solid var(--border);
    padding-bottom: 0.5rem;
    margin-top: 0;
}

h2 {
    font-size: 2rem;
    border-bottom: 1px solid var(--border);
    padding-bottom: 0.4rem;
}

h3 {
    font-size: 1.5rem;
}

p {
    margin: 1rem 0;
}

a {
    color: var(--link);
    text-decoration: none;
}

a:hover {
    color: var(--link-hover);
    text-decoration: underline;
}

code {
    background-color: var(--bg-code);
    color: var(--code-text);
    padding: 0.2rem 0.4rem;
    border-radius: 3px;
    font-family: "SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace;
    font-size: 0.9em;
}

pre {
    background-color: var(--bg-code);
    padding: 1rem;
    border-radius: 6px;
    overflow-x: auto;
    margin: 1.5rem 0;
    border: 1px solid var(--border);
}

pre code {
    background-color: transparent;
    color: var(--text-main);
    padding: 0;
    font-size: 0.9rem;
}

ul, ol {
    margin: 1rem 0;
    padding-left: 2rem;
}

li {
    margin: 0.5rem 0;
}

blockquote {
    border-left: 4px solid var(--border);
    padding-left: 1rem;
    margin: 1.5rem 0;
    color: var(--text-secondary);
    font-style: italic;
}

table {
    width: 100%;
    border-collapse: collapse;
    margin: 1.5rem 0;
}

th, td {
    padding: 0.75rem;
    border: 1px solid var(--border);
    text-align: left;
}

th {
    background-color: var(--bg-sidebar);
    font-weight: 600;
}

hr {
    border: none;
    border-top: 1px solid var(--border);
    margin: 2rem 0;
}

@media (max-width: 768px) {
    .container {
        flex-direction: column;
    }

    .sidebar {
        width: 100%;
        height: auto;
        position: relative;
    }

    .content {
        padding: 1.5rem;
    }
}
"#;
