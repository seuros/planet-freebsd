use std::assert_matches;
use std::path::Path;

use usage_rs::test as harness;

use super::*;

/// The parsed CLI, or the error text a user would see.
fn parse<const N: usize>(args: [&str; N]) -> Result<Planet, String> {
    let words = harness::argv(args);
    harness::parse(Planet::spec(), &words.words(), Planet::parse_from)
}

#[test]
fn spec_round_trips() {
    let _spec: usage_parser::Spec = Planet::to_kdl().parse().unwrap();
}

#[test]
fn build_has_cron_friendly_defaults() {
    let cli = parse(["build"]).unwrap();
    assert_eq!(cli.config, Path::new("planet.toml"));
    assert_matches!(cli.command, Commands::Build(Build { offline: false, force: false, ref out, ref state })
        if out == Path::new("public") && state == Path::new("state.json"));
}

#[test]
fn config_is_accepted_after_the_subcommand() {
    let cli = parse(["check", "-c", "other.toml"]).unwrap();
    assert_eq!(cli.config, Path::new("other.toml"));
    assert_matches!(cli.command, Commands::Check(_));
}

#[test]
fn unknown_flag_is_rejected() {
    let message = parse(["build", "--wat"]).unwrap_err();
    assert!(message.contains("--wat"), "{message}");
}
