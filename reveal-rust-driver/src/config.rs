use std::path::PathBuf;

use rustc_interface::Config as CompilerConfig;

#[derive(Clone, Debug, Default)]
pub struct UnstableFlags {
    pub verify_ast_lowering: bool,
}

pub struct Options {
    pub verbosity: u8,
    pub report_timings: bool,

    pub unstable_flags: UnstableFlags,
}

pub struct Config {
    pub compiler_config: CompilerConfig,
    pub invocation_fingerprint: Option<String>,
    pub reveal_rust_target_dir_root: Option<PathBuf>,
    pub opts: Options,
}

impl Config {
    pub fn target_dir_root(&self) -> PathBuf {
        self.reveal_rust_target_dir_root.clone().unwrap_or(self.compiler_config.output_dir.clone().unwrap_or_default())
    }
}
