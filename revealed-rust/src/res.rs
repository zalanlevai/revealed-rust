use std::cell::OnceCell;
use std::collections::hash_map;
use std::collections::vec_deque::VecDeque;
use std::num::NonZeroUsize;

use rustc_data_structures::fx::{FxHashMap, FxHashSet};
use rustc_data_structures::smallvec::{SmallVec, smallvec};
use rustc_data_structures::thin_vec::ThinVec;
use rustc_middle::span_bug;
use rustc_middle::metadata::{ModChild, Reexport};
use rustc_middle::ty::TyCtxt;
use rustc_session::config::ExternLocation;
use rustc_span::{ExpnKind, DUMMY_SP, Ident, Span, Symbol, sym, kw};
use rustc_span::hygiene::AstPass;

use crate::ast;
use crate::hir::{self, CRATE_DEF_ID, CRATE_MOD_ID, LOCAL_CRATE, DefKind, Res};
use crate::ty::{self, Ty};

pub struct CrateResolutions<'tcx> {
    tcx: TyCtxt<'tcx>,
    extern_crate_name_to_cnum: FxHashMap<Symbol, Option<hir::CrateNum>>,
    cnum_to_extern_crate_name: FxHashMap<hir::CrateNum, Symbol>,
}

impl<'tcx> CrateResolutions<'tcx> {
    pub(crate) fn empty(tcx: TyCtxt<'tcx>) -> Self {
        Self {
            tcx,
            extern_crate_name_to_cnum: Default::default(),
            cnum_to_extern_crate_name: Default::default(),
        }
    }

    pub fn from_post_analysis_tcx(tcx: TyCtxt<'tcx>) -> Self {
        let crate_sources = tcx.crates(()).iter()
            .filter_map(|&cnum| {
                let crate_source = tcx.used_crate_source(cnum);
                let mut extern_crate_source_paths = crate_source.paths().cloned().peekable();
                extern_crate_source_paths.peek()?;
                Some((cnum, extern_crate_source_paths.collect::<SmallVec<[_; 3]>>()))
            })
            .collect::<FxHashMap<_, _>>();

        let mut extern_crate_name_to_cnum = tcx.sess.opts.externs.iter()
            .filter(|(_, entry)| entry.add_prelude)
            .map(|(name, entry)| {
                let renamed_cnum = match &entry.location {
                    // --extern name
                    ExternLocation::FoundInLibrarySearchDirectories => None,

                    // --extern name=file.rlib
                    ExternLocation::ExactPaths(possible_paths) => 'arm: {
                        // HACK: Find crate with sources matching this --extern flag.
                        let Some((&cnum, _)) = crate_sources.iter().find(|(_, source_paths)| {
                            source_paths.iter().any(|source_path| possible_paths.iter().any(|possible_path| {
                                possible_path.canonicalized() == source_path
                            }))
                        }) else {
                            // NOTE: We can only fetch the crate sources for actually used crates,
                            //       so we can safely discard unused crates without used sources.
                            // TODO: It might be better to remove such crates entirely from the
                            //       apparent extern prelude, as that is what rustc seems to do.
                            break 'arm None;
                        };
                        Some(cnum)
                    }
                };

                (Symbol::intern(name), renamed_cnum)
            })
            .collect::<FxHashMap<_, _>>();
        if !hir::find_attr!(tcx, crate, NoCore) {
            extern_crate_name_to_cnum.insert(sym::core, None);
            if !hir::find_attr!(tcx, crate, NoStd) {
                extern_crate_name_to_cnum.insert(sym::std, None);
            }
        }

        let cnum_to_extern_crate_name = extern_crate_name_to_cnum.iter()
            .filter_map(|(crate_name, cnum)| cnum.map(|cnum| (cnum, *crate_name)))
            .collect::<FxHashMap<_, _>>();

