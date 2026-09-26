use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};

use rview::diff::parse_name_status;
use rview::git;
use rview::report::Report;

/// Organize the changes between two git revisions into review categories.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {
    /// Base revision to compare against (compared via `<BASE>...<HEAD>`).
    #[arg(default_value = "main")]
    base: String,

    /// Head revision.
    #[arg(long, default_value = "HEAD")]
    head: String,

    /// Path to the git repository.
    #[arg(short = 'C', long = "repo", default_value = ".")]
    repo: PathBuf,

    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Json,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let raw = git::diff_name_status(&cli.repo, &cli.base, &cli.head)?;
    let changes = parse_name_status(&raw).context("failed to parse git diff output")?;
    let report = Report::build(&cli.base, &cli.head, changes);

    match cli.format {
        Format::Text => print!("{}", report.render_text()),
        Format::Json => println!("{}", report.render_json()?),
    }

    Ok(())
}
