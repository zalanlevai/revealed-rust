#![feature(trim_prefix_suffix)]

use std::env;
use std::path::PathBuf;
use std::process::{self, Command};

use reveal_rust_driver_cli::{UnstableFlag, UnstableOption};

pub mod build {
    pub const RUST_TOOLCHAIN_VERSION: &str = env!("RUST_TOOLCHAIN_VERSION");
}

fn strip_arg(args: &mut Vec<String>, has_value: bool, short_arg: Option<&str>, long_arg: Option<&str>) {
    let short_arg = short_arg.map(|v| format!("-{v}"));
    let long_arg = long_arg.map(|v| format!("--{v}"));

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let arg_without_prefix = short_arg.as_deref().and_then(|v| arg.strip_prefix(v))
            .or_else(|| long_arg.as_deref().and_then(|v| arg.strip_prefix(v)));

        match arg_without_prefix.map(|v| has_value && !v.trim_start().starts_with("=") && i + 1 < args.len()) {
            Some(true) => { args.splice(i..=(i + 1), []); }
            Some(false) => { args.remove(i); }
            None => i += 1,
        }
    }
}

#[expect(unused)]
fn strip_arg_value_occurrences(args: &mut Vec<String>, short_arg: Option<&str>, long_arg: Option<&str>, value: &str) {
    let short_arg = short_arg.map(|v| format!("-{v}"));
    let long_arg = long_arg.map(|v| format!("--{v}"));

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];

        match () {
            _ if let Some(short_arg_without_prefix) = short_arg.as_deref().and_then(|v| arg.strip_prefix(v)) => {
                let short_arg_inline_value = short_arg_without_prefix.trim_prefix('=');
                match (short_arg_inline_value.is_empty(), args.get(i + 1)) {
                    (false, _) if short_arg_inline_value == value => { args.remove(i); }
                    (true, Some(v)) if v == value => { args.splice(i..=(i + 1), []); }
                    _ => { i += 1; }
                }
            }
            _ if let Some(long_arg_without_prefix) = long_arg.as_deref().and_then(|v| arg.strip_prefix(v)) => {
                let long_arg_inline_value = long_arg_without_prefix.strip_prefix('=');
                match (long_arg_inline_value, args.get(i + 1)) {
                    (Some(v), _) if v == value => { args.remove(i); }
                    (None, Some(v)) if v == value => { args.splice(i..=(i + 1), []); }
                    _ => { i += 1; }
                }
            }
            _ => { i += 1; }
        }
    }
}

#[test]
fn test_strip_arg() {
    let mut args = vec!["--lib".to_owned()];
    strip_arg(&mut args, false, None, Some("lib"));
    assert_eq!(&[] as &[String], &args[..]);

    let mut args = vec!["--lib".to_owned(), "--print".to_owned(), "tests".to_owned()];
    strip_arg(&mut args, false, None, Some("lib"));
    assert_eq!(&["--print".to_owned(), "tests".to_owned()] as &[String], &args[..]);

    let mut args = vec!["--features".to_owned(), "all".to_owned()];
    strip_arg(&mut args, true, None, Some("features"));
    assert_eq!(&[] as &[String], &args[..]);

    let mut args = vec!["--features=all".to_owned()];
    strip_arg(&mut args, true, None, Some("features"));
    assert_eq!(&[] as &[String], &args[..]);

    let mut args = vec!["--features=all".to_owned(), "--metadata-out-root-dir=target/reveal/json".to_owned(), "--print=code".to_owned()];
    strip_arg(&mut args, true, None, Some("features"));
    assert_eq!(&["--metadata-out-root-dir=target/reveal/json".to_owned(), "--print=code".to_owned()] as &[String], &args[..]);
}

#[test]
fn test_strip_arg_value_occurrences() {
    let mut args = vec!["-Z".to_owned(), "write-json-eval-stream".to_owned()];
    strip_arg_value_occurrences(&mut args, Some("Z"), None, "write-json-eval-stream");
    assert_eq!(&[] as &[String], &args[..]);

    let mut args = vec!["-Zwrite-json-eval-stream".to_owned()];
    strip_arg_value_occurrences(&mut args, Some("Z"), None, "write-json-eval-stream");
    assert_eq!(&[] as &[String], &args[..]);

    let mut args = vec!["-Z=write-json-eval-stream".to_owned()];
    strip_arg_value_occurrences(&mut args, Some("Z"), None, "write-json-eval-stream");
    assert_eq!(&[] as &[String], &args[..]);

    let mut args = vec!["-Z".to_owned(), "feature-a".to_owned(), "-Z".to_owned(), "feature-b".to_owned(), "-Z".to_owned(), "feature-c".to_owned()];
    strip_arg_value_occurrences(&mut args, Some("Z"), None, "feature-b");
    assert_eq!(&["-Z".to_owned(), "feature-a".to_owned(), "-Z".to_owned(), "feature-c".to_owned()] as &[String], &args[..]);

    let mut args = vec!["-Z".to_owned(), "feature-b".to_owned(), "-Z".to_owned(), "feature-a".to_owned(), "-Z".to_owned(), "feature-b".to_owned()];
    strip_arg_value_occurrences(&mut args, Some("Z"), None, "feature-b");
    assert_eq!(&["-Z".to_owned(), "feature-a".to_owned()] as &[String], &args[..]);
}