        Self {
            tcx,
            extern_crate_name_to_cnum,
            cnum_to_extern_crate_name,
        }
    }

    pub fn crate_by_visible_name(&self, symbol: Symbol) -> Option<hir::CrateNum> {
        self.extern_crate_name_to_cnum.get(&symbol).copied().flatten().or_else(|| {
            self.tcx.crates(()).into_iter().find(|&&cnum| self.tcx.crate_name(cnum) == symbol).copied()
        })
    }

    pub fn visible_crate_name(&self, cnum: hir::CrateNum) -> Symbol {
        self.cnum_to_extern_crate_name.get(&cnum).copied().unwrap_or_else(|| self.tcx.crate_name(cnum))
    }

    pub fn is_in_extern_prelude(&self, symbol: Symbol) -> bool {
        self.extern_crate_name_to_cnum.contains_key(&symbol)
    }
}

pub fn module_children<'tcx>(tcx: TyCtxt<'tcx>, mod_def_id: hir::DefId) -> &'tcx [ModChild] {
    match mod_def_id.as_local() {
        Some(mod_local_def_id) => tcx.module_children_local(mod_local_def_id),
        None => tcx.module_children(mod_def_id),
    }
}

pub fn lookup_mod_child<'tcx>(tcx: TyCtxt<'tcx>, mod_def_id: hir::DefId, res: hir::Res<!>, name: Symbol) -> Option<&'tcx ModChild> {
    module_children(tcx, mod_def_id).into_iter()
        .find(|mod_child| mod_child.res == res && mod_child.ident.name == name)
}

#[derive(Clone, Debug)]
pub struct ItemChild {
    pub ident: Ident,
    pub vis: ty::Visibility<hir::ModId>,
    pub res: Res,
    pub reexport: Option<Reexport>,
}

pub fn item_children<'tcx>(tcx: TyCtxt<'tcx>, def_id: hir::DefId) -> Box<dyn Iterator<Item = ItemChild> + 'tcx> {
    match tcx.def_kind(def_id) {
        DefKind::Mod | DefKind::Enum | DefKind::Trait => {
            let mod_children = match def_id.as_local() {
                Some(local_def_id) => tcx.module_children_local(local_def_id),
                None => tcx.module_children(def_id),
            };

            let iter = mod_children.iter()
                .map(|child| {
                    let res = child.res.expect_non_local();
                    let reexport = {
                        // Find first "opaque" re-export in the chain, which may alter the visible name of the item.
                        let top_level_opaque_reexport = child.reexport_chain.iter()
                            .filter(|reexport| {
                                match reexport {
                                    Reexport::Single(_) => true,
                                    Reexport::Glob(_) => false,
                                    Reexport::ExternCrate(_) => true,
                                    Reexport::MacroUse | Reexport::MacroExport => true,
                                }
                            })
                            .next();

                        top_level_opaque_reexport.copied()
                    };
                    ItemChild { ident: child.ident, vis: child.vis, res, reexport }
                });
            Box::new(iter)
        }
        DefKind::Impl { of_trait: _ } => {
            let iter = tcx.associated_item_def_ids(def_id).iter().copied()
                .map(move |assoc_def_id| {
                    let ident = tcx.opt_item_ident(assoc_def_id).unwrap();
                    let vis = tcx.visibility(assoc_def_id);
                    let res = Res::Def(tcx.def_kind(assoc_def_id), assoc_def_id);
                    ItemChild { ident, vis, res, reexport: None }
                });
            Box::new(iter)
        }
        _ => Box::new(std::iter::empty()),
    }
}

pub fn parent_iter<'tcx>(tcx: TyCtxt<'tcx>, def_id: hir::DefId) -> DefIdParentIter<'tcx> {
    DefIdParentIter { tcx, def_id }
}

pub struct DefIdParentIter<'tcx> {
    tcx: TyCtxt<'tcx>,
    def_id: hir::DefId,
}

impl<'tcx> std::iter::Iterator for DefIdParentIter<'tcx> {
    type Item = hir::DefId;

    fn next(&mut self) -> Option<Self::Item> {
        let parent_def_id = self.tcx.opt_parent(self.def_id)?;
        self.def_id = parent_def_id;
        Some(parent_def_id)
    }
}

pub fn def_id_path<'tcx>(tcx: TyCtxt<'tcx>, mut def_id: hir::DefId) -> Vec<hir::DefId> {
    let mut path = vec![def_id];
    while let Some(parent) = tcx.opt_parent(def_id) {
        path.push(parent);
        def_id = parent;
    }
    path.reverse();

    path
}

