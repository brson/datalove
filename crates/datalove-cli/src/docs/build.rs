use rmx::prelude::*;
use rmx::std::path::{Path, PathBuf};
use rmx::std::fs;

use super::template::{self, PageData};
use super::markdown;

pub struct BuildConfig {
    pub source_dir: PathBuf,
    pub output_dir: PathBuf,
    pub template_path: PathBuf,
}

/// Build the documentation site.
pub fn build_docs(config: BuildConfig) -> AnyResult<()> {
    println!("Building documentation...");
    println!("  Source: {}", config.source_dir.display());
    println!("  Output: {}", config.output_dir.display());
    println!("  Template: {}", config.template_path.display());

    if !config.source_dir.exists() {
        bail!("Source directory does not exist: {}", config.source_dir.display());
    }

    if !config.template_path.exists() {
        bail!("Template file does not exist: {}", config.template_path.display());
    }

    // Create output directory.
    fs::create_dir_all(&config.output_dir)?;

    // Collect all markdown files.
    let md_files = collect_markdown_files(&config.source_dir)?;
    println!("  Found {} markdown files", md_files.len());

    // Process each markdown file.
    for md_path in &md_files {
        process_markdown_file(&config, md_path, &config.template_path)?;
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

/// Process a single markdown file.
fn process_markdown_file(
    config: &BuildConfig,
    md_path: &Path,
    template_path: &Path,
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

    // Render page.
    let page_data = PageData {
        title,
        content: html_content,
    };

    let full_html = template::render_page(template_path, page_data)?;

    // Write output.
    fs::write(&html_path, full_html)?;

    println!("  Generated: {}", html_path.display());

    Ok(())
}