const RUN_UNSTABLE_FLAGS: &[UnstableFlag] = reveal_rust_driver_cli::extend_const_slice!(reveal_rust_driver_cli::UNSTABLE_FLAGS: &[UnstableFlag], &[
    // NOTE: Whenever these change, the `strip_arg_value_occurrences` calls in `run_cargo_with_reveal_rust_driver` have to be updated as well.
]);

const RUN_UNSTABLE_OPTIONS: &[UnstableOption] = reveal_rust_driver_cli::extend_const_slice!(reveal_rust_driver_cli::UNSTABLE_OPTIONS: &[UnstableOption], &[
    // No Cargo-specific unstable options at the moment.
]);

#[cfg(not(windows))]
fn cargo_command_base() -> Command {
    let mut cmd = Command::new("cargo");
    cmd.arg(format!("+{}", build::RUST_TOOLCHAIN_VERSION));
    cmd
}

#[cfg(windows)]
fn cargo_command_base() -> Command {
    let mut cmd = Command::new("rustup");
    cmd.arg("run");
    cmd.arg(build::RUST_TOOLCHAIN_VERSION);
    cmd.arg("cargo");
    cmd
}

fn main() {
    let mut args = env::args().collect::<Vec<_>>();
    // NOTE: We determine whether we are
    //       invoked through Cargo as a subcommand (`cargo reveal`) or as a standalone command (`cargo-reveal`)
    //       based on Cargo's behavior of inserting the subcommand name after the binary path for external subcommands,
    //       see https://doc.rust-lang.org/cargo/reference/external-tools.html#custom-subcommands.
    let bin_name = match args.get(1).map(String::as_str) == Some("reveal") {
        true => {
            args.remove(1);
            "cargo reveal"
        }
        false => "cargo-reveal",
    };

    let matches = reveal_rust_driver_cli::command("cargo-reveal")
        .bin_name(bin_name)
        .about("Reveal the Rust code behind macro expansions, type and name resolution, and elision")
        .author("Zalán Bálint Lévai")
        .version(reveal_rust_driver_cli::VERSION_STR)
        .styles(reveal_rust_driver_cli::STYLES)
        .propagate_version(true)
        .disable_help_flag(true)
        .disable_version_flag(true)
        .next_help_heading("Options")
        .arg(clap::arg!(Z: -Z [FLAG] "Experimental, unstable flags. See `-Z help` for details.").action(clap::ArgAction::Append))
        // Cargo options
        .next_help_heading("Package Selection")
        .arg(clap::arg!(-p --package [PACKAGE] "Test the specified packages.").action(clap::ArgAction::Append))
        .arg(clap::arg!(--workspace "Test all packages in the workspace."))
        .arg(clap::arg!(--exclude [PACKAGE] "Exclude packages from testing.").action(clap::ArgAction::Append))
        .next_help_heading("Target Selection")
        .arg(clap::arg!(--lib "Test only this package's library unit tests."))
        .arg(clap::arg!(--bin [BINARY] "Test only the specified binary. This flag may be specified multiple times.").action(clap::ArgAction::Append))
        .arg(clap::arg!(--bins "Test all binaries."))
        .arg(clap::arg!(--example [EXAMPLE] "Test only the specified example. This flag may be specified multiple times.").action(clap::ArgAction::Append))
        .arg(clap::arg!(--examples "Test all examples."))
        .arg(clap::arg!(--test [TEST] "Test only the specified integration test. This flag may be specified multiple times.").action(clap::ArgAction::Append))
        .arg(clap::arg!(--tests "Test all targets that have the `test = true` manifest flag set."))
        .arg(clap::arg!(--"all-targets" "Test all targets."))
        .next_help_heading("Feature Selection")
        .arg(clap::arg!(-F --features [FEATURES]... "Space or comma separated list of features to activate."))
        .arg(clap::arg!(--"all-features" "Activate all available features."))
        .arg(clap::arg!(--"no-default-features" "Do not activate the `default` feature."))
        .next_help_heading("Compilation Options")
        .arg(clap::arg!(--target [TRIPLE] "Test for the given architecture. The default is the host architecture."))
        .arg(clap::arg!(-r --release "Build artifacts in release mode, with optimizations."))
        .arg(clap::arg!(--profile [PROFILE] "Build artifacts with the specified profile."))
        .arg(clap::arg!(--"target-dir" [TARGET_DIR] "Directory for all generated artifacts.").value_parser(clap::value_parser!(PathBuf)))
        .next_help_heading("Manifest Options")
        .arg(clap::arg!(--"manifest-path" [MANIFEST_PATH] "Path to `Cargo.toml`."))
        .arg(clap::arg!(--locked "Assert that `Cargo.lock` will remain unchanged."))
        .arg(clap::arg!(--offline "Run without accessing the network."))
        .arg(clap::arg!(--frozen "Equivalent to specifying both `--locked` and `--offline`."))
        .next_help_heading("Options")
        // FIXME: Regression; the `help` subcommand can no longer be customized,
        //        so the about text does not match that of the help flags.
        .arg(clap::arg!(-h --help "Print help information; this message or the help of the given subcommand.").action(clap::ArgAction::Help).global(true))
        .arg(clap::arg!(-V --version "Print version information.").action(clap::ArgAction::Version).global(true))
        .get_matches_from(&args);

    let unstable_flags = matches.get_many::<String>("Z").into_iter().flatten().map(String::as_str).collect::<Vec<_>>();
    if unstable_flags.contains(&"help") {
        reveal_rust_driver_cli::print_unstable_flags_help(RUN_UNSTABLE_FLAGS);
        process::exit(0);
    }
    reveal_rust_driver_cli::check_unstable_flags(&unstable_flags, RUN_UNSTABLE_FLAGS);
    if !unstable_flags.contains(&"unstable-options") {
        reveal_rust_driver_cli::check_unstable_options(&matches, RUN_UNSTABLE_OPTIONS);
    }

    // Remove binary path from argument list for processing.
    let mut args = &args[1..];
    // Remove passed arguments from argument list for processing.
    if let Some(rest_idx) = args.iter().position(|arg| arg == "--") {
        args = &args[..rest_idx];
    }

    let mut cargo_invocation = process_cargo_args(&args, &matches);
    // NOTE: Our target directory lives within the real target directory,
    //       whether specified explicitly through `--target-dir`, or implicitly chosen by Cargo.
    cargo_invocation.target_dir.push("reveal");

    run_cargo_with_reveal_rust_driver(&cargo_invocation, &matches, &unstable_flags);
}