pub fn def_hir_path<'tcx>(tcx: TyCtxt<'tcx>, def_id: hir::LocalDefId) -> Vec<(hir::HirId, hir::Node<'tcx>)> {
    let def_hir_id = tcx.local_def_id_to_hir_id(def_id);

    let mut path = tcx.hir_parent_iter(def_hir_id).collect::<Vec<_>>();
    path.reverse();

    let def_node = tcx.hir_node(def_hir_id);
    path.push((def_hir_id, def_node));

    path
}

#[derive(Clone, Debug)]
pub enum DefPathRootKind<'tcx> {
    Global(hir::CrateNum),
    Ty(Ty<'tcx>),
    Local,
    Parent { supers: usize },
}

#[derive(Clone, Debug)]
pub struct DefPathSegment {
    pub def_id: hir::DefId,
    pub ident: Ident,
    pub reexport: Option<Reexport>,
}

#[derive(Clone, Debug)]
pub struct DefPath<'tcx> {
    pub root: DefPathRootKind<'tcx>,
    pub segments: Vec<DefPathSegment>,
}

impl<'tcx> DefPath<'tcx> {
    pub fn new(root: DefPathRootKind<'tcx>, segments: Vec<DefPathSegment>) -> Self {
        Self { root, segments }
    }

    /// Build `DefPath` representing the definition's canonical path, without visibility checks or hygienic renaming.
    fn canonical(tcx: TyCtxt<'tcx>, def_id: hir::DefId) -> Option<Self> {
        let [crate_def_id, def_ids @ ..] = &def_id_path(tcx, def_id)[..] else { unreachable!("empty def id path") };

        let segments = def_ids.into_iter()
            .map(|&def_id| {
                let span = tcx.def_ident_span(def_id).unwrap_or(DUMMY_SP);
                let name = tcx.opt_item_name(def_id)?;
                Some(DefPathSegment { def_id, ident: Ident::new(name, span), reexport: None })
            })
            .try_collect::<Vec<_>>()?;

        let Some(cnum) = crate_def_id.as_crate_root() else { unreachable!("def id path does not have crate root"); };

        Some(Self::new(DefPathRootKind::Global(cnum), segments))
    }

    pub fn def_id_path(&self) -> impl Iterator<Item = hir::DefId> + '_ {
        self.segments.iter().map(|segment| segment.def_id)
    }

    pub fn unhygienic_ast_path(&self, crate_res: &CrateResolutions<'tcx>, ast_ty_printer: &mut ty::print::AstTyPrinter<'tcx, '_>) -> (Option<Box<ast::QSelf>>, ast::Path) {
        let mut segments = self.segments.iter().map(|segment| {
            let ident = segment.ident;
            ast::PathSegment { id: ast::DUMMY_NODE_ID, ident, args: None }
        }).collect::<ThinVec<_>>();

        let mut qself = None;
        match &self.root {
            // `crate::..` paths.
            &DefPathRootKind::Global(cnum) if cnum == LOCAL_CRATE => {
                segments.insert(0, ast::PathSegment { id: ast::DUMMY_NODE_ID, ident: Ident::new(kw::Crate, DUMMY_SP), args: None });
            }
            // `::<crate>::..` paths.
            &DefPathRootKind::Global(cnum) => {
                let crate_name = crate_res.visible_crate_name(cnum);
                segments.splice(0..0, [
                    ast::PathSegment::path_root(DUMMY_SP),
                    ast::PathSegment { id: ast::DUMMY_NODE_ID, ident: Ident::new(crate_name, DUMMY_SP), args: None },
                ]);
            }

            // `<ty>::..` paths to inherent impl assoc items.
            &DefPathRootKind::Ty(ty) => {
                let Ok(ty_ast) = ast_ty_printer.print_ty(ty) else {
                    // FIXME: Give proper diagnostic span for errors.
                    span_bug!(DUMMY_SP, "cannot construct AST representation of type `{ty:?}`");
                };
                qself = Some(Box::new(ast::QSelf { ty: ty_ast, position: 0, path_span: DUMMY_SP }));
            }

            // Local path, no special prefix.
            DefPathRootKind::Local => {}

            // `super::..` paths.
            DefPathRootKind::Parent { supers } => {
                segments.splice(0..0, (0..*supers).map(|_| ast::PathSegment { id: ast::DUMMY_NODE_ID, ident: Ident::new(kw::Super, DUMMY_SP), args: None }));
            }
        }

        (qself, ast::Path { span: DUMMY_SP, segments })
    }
}

