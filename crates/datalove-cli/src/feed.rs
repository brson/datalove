use rmx::prelude::*;

use rmx::std::collections::BTreeMap;
use rmx::std::fs;
use rmx::std::path::Path;
use rmx::regex::Regex;
use rmx::tera;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Post {
    pub slug: String,
    pub date: String,
    pub category: String,
    pub title: String,
    pub summary: String,
    pub content_md: String,
    pub content_html: String,
}

#[derive(Debug, Clone, Copy)]
pub enum Category {
    News,
    Release,
    Dev,
}

impl Category {
    fn from_str(s: &str) -> AnyResult<Self> {
        match s {
            "news" => Ok(Category::News),
            "release" => Ok(Category::Release),
            "dev" => Ok(Category::Dev),
            _ => bail!("invalid category: {}", s),
        }
    }

    fn as_str(&self) -> &str {
        match self {
            Category::News => "news",
            Category::Release => "release",
            Category::Dev => "dev",
        }
    }
}

/// Parse all posts from the posts directory.
///
/// Posts are named YYYYMMDDA-{slug}.md where A is a letter for ordering same-day posts.
pub fn parse_posts(posts_dir: &Path) -> AnyResult<Vec<Post>> {
    let filename_regex = Regex::new(r"^(\d{8})([A-Z])-(.+)\.md$")?;

    let mut posts = Vec::new();

    if !posts_dir.exists() {
        return Ok(posts);
    }

    let mut entries: Vec<_> = fs::read_dir(posts_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .collect();

    // Sort by filename descending (newest first).
    entries.sort_by(|a, b| b.file_name().cmp(&a.file_name()));

    for entry in entries {
        let path = entry.path();

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow!("invalid filename"))?;

        let Some(captures) = filename_regex.captures(filename) else {
            continue;
        };

        let date_str = captures.get(1).unwrap().as_str();
        let slug = captures.get(3).unwrap().as_str().to_string();

        // Parse date from YYYYMMDD format.
        let date = rmx::chrono::NaiveDate::parse_from_str(date_str, "%Y%m%d")
            .context("parsing date")?;

        let content = fs::read_to_string(&path)?;
        let (frontmatter, content_md) = parse_frontmatter(&content)?;

        let title = frontmatter
            .get("title")
            .ok_or_else(|| anyhow!("missing title in frontmatter"))?
            .clone();

        let summary = frontmatter
            .get("summary")
            .ok_or_else(|| anyhow!("missing summary in frontmatter"))?
            .clone();

        let category_str = frontmatter
            .get("category")
            .ok_or_else(|| anyhow!("missing category in frontmatter"))?;

        let category = Category::from_str(category_str)?;

        // Convert markdown to HTML with GFM extensions.
        let mut options = rmx::comrak::Options::default();
        options.extension.table = true;
        options.extension.strikethrough = true;
        options.extension.autolink = true;
        options.render.unsafe_ = true;
        let content_html = rmx::comrak::markdown_to_html(&content_md, &options);

        posts.push(Post {
            slug,
            date: date.format("%Y-%m-%d").to_string(),
            category: category.as_str().to_string(),
            title,
            summary,
            content_md,
            content_html,
        });
    }

    Ok(posts)
}

fn parse_frontmatter(content: &str) -> AnyResult<(BTreeMap<String, String>, String)> {
    let mut lines = content.lines();

    // First line should be "---".
    let first = lines.next().ok_or_else(|| anyhow!("empty file"))?;
    if first.trim() != "---" {
        bail!("missing frontmatter start");
    }

    let mut frontmatter = BTreeMap::new();
    let mut body_lines = Vec::new();
    let mut in_frontmatter = true;

    for line in lines {
        if in_frontmatter {
            if line.trim() == "---" {
                in_frontmatter = false;
                continue;
            }

            // Parse key: value.
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim().to_string();
                let value = value.trim().trim_matches('"').to_string();
                frontmatter.insert(key, value);
            }
        } else {
            body_lines.push(line);
        }
    }

    let body = body_lines.join("\n");

    Ok((frontmatter, body))
}

/// Generate the posts feed page.
pub fn generate_feed_page(
    posts: &[Post],
    tera: &tera::Tera,
    out_dir: &Path,
) -> AnyResult<()> {
    let mut context = tera::Context::new();
    context.insert("posts", posts);

    let rendered = tera.render("posts-template.html", &context)?;

    let posts_path = out_dir.join("posts.html");
    fs::write(&posts_path, rendered)?;
    println!("posts.html");

    Ok(())
}

/// Generate RSS feed.
pub fn generate_rss(posts: &[Post], out_dir: &Path, base_url: &str) -> AnyResult<()> {
    let mut rss = String::new();
    rss.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    rss.push('\n');
    rss.push_str(r#"<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom">"#);
    rss.push('\n');
    rss.push_str("  <channel>\n");
    rss.push_str("    <title>Datalove</title>\n");
    rss.push_str(&format!("    <link>{}/posts.html</link>\n", base_url));
    rss.push_str("    <description>Updates from Datalove</description>\n");
    rss.push_str(&format!("    <atom:link href=\"{}/posts.xml\" rel=\"self\" type=\"application/rss+xml\" />\n", base_url));

    for post in posts {
        rss.push_str("    <item>\n");
        rss.push_str(&format!("      <title>{}</title>\n", escape_xml(&post.title)));
        rss.push_str(&format!("      <link>{}/posts.html#{}</link>\n", base_url, post.slug));
        rss.push_str(&format!("      <guid>{}/posts.html#{}</guid>\n", base_url, post.slug));
        let date = rmx::chrono::NaiveDate::parse_from_str(&post.date, "%Y-%m-%d").unwrap();
        rss.push_str(&format!("      <pubDate>{}</pubDate>\n", format_rfc822_date(date)));
        rss.push_str(&format!("      <category>{}</category>\n", &post.category));
        rss.push_str(&format!("      <description><![CDATA[{}]]></description>\n", post.content_html));
        rss.push_str("    </item>\n");
    }

    rss.push_str("  </channel>\n");
    rss.push_str("</rss>\n");

    let rss_path = out_dir.join("posts.xml");
    fs::write(&rss_path, rss)?;
    println!("posts.xml");

    Ok(())
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn format_rfc822_date(date: rmx::chrono::NaiveDate) -> String {
    use rmx::chrono::{DateTime, Utc, NaiveTime};

    // Convert to DateTime at midnight UTC.
    let time = NaiveTime::from_hms_opt(0, 0, 0).unwrap();
    let datetime = date.and_time(time);
    let datetime = DateTime::<Utc>::from_naive_utc_and_offset(datetime, Utc);
    datetime.format("%a, %d %b %Y %H:%M:%S %z").to_string()
}
