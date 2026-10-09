#![feature(associated_type_defaults)]
#![feature(decl_macro)]
#![feature(f16)]
#![feature(iter_collect_into)]
#![feature(iter_intersperse)]
#![feature(iterator_try_collect)]
#![feature(never_type)]

#![feature(rustc_private)]
extern crate itertools;
extern crate rustc_abi;
extern crate rustc_apfloat;
extern crate rustc_ast;
extern crate rustc_ast_pretty;
extern crate rustc_data_structures;
extern crate rustc_expand;
extern crate rustc_hir;
extern crate rustc_hir_analysis;
extern crate rustc_infer;
extern crate rustc_metadata;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;
extern crate rustc_trait_selection;

pub mod ast;
pub mod ast_lowering;
pub mod hir;
pub mod hygiene;
pub mod res;
pub mod ty;

pub trait Descr {
    fn descr(&self) -> &'static str;
}