struct CargoInvocation<'a> {
    cargo_args: Vec<&'a str>,
    non_cargo_args: Vec<String>,
    target_dir: PathBuf,
    #[expect(unused)]
    explicit_targetings_count: usize,
}

fn process_cargo_args<'a>(args: &'a [String], matches: &'a clap::ArgMatches) -> CargoInvocation<'a> {
    let mut cargo_args = vec![];
    let mut non_cargo_args = args.to_vec();

    // NOTE: `--color` may be interpreted by the wrapper invoked through Cargo, so we leave it in the non-Cargo args.
    if let Some(color) = matches.get_one::<String>("color") {
        cargo_args.extend(["--color", color]);
    }

    let mut metadata_cmd = cargo_metadata::MetadataCommand::new();

    if let Some(manifest_path) = matches.get_one::<String>("manifest-path") {
        metadata_cmd.manifest_path(manifest_path);
        cargo_args.extend(["--manifest-path", manifest_path]);
        strip_arg(&mut non_cargo_args, true, None, Some("manifest-path"));
    }

    // Package selection.
    if let Some(packages) = matches.get_many::<String>("package") {
        for package in packages { cargo_args.extend(["--package", package]); }
        strip_arg(&mut non_cargo_args, true, Some("p"), Some("package"));
    }
    if matches.get_flag("workspace") {
        cargo_args.push("--workspace");
        strip_arg(&mut non_cargo_args, false, None, Some("workspace"));
    }
    if let Some(packages) = matches.get_many::<String>("exclude") {
        for package in packages { cargo_args.extend(["--exclude", package]); }
        strip_arg(&mut non_cargo_args, true, None, Some("exclude"));
    }

    // Feature selection.
    if let Some(features) = matches.get_many::<String>("features") {
        metadata_cmd.features(cargo_metadata::CargoOpt::SomeFeatures(features.clone().map(ToOwned::to_owned).collect()));
        for feature in features { cargo_args.extend(["--features", feature]); }
        strip_arg(&mut non_cargo_args, true, Some("F"), Some("features"));
    }
    if matches.get_flag("all-features") {
        metadata_cmd.features(cargo_metadata::CargoOpt::AllFeatures);
        cargo_args.push("--all-features");
        strip_arg(&mut non_cargo_args, false, None, Some("all-features"));
    }
    if matches.get_flag("no-default-features") {
        metadata_cmd.features(cargo_metadata::CargoOpt::NoDefaultFeatures);
        cargo_args.push("--no-default-features");
        strip_arg(&mut non_cargo_args, false, None, Some("no-default-features"));
    }

    let cargo_metadata = metadata_cmd.exec().expect("could not retrieve Cargo metadata");

    let target_dir = matches.get_one::<PathBuf>("target-dir").cloned().unwrap_or_else(|| cargo_metadata.target_directory.clone().into_std_path_buf());
    strip_arg(&mut non_cargo_args, true, None, Some("target-dir"));

    if let Some(target) = matches.get_one::<String>("target") {
        cargo_args.extend(["--target", target]);
        strip_arg(&mut non_cargo_args, true, None, Some("target"));
    }

    if matches.get_flag("release") {
        cargo_args.push("--release");
        strip_arg(&mut non_cargo_args, false, Some("r"), Some("release"));
    }
    if let Some(profile) = matches.get_one::<String>("profile") {
        cargo_args.extend(["--profile", profile]);
        strip_arg(&mut non_cargo_args, true, None, Some("profile"));
    }

    // Target selection.
    let mut explicit_targetings_count = 0;
    if matches.get_flag("lib") {
        explicit_targetings_count += 1;
        cargo_args.push("--lib");
        strip_arg(&mut non_cargo_args, false, None, Some("lib"));
    }
    if let Some(bins) = matches.get_many::<String>("bin") {
        for bin in bins {
            explicit_targetings_count += 1;
            cargo_args.extend(["--bin", bin]);
        }
        strip_arg(&mut non_cargo_args, true, None, Some("bin"));
    }
    if matches.get_flag("bins") {
        explicit_targetings_count  += 1;
        cargo_args.push("--bins");
        strip_arg(&mut non_cargo_args, false, None, Some("bins"));
    }
    if let Some(examples) = matches.get_many::<String>("example") {
        for example in examples {
            explicit_targetings_count += 1;
            cargo_args.extend(["--example", example]);
        }
        strip_arg(&mut non_cargo_args, true, None, Some("example"));
    }
    if matches.get_flag("examples") {
        explicit_targetings_count += 1;
        cargo_args.push("--examples");
        strip_arg(&mut non_cargo_args, false, None, Some("examples"));
    }
    if let Some(tests) = matches.get_many::<String>("test") {
        for test in tests {
            explicit_targetings_count += 1;
            cargo_args.extend(["--test", test]);
        }
        strip_arg(&mut non_cargo_args, true, None, Some("test"));
    }
    if matches.get_flag("tests") {
        explicit_targetings_count += 1;
        cargo_args.push("--tests");
        strip_arg(&mut non_cargo_args, false, None, Some("tests"));
    }
    if matches.get_flag("all-targets") {
        explicit_targetings_count += 1;
        cargo_args.push("--all-targets");
        strip_arg(&mut non_cargo_args, false, None, Some("all-targets"));
    }

    if matches.get_flag("locked") {
        cargo_args.push("--locked");
        strip_arg(&mut non_cargo_args, false, None, Some("locked"));
    }
    if matches.get_flag("offline") {
        cargo_args.push("--offline");
        strip_arg(&mut non_cargo_args, false, None, Some("offline"));
    }
    if matches.get_flag("frozen") {
        cargo_args.push("--frozen");
        strip_arg(&mut non_cargo_args, false, None, Some("frozen"));
    }

    CargoInvocation { cargo_args, non_cargo_args, target_dir, explicit_targetings_count }
}

