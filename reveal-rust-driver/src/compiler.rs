use std::collections::BTreeMap;

use rustc_interface::Config as CompilerConfig;
use rustc_session::Session;
use rustc_session::config::{CrateType, ExternEntry, ExternLocation, Externs, Input};
use rustc_span::Symbol;
use rustc_span::source_map::RealFileLoader;

use crate::config::Config;

pub fn copy_compiler_settings(config: &CompilerConfig) -> CompilerConfig {
    let input = match &config.input {
        Input::File(f) => Input::File(f.clone()),
        Input::Str { name, input } => Input::Str { name: name.clone(), input: input.clone() },
    };

    CompilerConfig {
        opts: config.opts.clone(),
        crate_cfg: config.crate_cfg.clone(),
        crate_check_cfg: config.crate_check_cfg.clone(),
        input,
        output_file: config.output_file.clone(),
        output_dir: config.output_dir.clone(),
        ice_file: config.ice_file.clone(),
        file_loader: Some(Box::new(RealFileLoader)),
        lint_caps: config.lint_caps.clone(),
        psess_created: None,
        track_state: None,
        register_lints: None,
        override_queries: None,
        extra_symbols: config.extra_symbols.clone(),
        make_codegen_backend: None,
        using_internal_features: config.using_internal_features,
    }
}

struct RustcConfigCallbacks {
    config: Option<CompilerConfig>,
    crate_types: Vec<CrateType>,
}

impl rustc_driver::Callbacks for RustcConfigCallbacks {
    fn config(&mut self, config: &mut CompilerConfig) {
        self.config = Some(copy_compiler_settings(config));
    }

    fn after_crate_root_parsing(
        &mut self,
        compiler: &rustc_interface::interface::Compiler,
        krate: &mut rustc_ast::Crate,
    ) -> rustc_driver::Compilation {
        rustc_interface::create_and_enter_global_ctxt(compiler, krate.clone(), |tcx| {
            self.crate_types = tcx.crate_types().to_vec();
        });

        rustc_driver::Compilation::Stop
    }
}

pub fn parse_compiler_args(args: &[String]) -> (Option<CompilerConfig>, Vec<CrateType>) {
    let mut callbacks = RustcConfigCallbacks { config: None, crate_types: vec![] };
    rustc_driver::run_compiler(args, &mut callbacks);
    (callbacks.config, callbacks.crate_types)
}

pub fn track_invocation_fingerprint(sess: &Session, invocation_fingerprint: Option<&str>) {
    sess.env_depinfo.borrow_mut().insert((
        Symbol::intern("REVEAL_RUST_FINGERPRINT"),
        invocation_fingerprint.map(Symbol::intern),
    ));
}

pub fn base_compiler_config_from_parts(compiler_config: &CompilerConfig, invocation_fingerprint: Option<String>) -> CompilerConfig {
    let mut compiler_config = copy_compiler_settings(compiler_config);

    compiler_config.track_state = Some(Box::new(move |sess| {
        track_invocation_fingerprint(sess, invocation_fingerprint.as_deref());
    }));

    compiler_config.override_queries = Some(|_sess, providers| {
        // FIXME: Remove once https://github.com/rust-lang/rust/pull/159881 is accepted into upstream.
        providers.queries.visible_parent_map = |tcx, ()| revealed_rust::res::visible_parent_map(tcx);
    });

    // Register #[cfg(test)] as a valid cfg.
    // See the rustc change https://github.com/rust-lang/rust/pull/131729, and
    // the Cargo change https://github.com/rust-lang/cargo/pull/14963
    // for more details.
    compiler_config.crate_check_cfg.push("cfg(test)".to_owned());

    let mut externs = BTreeMap::<String, ExternEntry>::new();
    for (key, entry) in compiler_config.opts.externs.iter() {
        externs.insert(key.clone(), entry.clone());
    }
    // Externs for some std macros may have to be loaded.
    externs.insert("alloc".to_owned(), ExternEntry {
        location: ExternLocation::FoundInLibrarySearchDirectories,
        is_private_dep: false,
        add_prelude: true,
        nounused_dep: false,
        force: false,
    });
    externs.insert("std_detect".to_owned(), ExternEntry {
        location: ExternLocation::FoundInLibrarySearchDirectories,
        is_private_dep: false,
        add_prelude: true,
        nounused_dep: false,
        force: false,
    });
    compiler_config.opts.externs = Externs::new(externs);

    compiler_config
}

pub fn base_compiler_config(config: &Config) -> CompilerConfig {
    base_compiler_config_from_parts(&config.compiler_config, config.invocation_fingerprint.clone())
}