/// Build a relative `DefPath` from `ancestor` to `def_id`, which must be a descendant definition
/// contained directly or indirectly within `ancestor`.
///
/// If `enforce_vis` is set, ensures that each descendant path component is visible to `ancestor`.
fn descendant_def_path<'tcx>(tcx: TyCtxt<'tcx>, def_id: hir::DefId, ancestor: hir::DefId, enforce_vis: bool) -> Option<DefPath<'tcx>> {
    if !tcx.is_descendant_of(def_id, ancestor) { return None; }

    if def_id == ancestor {
        if let Some(cnum) = def_id.as_crate_root() {
            return Some(DefPath::new(DefPathRootKind::Global(cnum), vec![]));
        }
        let span = tcx.def_ident_span(def_id).unwrap_or(DUMMY_SP);
        let name = tcx.opt_item_name(def_id)?;
        return Some(DefPath::new(DefPathRootKind::Local, vec![DefPathSegment { def_id, ident: Ident::new(name, span), reexport: None }]));
    }

    let full_def_id_path = def_id_path(tcx, def_id);
    let ancestor_def_id_path = def_id_path(tcx, ancestor);
    let mut relative_def_id_path = &full_def_id_path[ancestor_def_id_path.len()..];

    // NOTE: If we find an inherent impl parent in the relative path,
    //       we modify the path to be type-relative to the type the inherent impl is for.
    //       This technically makes it a "global" (i.e. non-relative) path.
    let mut root = match relative_def_id_path {
        [crate_def_id, ..] if let Some(cnum) = crate_def_id.as_crate_root() => DefPathRootKind::Global(cnum),
        _ => DefPathRootKind::Local,
    };
    for (i, &def_id) in relative_def_id_path.iter().enumerate().rev() {
        let hir::DefKind::Impl { of_trait: false } = tcx.def_kind(def_id) else { continue; };
        let implementer_ty = tcx.type_of(def_id).instantiate_identity().skip_normalization();

        relative_def_id_path = &relative_def_id_path[(i + 1)..];
        root = DefPathRootKind::Ty(implementer_ty);
        break;
    }

    let mut def_path = DefPath::new(root, Vec::with_capacity(relative_def_id_path.len()));
    for &def_id in relative_def_id_path {
        if enforce_vis {
            if !tcx.visibility(def_id).is_accessible_from(ancestor, tcx) { return None; }
        }

        let span = tcx.def_ident_span(def_id).unwrap_or(DUMMY_SP);
        let name = tcx.opt_item_name(def_id).unwrap_or(sym::empty);
        def_path.segments.push(DefPathSegment { def_id, ident: Ident::new(name, span), reexport: None });
    }

    Some(def_path)
}

