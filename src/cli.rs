use clap::Parser;

/// Command-line arguments. --symbol_list and --secrets_dir are required,
/// same as the C++ feed's getopt_long setup (missing either one in the
/// C++ version prints help and exits; clap does the same automatically
/// for a required field with no default).
///
/// --no_snapshots is deliberately the opposite polarity of the C++
/// version's --snapshots: the C++ feed defaults to NOT processing
/// snapshots (process_snapshots = false, --snapshots turns it on).
/// This version defaults to processing snapshots, and --no_snapshots
/// turns that off.
#[derive(Parser, Debug)]
#[command(rename_all = "snake_case")]
pub struct Cli {
    /// Comma-separated list of symbols, e.g. "BTC-USD,ETH-USD"
    #[arg(long, required = true, value_delimiter = ',')]
    pub symbol_list: Vec<String>,

    /// Directory containing api_key.txt and api_secret.pem
    #[arg(long)]
    pub secrets_dir: String,

    /// Skip processing snapshot messages -- only apply incremental updates
    #[arg(long)]
    pub no_snapshots: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_symbol_list_and_secrets_dir() {
        let cli = Cli::try_parse_from([
            "coinbase-advanced-l2-rs",
            "--symbol_list",
            "BTC-USD,ETH-USD",
            "--secrets_dir",
            "/some/path",
        ])
        .unwrap();

        assert_eq!(
            cli.symbol_list,
            vec!["BTC-USD".to_string(), "ETH-USD".to_string()]
        );
        assert_eq!(cli.secrets_dir, "/some/path");
        assert!(!cli.no_snapshots);
    }

    #[test]
    fn no_snapshots_flag_sets_true() {
        let cli = Cli::try_parse_from([
            "coinbase-advanced-l2-rs",
            "--symbol_list",
            "BTC-USD",
            "--secrets_dir",
            "/some/path",
            "--no_snapshots",
        ])
        .unwrap();

        assert!(cli.no_snapshots);
    }

    #[test]
    fn missing_secrets_dir_fails() {
        let result = Cli::try_parse_from([
            "coinbase-advanced-l2-rs",
            "--symbol_list",
            "BTC-USD",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn missing_symbol_list_fails() {
        let result = Cli::try_parse_from([
            "coinbase-advanced-l2-rs",
            "--secrets_dir",
            "/some/path",
        ]);
        assert!(result.is_err());
    }
}
