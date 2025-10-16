use rmx::prelude::*;
use rmx::clap;
use rmx::std::path::PathBuf;

mod template;
mod markdown;
mod build;

pub use build::{build_docs, BuildConfig};

#[derive(clap::Args)]
pub struct DocsCommand {
    #[command(subcommand)]
    pub cmd: DocsSubcommand,
}

#[derive(clap::Subcommand)]
pub enum DocsSubcommand {
    /// Build static documentation site.
    Build(BuildCommand),
    /// Initialize documentation structure.
    Init(InitCommand),
}

#[derive(clap::Args)]
pub struct BuildCommand {
    /// Output directory for generated HTML.
    #[arg(long, short, default_value = "target/www/docs")]
    pub output: PathBuf,

    /// Source directory for markdown files.
    #[arg(long, short, default_value = "docs")]
    pub source: PathBuf,
}

#[derive(clap::Args)]
pub struct InitCommand {
    /// Directory to initialize documentation in.
    #[arg(default_value = "docs")]
    pub path: PathBuf,
}

impl DocsCommand {
    pub fn run(&self) -> AnyResult<()> {
        match &self.cmd {
            DocsSubcommand::Build(cmd) => cmd.run(),
            DocsSubcommand::Init(cmd) => cmd.run(),
        }
    }
}

impl BuildCommand {
    pub fn run(&self) -> AnyResult<()> {
        let config = BuildConfig {
            source_dir: self.source.clone(),
            output_dir: self.output.clone(),
        };

        build_docs(config)?;

        println!("Documentation built successfully!");
        println!("Output: {}", self.output.display());

        Ok(())
    }
}

impl InitCommand {
    pub fn run(&self) -> AnyResult<()> {
        if self.path.exists() {
            bail!("Directory already exists: {}", self.path.display());
        }

        println!("Initializing documentation structure in: {}", self.path.display());
        println!("This feature is not yet implemented.");
        println!("For now, manually create your docs/ directory structure.");

        Ok(())
    }
}