/// Build a relative `DefPath` to `def_id`, with each path component visible to `scope`,
/// considering the entire lexical scope at `scope`, including enclosing "transparent" scopes.
fn lexical_def_path<'tcx>(tcx: TyCtxt<'tcx>, def_id: hir::DefId, mut scope: hir::DefId) -> Result<DefPath<'tcx>, hir::DefId> {
    // HACK: The built-in test harness expansion generates a `#[rustc_main]` function
    //       that generates paths to (and through) private items,
    //       which are only valid because of hygiene and cannot be replicated in user-written Rust code.
    //       We leave these as-is, and do not force visibility constraints on them.
    let is_in_generated_test_main = hir::find_attr!(tcx, scope, RustcMain)
        && matches!(tcx.def_span(scope).ctxt().outer_expn_data().kind, ExpnKind::AstPass(AstPass::TestHarness));

    if !tcx.is_descendant_of(def_id, scope) {
        'fail: {
            // For some scopes, we can make an adjustment and try to find a relative path from the parent scope.
            let is_transparent = |def_kind: hir::DefKind| matches!(def_kind,
                | hir::DefKind::Struct | hir::DefKind::Enum | hir::DefKind::Union | hir::DefKind::Variant | hir::DefKind::TyAlias
                | hir::DefKind::ForeignMod | hir::DefKind::ForeignTy
                | hir::DefKind::Trait | hir::DefKind::Impl { .. } | hir::DefKind::TraitAlias
                | hir::DefKind::Fn | hir::DefKind::Const { .. } | hir::DefKind::Static { .. } | hir::DefKind::Ctor(..)
                | hir::DefKind::AssocTy | hir::DefKind::AssocFn | hir::DefKind::AssocConst { .. }
                | hir::DefKind::AnonConst
                | hir::DefKind::Closure
            );
            while is_transparent(tcx.def_kind(scope)) && let Some(parent_scope) = tcx.opt_parent(scope) {
                scope = parent_scope;
                // Adjustment succeeded, escape failing case.
                if tcx.is_descendant_of(def_id, parent_scope) { break 'fail; }
            }

            return Err(scope);
        }
    }

    // Ensure that traits are still named when referring to assoc items.
    if let hir::DefKind::Trait | hir::DefKind::TraitAlias | hir::DefKind::Impl { .. } = tcx.def_kind(scope) {
        scope = tcx.parent(scope);
    }

    let enforce_vis = !is_in_generated_test_main;
    let Some(mut def_path) = descendant_def_path(tcx, def_id, scope, enforce_vis) else { return Err(scope); };

    if let hir::DefKind::Impl { of_trait: _ } = tcx.def_kind(scope) {
        let ident = Ident::new(kw::SelfUpper, DUMMY_SP);
        def_path.segments.insert(0, DefPathSegment { def_id: scope, ident, reexport: None });
    }

    Ok(def_path)
}

/// Query override for `visible_parent_map` with our fix applied from
/// https://github.com/rust-lang/rust/pull/159881.
/// See `tests/ui/hygiene/paths/doc_hidden_reexport_of_transitive_dep_item`.
///
/// Do not use directly, instead call `TyCtxt::visible_parent_map` as normal.
/// A query override is applied for any "active" invocations of mutest-driver.
// TODO: Remove once our fix is accepted into upstream and
//       we upgrade to a version of the toolchain with the fix applied.
pub fn visible_parent_map<'tcx>(tcx: TyCtxt<'tcx>) -> hir::DefIdMap<hir::DefId> {
    let mut visible_parent_map: hir::DefIdMap<hir::DefId> = Default::default();
    let mut fallback_map: Vec<(hir::DefId, hir::DefId)> = Default::default();

    let bfs_queue = &mut VecDeque::new();

    for &cnum in tcx.crates(()) {
        if tcx.missing_extern_crate_item(cnum) { continue; }
        bfs_queue.push_back(cnum.as_def_id());
    }

    let mut add_child = |bfs_queue: &mut VecDeque<_>, child: &ModChild, parent: hir::DefId| {
        if !child.vis.is_public() { return; }

        if let Some(def_id) = child.res.opt_def_id() {
            let mut fallback = false;
            if child.ident.name == kw::Underscore {
                fallback = true;
            }
            if tcx.is_doc_hidden(parent) {
                fallback = true;
            }
            if child.reexport_chain.first().and_then(|r| r.id()).is_some_and(|id| tcx.is_doc_hidden(id)) {
                fallback = true;
            }

            match visible_parent_map.entry(def_id) {
                hash_map::Entry::Occupied(mut entry) => {
                    if !fallback {
                        if def_id.is_local() && entry.get().is_local() {
                            entry.insert(parent);
                        }
                    }
                }
                hash_map::Entry::Vacant(entry) => {
                    if fallback {
                        fallback_map.push((def_id, parent));
                    } else {
                        entry.insert(parent);
                    }

                    if child.res.module_like_def_id().is_some() {
                        bfs_queue.push_back(def_id);
                    }
                }
            }
        }
    };

    while let Some(def) = bfs_queue.pop_front() {
        for child in tcx.module_children(def).iter() {
            add_child(bfs_queue, child, def);
        }
    }

    for (child, parent) in fallback_map {
        visible_parent_map.entry(child).or_insert(parent);
    }

    visible_parent_map
}

