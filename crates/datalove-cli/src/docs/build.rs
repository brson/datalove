use rmx::prelude::*;
use rmx::std::path::{Path, PathBuf};
use rmx::std::fs;

use super::template::{self, PageData};
use super::markdown;

pub struct BuildConfig {
    pub source_dir: PathBuf,
    pub output_dir: PathBuf,
}

/// Build the documentation site.
pub fn build_docs(config: BuildConfig) -> AnyResult<()> {
    println!("Building documentation...");
    println!("  Source: {}", config.source_dir.display());
    println!("  Output: {}", config.output_dir.display());

    if !config.source_dir.exists() {
        bail!("Source directory does not exist: {}", config.source_dir.display());
    }

    // Create output directory.
    fs::create_dir_all(&config.output_dir)?;

    // Collect all markdown files.
    let md_files = collect_markdown_files(&config.source_dir)?;
    println!("  Found {} markdown files", md_files.len());

    // Process each markdown file.
    for md_path in &md_files {
        process_markdown_file(&config, md_path, &md_files)?;
    }

    println!("Done!");

    Ok(())
}

/// Recursively collect all .md files in a directory.
fn collect_markdown_files(dir: &Path) -> AnyResult<Vec<PathBuf>> {
    let mut files = Vec::new();

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            files.extend(collect_markdown_files(&path)?);
        } else if path.extension().and_then(|s| s.to_str()) == Some("md") {
            files.push(path);
        }
    }

    Ok(files)
}

/// Build navigation HTML from markdown files, relative to the current page.
fn build_navigation(
    source_dir: &Path,
    files: &[PathBuf],
    current_file: &Path,
) -> AnyResult<String> {
    let mut nav = String::from("<ul>\n");

    // Get current file's relative path from source dir.
    let current_rel = current_file.strip_prefix(source_dir)
        .unwrap_or(current_file);

    for file in files {
        if file.file_name().and_then(|n| n.to_str()) == Some("index.md") {
            let rel_path = file.strip_prefix(source_dir)
                .unwrap_or(file);
            let html_path = rel_path.with_extension("html");

            let link_text = if rel_path == Path::new("index.md") {
                "Home".to_string()
            } else {
                rel_path.parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .unwrap_or("Unknown")
                    .to_string()
            };

            // Compute relative path from current file to target file.
            let relative_link = compute_relative_path(current_rel, &html_path);

            nav.push_str(&format!(
                r#"  <li><a href="{}">{}</a></li>"#,
                relative_link,
                link_text
            ));
            nav.push('\n');
        }
    }

    nav.push_str("</ul>\n");

    Ok(nav)
}

/// Compute relative path from one file to another.
fn compute_relative_path(from: &Path, to: &Path) -> String {
    // Count directory depth of the source file.
    let from_depth = from.parent()
        .map(|p| p.components().count())
        .unwrap_or(0);

    // Build "../" prefix based on depth.
    let mut result = String::new();
    for _ in 0..from_depth {
        result.push_str("../");
    }

    // Append target path.
    result.push_str(&to.display().to_string().replace('\\', "/"));

    result
}

/// Process a single markdown file.
fn process_markdown_file(
    config: &BuildConfig,
    md_path: &Path,
    all_files: &[PathBuf],
) -> AnyResult<()> {
    // Read markdown.
    let markdown_content = fs::read_to_string(md_path)?;

    // Extract title.
    let title = markdown::extract_title(&markdown_content)
        .unwrap_or_else(|| "Untitled".to_string());

    // Convert markdown links to HTML links.
    let html_ready_markdown = markdown_content.replace(".md)", ".html)");

    // Convert to HTML.
    let html_content = markdown::markdown_to_html_with_highlighting(&html_ready_markdown);

    // Determine output path.
    let rel_path = md_path.strip_prefix(&config.source_dir)?;
    let html_path = config.output_dir.join(rel_path.with_extension("html"));

    // Create parent directory if needed.
    if let Some(parent) = html_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Build navigation relative to this page.
    let nav_html = build_navigation(&config.source_dir, all_files, md_path)?;

    // Render page.
    let page_data = PageData {
        title,
        content: html_content,
        nav_html,
    };

    let full_html = template::render_page(page_data);

    // Write output.
    fs::write(&html_path, full_html)?;

    println!("  Generated: {}", html_path.display());

    Ok(())
}
