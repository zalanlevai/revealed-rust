#![feature(rustc_private)]
extern crate rustc_ast;
extern crate rustc_ast_pretty;
extern crate rustc_driver;
extern crate rustc_interface;
extern crate rustc_session;
extern crate rustc_span;

pub mod compiler;
pub mod config;

use std::time::{Duration, Instant};

use rustc_interface::{create_and_enter_global_ctxt, passes, run_compiler};
use rustc_interface::interface::Result as CompilerResult;
use rustc_span::ErrorGuaranteed;

use crate::compiler::base_compiler_config;
use crate::config::Config;

const GENERATED_CODE_PRELUDE: &str = r#"
#![allow(unused_features)]
#![allow(unused_imports)]
"#;

pub struct RevealResult {
    pub duration: Duration,
    pub sanitize_macro_expns_duration: Duration,
    pub codegen_duration: Duration,
    pub generated_crate_code: String,
}

pub fn run(config: Config) -> CompilerResult<RevealResult> {
    let t_start = Instant::now();

    let mut compiler_config = base_compiler_config(&config);

    // NOTE: We must turn off `format_args` optimizations to be able to match up
    //       argument nodes between the AST and the HIR.
    //       See `revealed_rust::ast_lowering` for more details.
    compiler_config.opts.unstable_opts.flatten_format_args = false;

    let opts = &config.opts;

    let reveal_result = run_compiler(compiler_config, |compiler| -> CompilerResult<RevealResult> {
        let sess = &compiler.sess;

        let t_start = Instant::now();
        let mut reveal_result = RevealResult {
            duration: Duration::ZERO,
            sanitize_macro_expns_duration: Duration::ZERO,
            codegen_duration: Duration::ZERO,
            generated_crate_code: String::new(),
        };

        let crate_ast = passes::parse(sess);

        create_and_enter_global_ctxt(compiler, crate_ast, |tcx| -> Result<RevealResult, ErrorGuaranteed> {
            let (mut generated_crate_ast, def_res) = {
                let (resolver, expanded_crate_ast) = tcx.resolver_for_lowering();
                let def_res = revealed_rust::ast_lowering::DefResolutions::from_resolver(&*resolver.borrow());
                let generated_crate_ast = expanded_crate_ast.borrow().clone();
                (generated_crate_ast, def_res)
            };

            tcx.ensure_ok().analysis(());

            let crate_res = revealed_rust::res::CrateResolutions::from_post_analysis_tcx(tcx);

            let body_res = revealed_rust::ast_lowering::resolve_bodies(tcx, &def_res, &generated_crate_ast);
            if opts.unstable_flags.verify_ast_lowering {
                revealed_rust::ast_lowering::validate_body_resolutions(&body_res, &def_res, &generated_crate_ast);
            }

            let t_sanitize_macro_expns_start = Instant::now();
            revealed_rust::hygiene::sanitize_macro_expansions(tcx, &crate_res, &def_res, &body_res, &mut generated_crate_ast);
            reveal_result.sanitize_macro_expns_duration = t_sanitize_macro_expns_start.elapsed();

            let t_codegen_start = Instant::now();
            struct NoAnn;
            impl rustc_ast_pretty::pprust::state::PpAnn for NoAnn {}
            reveal_result.generated_crate_code = format!("{prelude}\n{code}",
                prelude = GENERATED_CODE_PRELUDE,
                code = rustc_ast_pretty::pprust::print_crate(
                    tcx.sess.source_map(),
                    &generated_crate_ast,
                    tcx.sess.io.input.file_name(tcx.sess),
                    "".to_owned(),
                    &NoAnn,
                    true,
                    tcx.sess.edition(),
                    &tcx.sess.psess.attr_id_generator,
                ),
            );
            reveal_result.codegen_duration = t_codegen_start.elapsed();

            reveal_result.duration = t_start.elapsed();

            Ok(reveal_result)
        })
    })?;

    println!("{}", reveal_result.generated_crate_code);

    if opts.report_timings {
        println!("\nfinished in {total:.2?} (hygiene {hygiene:.2?}; codegen {codegen:.2?})",
            total = t_start.elapsed(),
            hygiene = reveal_result.sanitize_macro_expns_duration,
            codegen = reveal_result.codegen_duration,
        );
    }

    Ok(reveal_result)
}
