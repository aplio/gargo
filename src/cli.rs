use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliMode {
    RunEditor,
    CheckUpgrade,
    Update,
}

#[derive(Debug, Parser)]
#[command(name = "gargo")]
pub struct Cli {
    /// Check whether a newer version is available.
    #[arg(long, conflicts_with = "update")]
    pub check: bool,

    /// Download and replace the current binary with the latest release.
    #[arg(long, conflicts_with = "check")]
    pub update: bool,

    /// Optional file or directory to open.
    #[arg(value_name = "PATH", conflicts_with_all = ["check", "update"])]
    pub path: Option<PathBuf>,
}

impl Cli {
    pub fn mode(&self) -> CliMode {
        if self.check {
            CliMode::CheckUpgrade
        } else if self.update {
            CliMode::Update
        } else {
            CliMode::RunEditor
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, CliMode};

    #[test]
    fn parses_check_flag() {
        let cli = Cli::try_parse_from(["gargo", "--check"]).expect("parse --check");
        assert_eq!(cli.mode(), CliMode::CheckUpgrade);
        assert!(cli.path.is_none());
    }

    #[test]
    fn parses_update_flag() {
        let cli = Cli::try_parse_from(["gargo", "--update"]).expect("parse --update");
        assert_eq!(cli.mode(), CliMode::Update);
        assert!(cli.path.is_none());
    }

    #[test]
    fn parses_positional_path() {
        let cli = Cli::try_parse_from(["gargo", "README.md"]).expect("parse path");
        assert_eq!(cli.mode(), CliMode::RunEditor);
        assert_eq!(cli.path.as_deref(), Some(std::path::Path::new("README.md")));
    }

    #[test]
    fn parses_separator_for_path_like_flag() {
        let cli = Cli::try_parse_from(["gargo", "--", "--update"]).expect("parse -- separator");
        assert_eq!(cli.mode(), CliMode::RunEditor);
        assert_eq!(cli.path.as_deref(), Some(std::path::Path::new("--update")));
    }

    #[test]
    fn rejects_conflicting_flags() {
        let err = Cli::try_parse_from(["gargo", "--check", "--update"]).expect_err("conflict");
        let message = err.to_string();
        assert!(
            message.contains("cannot be used with"),
            "unexpected clap error: {message}"
        );
    }

    #[test]
    fn rejects_path_with_update_flag() {
        let err = Cli::try_parse_from(["gargo", "--update", "README.md"])
            .expect_err("path conflicts with --update");
        let message = err.to_string();
        assert!(
            message.contains("cannot be used with"),
            "unexpected clap error: {message}"
        );
    }
}
