use clap::Parser;

/// Install and update Tempo releases.
#[derive(Parser)]
#[command(name = "tempoup", about, disable_version_flag = true)]
pub(crate) struct Cli {
    /// Install a specific Tempo version, such as v1.13.2
    #[arg(
        short = 'i',
        long = "install",
        value_name = "VERSION",
        conflicts_with = "update"
    )]
    pub version: Option<String>,

    /// Update tempoup itself
    #[arg(short = 'U', long, conflicts_with = "version")]
    pub update: bool,

    /// Skip provenance or signature verification; checksums remain mandatory
    #[arg(long)]
    pub unsafe_skip_verify: bool,

    /// Print the tempoup version
    #[arg(short = 'v', long = "version")]
    pub print_version: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_and_existing_flags_are_valid() {
        Cli::command().debug_assert();
        let cli = Cli::try_parse_from(["tempoup", "-i", "v1.2.3", "--unsafe-skip-verify"]).unwrap();
        assert_eq!(cli.version.as_deref(), Some("v1.2.3"));
        assert!(cli.unsafe_skip_verify);

        let cli = Cli::try_parse_from(["tempoup", "-U"]).unwrap();
        assert!(cli.update);

        let cli = Cli::try_parse_from(["tempoup", "-v"]).unwrap();
        assert!(cli.print_version);
    }
}