/// Build up to `limit` absolute `DefPath` paths to `def_id`, with each path's every path component visible to `scope`.
///
/// Searches the module hierarchy of module children and re-exports
/// starting from the definition's crate root if either
/// the definition is in the local crate,
/// the definition is in a crate in the extern prelude,
/// there is an accessible path to an `extern crate` item of the definition's crate, or
/// the definition's crate is accessible through another crate.
fn absolute_def_paths<'tcx>(
    tcx: TyCtxt<'tcx>,
    crate_res: &CrateResolutions<'tcx>,
    def_id: hir::DefId,
    scope: Option<hir::DefId>,
    ignore_reexport: Option<hir::DefId>,
    span: Span,
    limit: Option<NonZeroUsize>,
) -> SmallVec<[DefPath<'tcx>; 1]> {
    let mut impl_parents = parent_iter(tcx, def_id).enumerate().filter(|&(_, def_id)| matches!(tcx.def_kind(def_id), hir::DefKind::Impl { of_trait: _ }));
    match impl_parents.next() {
        // `..::{impl#?}::$assoc_item::..` path.
        // NOTE: Such paths will never be accessible outside of the scope of the assoc item.
        Some((1.., _)) => { return smallvec![]; }

        // `..::{impl#?}::$assoc_item` path.
        Some((0, impl_parent_def_id)) => {
            let hir::DefKind::Impl { of_trait: false } = tcx.def_kind(impl_parent_def_id) else { unreachable!("encountered trait impl in def path") };
            let implementer_ty = tcx.type_of(impl_parent_def_id).instantiate_identity().skip_normalization();

            let ident = tcx.opt_item_ident(def_id).unwrap();
            let def_path = DefPath::new(DefPathRootKind::Ty(implementer_ty), vec![DefPathSegment { def_id, ident, reexport: None }]);

            return smallvec![def_path];
        }

        None => {}
    }

    let (root_def_id, root_def_path) = match def_id.as_local() {
        Some(_) => {
            let root_def_id = CRATE_DEF_ID.to_def_id();
            let root_def_path = DefPath::new(DefPathRootKind::Global(LOCAL_CRATE), vec![]);
            (root_def_id, root_def_path)
        }
        None if let crate_name = crate_res.visible_crate_name(def_id.krate) && crate_res.is_in_extern_prelude(crate_name) => {
            let root_def_id = def_id.krate.as_def_id();
            let root_def_path = DefPath::new(DefPathRootKind::Global(def_id.krate), vec![]);
            (root_def_id, root_def_path)
        }
        None => 'root: {
            // NOTE: Lazily compute this once and only when needed upon first access.
            let visible_extern_crate_defs = OnceCell::new();
            let crate_to_def_path = |cnum| {
                // First, check if the crate is in the extern prelude through an `--extern` option.
                let crate_name = crate_res.visible_crate_name(cnum);
                if crate_res.is_in_extern_prelude(crate_name) {
                    let root_def_path = DefPath::new(DefPathRootKind::Global(cnum), vec![]);
                    return Some(Some(root_def_path));
                }

                // Otherwise, check if there is a visible `extern crate` item we can use to refer to the crate instead.
                let visible_extern_crate_defs = visible_extern_crate_defs.get_or_init(|| {
                    tcx.hir_crate_items(()).definitions()
                        .filter(|&def_id| matches!(tcx.def_kind(def_id), hir::DefKind::ExternCrate))
                        .filter_map(|def_id| Some((def_id, tcx.extern_mod_stmt_cnum(def_id)?)))
                        .filter(|&(extern_crate_def_id, _cnum)| {
                            let mod_scope = scope.map(|scope| match tcx.def_kind(scope) {
                                hir::DefKind::Mod => scope,
                                _ => tcx.parent_module_from_def_id(scope.expect_local()).to_def_id(),
                            });
                            tcx.visibility(extern_crate_def_id).is_accessible_from(mod_scope.unwrap_or(LOCAL_CRATE.as_def_id()), tcx)
                        })
                        .collect::<Vec<_>>()
                });
                if let Some(&(visible_extern_crate_def_id, _cnum)) = visible_extern_crate_defs.iter().find(|&&(_, extern_crate_def_cnum)| extern_crate_def_cnum == cnum) {
                    let Some(mut root_def_path) = DefPath::canonical(tcx, visible_extern_crate_def_id.to_def_id()) else { return Some(None) };

                    if let Some(scope) = scope && tcx.is_descendant_of(visible_extern_crate_def_id.to_def_id(), scope) {
                        let scope_def_id_path = def_id_path(tcx, scope);
                        root_def_path.segments.splice(0..(scope_def_id_path.len() - 1), []);
                        root_def_path.root = DefPathRootKind::Local;
                    }

                    return Some(Some(root_def_path));
                }

                None
            };

            // First, check if this path points to a direct dependency extern crate.
            // NOTE: The visible_parent_map does not have entries for direct dependency crates.
            if let Some(cnum) = def_id.as_crate_root() {
                match crate_to_def_path(cnum) {
                    Some(Some(root_def_path)) => { break 'root (def_id, root_def_path); }
                    Some(None) => { return smallvec![]; }
                    None => {}
                }
            }

            // Otherwise, check the `visible_parent_map` to
            // find a visible parent to the transitive dependency through direct dependency crates.
            let visible_parent_map = tcx.visible_parent_map(());
            let mut visible_def_id = def_id;
            while let Some(&visible_parent) = visible_parent_map.get(&visible_def_id) {
                visible_def_id = visible_parent;
                if let Some(cnum) = visible_def_id.as_crate_root() {
                    match crate_to_def_path(cnum) {
                        Some(Some(root_def_path)) => { break 'root (visible_def_id, root_def_path); }
                        Some(None) => { return smallvec![]; }
                        None => {}
                    }
                }
            }

            span_bug!(span, "expected `{}` to be reached through another crate either through the extern prelude or an `extern crate` item", tcx.def_path_str(def_id));
        }
    };

    if root_def_id == def_id {
        return smallvec![root_def_path];
    }

    let mut paths = smallvec![];

    let mut seen_containers: FxHashSet<hir::DefId> = Default::default();
    let mut worklist = vec![(root_def_id, root_def_path)];
    while !worklist.is_empty() {
        let mut new_worklist: Vec<(hir::DefId, DefPath)> = vec![];

        for (container_def_id, container_def_path) in worklist.drain(..) {
            let children = item_children(tcx, container_def_id);

            for child in children {
                let visible = false
                    || child.vis == ty::Visibility::Public
                    || child.vis == ty::Visibility::Restricted(CRATE_MOD_ID.to_mod_id())
                    || scope.is_some_and(|scope| child.vis.is_accessible_from(scope, tcx));

                if !visible { continue; }
                if child.ident.name == kw::Underscore { continue; }
                if let Some(reexport) = child.reexport && reexport.id() == ignore_reexport { continue; }

                if child.res.opt_def_id() == Some(def_id) {
                    let mut path = container_def_path.clone();
                    path.segments.push(DefPathSegment { def_id, ident: child.ident, reexport: child.reexport });
                    paths.push(path);

                    if let Some(limit) = limit && paths.len() >= limit.get() {
                        return paths;
                    }
                }

                match child.res {
                    Res::Def(DefKind::Mod | DefKind::Enum | DefKind::Trait | DefKind::Impl { .. }, child_def_id) => {
                        if seen_containers.contains(&child_def_id) { continue; }
                        seen_containers.insert(child_def_id);

                        let mut path = container_def_path.clone();
                        path.segments.push(DefPathSegment { def_id: child_def_id, ident: child.ident, reexport: child.reexport });
                        new_worklist.push((child_def_id, path));
                    }
                    _ => {}
                };
            }
        }

        worklist.extend(new_worklist);
    }

    paths
}

/// Build an absolute `DefPath` to `def_id`, with each path component visible to `scope`.
/// See `absolute_def_paths` for more details.
fn absolute_def_path<'tcx>(
    tcx: TyCtxt<'tcx>,
    crate_res: &CrateResolutions<'tcx>,
    def_id: hir::DefId,
    scope: Option<hir::DefId>,
    ignore_reexport: Option<hir::DefId>,
    span: Span,
) -> Option<DefPath<'tcx>> {
    absolute_def_paths(tcx, crate_res, def_id, scope, ignore_reexport, span, NonZeroUsize::new(1)).into_iter().next()
}