fn run_cargo_with_reveal_rust_driver(cargo_invocation: &CargoInvocation, _matches: &clap::ArgMatches, _unstable_flags: &[&str]) {
    let reveal_rust_args = cargo_invocation.non_cargo_args.clone();

    let mut cmd = cargo_command_base();
    cmd.arg("check");

    let cargo_verbosity = env::var("CARGO_VERBOSITY").ok().and_then(|s| s.parse::<usize>().ok()).unwrap_or_default();
    cmd.args((0..cargo_verbosity).map(|_| "-v"));

    cmd.arg("--target-dir");
    cmd.arg(&cargo_invocation.target_dir);
    cmd.env("REVEAL_RUST_TARGET_DIR_ROOT", &cargo_invocation.target_dir);

    cmd.args(&cargo_invocation.cargo_args);

    let mut path = env::current_exe().expect("current executable path invalid");
    path.set_file_name("reveal-rust-driver");
    if cfg!(windows) { path.set_extension("exe"); }
    cmd.env("RUSTC_WORKSPACE_WRAPPER", path);

    cmd.env("REVEAL_RUST_ENCODED_ARGS", reveal_rust_args.join("\x1F"));

    let exit_status = cmd
        .spawn().expect("failed to run Cargo")
        .wait().expect("failed to run Cargo");

    let exit_code = exit_status.code();
    if exit_code != Some(0) && exit_code != Some(101) {
        process::exit(exit_code.unwrap_or(-1));
    }
}
