#![feature(decl_macro)]

use std::process;

pub macro extend_const_slice($base:path: &[$ty:ty], $ext:expr) {
    &const {
        const EXT: &[$ty] = $ext;
        const LEN: usize = $base.len() + EXT.len();
        let mut dst: [$ty; LEN] = match ($base.len(), EXT.len()) {
            // HACK: We must generate a value of type `[$ty; LEN]` here
            //       regardless of whether it will be used during const evaluation
            //       (i.e., even if `LEN != 0`).
            // SAFETY: For the only case this will be used (`LEN == 0`),
            //         the value will always be a `[_; 0]`, i.e., an empty `[]`.
            (0, 0) => unsafe { std::mem::transmute([0u8; std::mem::size_of::<$ty>() * LEN]) }
            (0, _) => [EXT[0]; LEN],
            _ => [$base[0]; LEN],
        };
        let mut i = 0;
        while i < $base.len() {
            dst[i] = $base[i];
            i += 1;
        }
        i = 0;
        while i < EXT.len() {
            dst[i + $base.len()] = EXT[i];
            i += 1;
        }
        dst
    }
}

#[derive(Copy, Clone, Debug)]
pub struct UnstableFlag {
    pub name: &'static str,
    pub help: Option<&'static str>,
}

impl UnstableFlag {
    pub const fn new(name: &'static str, help: Option<&'static str>) -> Self {
        Self { name, help }
    }
}

pub fn print_unstable_flags_help(flags: &[UnstableFlag]) {
    color_print::cprintln!("<bright-green,bold>Unstable Flags:</>");
    let w_name = flags.iter().map(|flag| flag.name.len()).max().unwrap_or_default();
    color_print::cprintln!("  <bright-blue,bold>-Z {:<w_name$}</>  Print help information.", "help");
    for flag in flags {
        color_print::cprint!("  <bright-blue,bold>-Z {:<w_name$}</>", flag.name);
        if let Some(help) = &flag.help {
            color_print::cprint!("  {}", help);
        }
        color_print::cprintln!("");
    }
}

pub fn check_unstable_flags(provided_flags: &[&str], known_flags: &[UnstableFlag]) {
    for flag in provided_flags {
        if !known_flags.iter().any(|f| f.name == *flag) {
            color_print::ceprintln!("<red,bold>error</>: unknown unstable flag `{}`", flag);
            process::exit(1);
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct UnstableOption {
    pub name: &'static str,
    pub value: Option<&'static str>,
}

impl UnstableOption {
    pub const fn new(name: &'static str, value: Option<&'static str>) -> Self {
        Self { name, value }
    }
}

pub fn check_unstable_options(matches: &clap::ArgMatches, options: &[UnstableOption]) {
    for option in options {
        if let None | Some(clap::parser::ValueSource::DefaultValue) = matches.value_source(option.name) { continue; }

        match option.value {
            None => {
                color_print::ceprintln!("<red,bold>error</>: the `--{}` flag is unstable, pass `-Z unstable-options` to enable it", option.name);
                process::exit(1);
            }
            Some(value) if let Some(mut values) = matches.get_many::<String>(option.name) && values.any(|v| v == value) => {
                color_print::ceprintln!("<red,bold>error</>: the `--{}={}` option is unstable, pass `-Z unstable-options` to enable it", option.name, value);
                process::exit(1);
            }
            _ => {}
        }
    }
}

pub macro opts(
    $all:ident, $possible_values_vis:vis $possible_values:ident where
    $($(#[$attr:meta])* $ident:ident = $name:expr; $([$help:expr])?)*
) {
    $($(#[$attr])* pub const $ident: &str = $name;)*
    pub const $all: &[&str] = &[$($(#[$attr])* $ident,)*];

    $possible_values_vis fn $possible_values() -> Vec<clap::builder::PossibleValue> {
        vec![
            clap::builder::PossibleValue::new("all"),
            $($(#[$attr])* clap::builder::PossibleValue::new($name)$(.help($help))?,)*
        ]
    }
}

pub macro exclusive_opts(
    $possible_values_vis:vis $possible_values:ident where
    $($(#[$attr:meta])* $ident:ident = $name:expr; $([$help:expr])?)*
) {
    $($(#[$attr])* pub const $ident: &str = $name;)*

    $possible_values_vis fn $possible_values() -> Vec<clap::builder::PossibleValue> {
        vec![$($(#[$attr])* clap::builder::PossibleValue::new($name)$(.help($help))?,)*]
    }
}

pub const UNSTABLE_FLAGS: &[UnstableFlag] = &[
    UnstableFlag::new("unstable-options", Some("Enable the use of unstable options.")),
    // Permanently unstable options.
    UnstableFlag::new("verify-ast-lowering", Some("Verify whether all AST nodes are mapped to their HIR counterparts.")),
];

pub const UNSTABLE_OPTIONS: &[UnstableOption] = &[
    // No unstable options at the moment.
];

pub const fn rustc_version_str() -> &'static str {
    env!("RUSTC_VERSION_STR")
}

pub const VERSION_STR: &str = concat!(env!("CARGO_PKG_VERSION"), " (rustc ", env!("RUSTC_VERSION_STR"), ")");

pub const STYLES: clap::builder::styling::Styles = {
    use clap::builder::styling::*;
    Styles::styled()
        .header(Style::new().fg_color(Some(Color::Ansi(AnsiColor::BrightGreen))).bold())
        .usage(Style::new().fg_color(Some(Color::Ansi(AnsiColor::BrightGreen))).bold())
        .literal(Style::new().fg_color(Some(Color::Ansi(AnsiColor::BrightBlue))).bold())
        .placeholder(Style::new().fg_color(Some(Color::Ansi(AnsiColor::BrightBlue))))
};

pub fn command(name: &'static str) -> clap::Command {
    let cmd = clap::Command::new(name)
        .disable_help_flag(true)
        .disable_version_flag(true)
        .next_help_heading("Options")
        // Information
        // FIXME: Regression; the `help` subcommand can no longer be customized,
        //        so the about text does not match that of the help flags.
        .arg(clap::arg!(-h --help "Print help information; this message or the help of the given subcommand.").action(clap::ArgAction::Help).global(true))
        .arg(clap::arg!(-V --version "Print version information.").action(clap::ArgAction::Version).global(true))
        .next_help_heading("Display Options")
        .arg(clap::arg!(--timings "Print timing information."))
        .arg(clap::arg!(-v --verbose "Print more verbose information during execution.").action(clap::ArgAction::Count).default_value("0"));

    cmd
}