#[derive(Clone, Copy)]
pub enum DefPathRequestKind {
    Def(hir::DefId),
    ParentModPrefix(hir::ModId),
}

impl DefPathRequestKind {
    pub fn def_id(&self) -> hir::DefId {
        match self {
            DefPathRequestKind::Def(def_id) => *def_id,
            DefPathRequestKind::ParentModPrefix(mod_id) => mod_id.to_def_id(),
        }
    }
}

/// Build a `DefPath` to `def_id`, with each path component visible to `scope`.
pub fn visible_def_path<'tcx>(
    tcx: TyCtxt<'tcx>,
    crate_res: &CrateResolutions<'tcx>,
    request: DefPathRequestKind,
    scope: Option<hir::DefId>,
    ignore_reexport: Option<hir::DefId>,
    span: Span,
) -> Result<DefPath<'tcx>, Option<hir::DefId>> {
    let mut def_id = request.def_id();
    if let hir::DefKind::Ctor(..) = tcx.def_kind(def_id) {
        // Adjust target definition to the variant parent to avoid naming the unnamed constructor.
        def_id = tcx.parent(def_id);
    }

    // Prefer using a direct, local path to local items within the same module as the enclosing module (or parent modules) of the current scope.
    // NOTE: This helps avoid visibility-related resolution issues in local items, see
    //       `tests/ui/hygiene/rustc_res/private_ctor_not_available_in_same_scope_through_reexport`, and
    //       `tests/ui/hygiene/rustc_res/private_ctor_not_available_in_child_scope_through_reexport`.
    if let Some(scope) = scope && let Some(local_def_id) = def_id.as_local() && def_id != scope {
        let mod_scope = match tcx.def_kind(scope) {
            hir::DefKind::Mod => scope,
            _ => tcx.parent_module_from_def_id(scope.expect_local()).to_def_id(),
        };
        let containing_mod = match request {
            DefPathRequestKind::Def(_) => tcx.parent_module_from_def_id(local_def_id).to_def_id(),
            DefPathRequestKind::ParentModPrefix(_) => def_id,
        };

        if containing_mod == mod_scope {
            if let Ok(visible_path) = lexical_def_path(tcx, def_id, scope) {
                return Ok(visible_path);
            }
        } else if !containing_mod.is_crate_root() {
            let is_locally_accessible_through_supers = 'v: {
                let mut parent_mod_scope = mod_scope;
                let mut super_mods = vec![];

                while parent_mod_scope != containing_mod {
                    let parent_mod = tcx.parent_module_from_def_id(parent_mod_scope.expect_local()).to_def_id();
                    if parent_mod.is_crate_root() {
                        break 'v None;
                    }

                    super_mods.push(parent_mod);
                    parent_mod_scope = parent_mod;
                }

                Some(super_mods)
            };

            if let Some(super_mods) = is_locally_accessible_through_supers {
                match request {
                    DefPathRequestKind::Def(_) => {
                        if let Ok(mut visible_path) = lexical_def_path(tcx, def_id, containing_mod) {
                            // Construct path to containing parent module, which is
                            // always accessible through consecutive `super` path segments.
                            visible_path.root = DefPathRootKind::Parent { supers: super_mods.len() };
                            return Ok(visible_path);
                        }
                    }
                    DefPathRequestKind::ParentModPrefix(_) => {
                        // Construct direct path of `super` path segments, which is not valid by itself,
                        // but will be valid once the caller appends the item segment(s) to it.
                        return Ok(DefPath::new(DefPathRootKind::Parent { supers: super_mods.len() }, vec![]));
                    }
                }
            }
        }
    }

    if let Some(visible_path) = absolute_def_path(tcx, crate_res, def_id, scope, ignore_reexport, span) {
        return Ok(visible_path);
    }

    // Ensure that the def is in the current scope, otherwise it really is not visible from here.
    let Some(scope) = scope else { return Err(None); };
    match lexical_def_path(tcx, def_id, scope) {
        Ok(visible_path) => Ok(visible_path),
        Err(adjusted_scope) => Err(Some(adjusted_scope)),
    }
}
