#![feature(rustc_private)]
extern crate rustc_data_structures;
extern crate rustc_driver;
extern crate rustc_interface;
extern crate rustc_session;

use std::env;
use std::path::{Path, PathBuf};
use std::process;

use reveal_rust_driver::config::{self, Config};
use reveal_rust_driver_cli::{UnstableFlag, UnstableOption};
use rustc_interface::Config as CompilerConfig;
use rustc_session::EarlyDiagCtxt;
use rustc_session::config::ErrorOutputType;

struct DefaultCallbacks;
impl rustc_driver::Callbacks for DefaultCallbacks {}

/// This is different from `DefaultCallbacks` in that it will instruct Cargo to track the value of the `REVEAL_RUST_ARGS`
/// environment variable.
struct RustcCallbacks {
    reveal_rust_args: Option<String>,
}

impl rustc_driver::Callbacks for RustcCallbacks {
    fn config(&mut self, config: &mut CompilerConfig) {
        let args = self.reveal_rust_args.take();
        config.track_state = Some(Box::new(move |sess| {
            reveal_rust_driver::compiler::track_invocation_fingerprint(sess, args.as_deref());
        }));
    }
}

const UNSTABLE_FLAGS: &[UnstableFlag] = reveal_rust_driver_cli::extend_const_slice!(reveal_rust_driver_cli::UNSTABLE_FLAGS: &[UnstableFlag], &[
    // No reveal-rust-driver-specific unstable flags at the moment.
]);

const UNSTABLE_OPTIONS: &[UnstableOption] = reveal_rust_driver_cli::extend_const_slice!(reveal_rust_driver_cli::UNSTABLE_OPTIONS: &[UnstableOption], &[
    // No reveal-rust-driver-specific unstable options at the moment.
]);

const BUG_REPORT_URL: &str = "https://github.com/zalanlevai/revealed-rust/issues/new";

pub fn main() -> process::ExitCode {
    let early_dcx = EarlyDiagCtxt::new(ErrorOutputType::default());
    let mut args = rustc_driver::args::raw_args(&early_dcx);

    rustc_driver::init_rustc_env_logger(&early_dcx);

    rustc_driver::install_ice_hook(BUG_REPORT_URL, |_dcx| {});

    // NOTE: When being invoked by Cargo through RUSTC_WRAPPER / RUSTC_WORKSPACE_WRAPPER,
    //       we are passed the path to rustc as the first argument.
    //       This is ignored, since we are using rustc_driver directly to invoke the compiler.
    let rustc_wrapper = args.get(1).map(Path::new).and_then(Path::file_stem) == Some("rustc".as_ref());
    if rustc_wrapper { args.remove(1); }

    // Make `reveal-rust-driver --rustc` work like a subcommand that passes further args to rustc directly.
    // For example, `reveal-rust-driver --rustc --version` will print
    // the rustc version of rustc_driver that reveal-rust-driver is linked against.
    // This is distinct from `reveal-rust-driver --version`, which prints the version string of reveal-rust-driver.
    if let Some(marker_arg_position) = args.iter().position(|arg| arg == "--rustc") {
        args.remove(marker_arg_position);
        args[0] = "rustc".to_owned();

        return rustc_driver::catch_with_exit_code(|| {
            rustc_driver::run_compiler(&args, &mut DefaultCallbacks)
        });
    }

    // Parser for non-rustc, reveal-rust-specific arguments provided through REVEAL_RUST_ARGS / REVEAL_RUST_ENCODED_ARGS.
    let reveal_rust_command = reveal_rust_driver_cli::command("reveal-rust-driver")
        .about("Reveal expanded Rust code using a rustc-compatible interface.")
        .author("Zalán Bálint Lévai")
        .version(reveal_rust_driver_cli::VERSION_STR)
        .styles(reveal_rust_driver_cli::STYLES)
        .override_usage(color_print::cstr!("<bright-blue,bold>[REVEAL_RUST_ARGS=\"<<REVEAL_RUST_OPTIONS>>\"] reveal-rust-driver [<<RUSTC_PATH>>] [--rustc] [<<RUSTC_OPTIONS>>]</>"))
        .no_binary_name(true)
        .next_help_heading("Options")
        .arg(clap::arg!(Z: -Z [FLAG] "Experimental, unstable flags. See `-Z help` for details.").action(clap::ArgAction::Append));

    if !rustc_wrapper {
        // Forward help and version information invocations to their reveal-rust-driver counterparts for convenience.
        // NOTE: The exit calls are actually unreachable, but it is good to have them just in case.
        if args.iter().any(|arg| arg == "-V" || arg == "--version") {
            let _ = reveal_rust_command.get_matches_from(["--version"]);
            return process::ExitCode::SUCCESS;
        }
        if args.iter().any(|arg| arg == "-h") {
            let _ = reveal_rust_command.get_matches_from(["-h"]);
            return process::ExitCode::SUCCESS;
        }
        if args.iter().any(|arg| arg == "--help") {
            let _ = reveal_rust_command.get_matches_from(["--help"]);
            return process::ExitCode::SUCCESS;
        }
    }

    // HACK: This is an imperfect list of possible info queries, but matches what clippy-driver and cargo-miri do.
    let info_query = args.iter().any(|arg| arg == "-vV" || arg.starts_with("--print"));

    let cargo_invocation = rustc_session::utils::was_invoked_from_cargo();
    let primary_package = env::var("CARGO_PRIMARY_PACKAGE").is_ok();

    let reveal_rust_args = None
        .or_else(|| env::var("REVEAL_RUST_ENCODED_ARGS").ok().map(|args| args.split('\x1F').map(ToOwned::to_owned).collect::<Vec<_>>()))
        .or_else(|| env::var("REVEAL_RUST_ARGS").ok().map(|args| args.split(' ').map(ToOwned::to_owned).collect::<Vec<_>>()));
    let reveal_rust_args_str = reveal_rust_args.as_ref().map(|reveal_rust_args| reveal_rust_args.join(" "));

    // Fall back to a rustc invocation if reveal-rust is not "enabled" for the given crate based on invocation.
    if info_query || (cargo_invocation && !primary_package) {
        return rustc_driver::catch_with_exit_code(|| {
            rustc_driver::run_compiler(&args, &mut RustcCallbacks { reveal_rust_args: reveal_rust_args_str })
        });
    }

    let reveal_rust_arg_matches = reveal_rust_command.get_matches_from(reveal_rust_args.unwrap_or_default());

    let unstable_flags = reveal_rust_arg_matches.get_many::<String>("Z").into_iter().flatten().map(String::as_str).collect::<Vec<_>>();
    if unstable_flags.contains(&"help") {
        reveal_rust_driver_cli::print_unstable_flags_help(UNSTABLE_FLAGS);
        return process::ExitCode::SUCCESS;
    }
    reveal_rust_driver_cli::check_unstable_flags(&unstable_flags, UNSTABLE_FLAGS);
    if !unstable_flags.contains(&"unstable-options") {
        reveal_rust_driver_cli::check_unstable_options(&reveal_rust_arg_matches, UNSTABLE_OPTIONS);
    }

    rustc_driver::catch_with_exit_code(|| {
        let (Some(compiler_config), _crate_types) = reveal_rust_driver::compiler::parse_compiler_args(&args) else {
            early_dcx.early_fatal("no compiler configuration was generated");
        };

        let _early_dcx = EarlyDiagCtxt::new(compiler_config.opts.error_format);

        let reveal_rust_target_dir_root = env::var("REVEAL_RUST_TARGET_DIR_ROOT").ok().map(PathBuf::from);

        let verbosity = reveal_rust_arg_matches.get_count("verbose");
        let report_timings = reveal_rust_arg_matches.get_flag("timings");

        let unstable_flag_opts = config::UnstableFlags {
            verify_ast_lowering: unstable_flags.contains(&"verify-ast-lowering"),
        };

        let config = Config {
            compiler_config,
            invocation_fingerprint: reveal_rust_args_str,
            reveal_rust_target_dir_root,
            opts: config::Options {
                verbosity,
                report_timings,

                unstable_flags: unstable_flag_opts,
            },
        };

        reveal_rust_driver::run(config).unwrap();
    })
}
