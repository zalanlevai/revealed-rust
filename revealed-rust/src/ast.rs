pub use rustc_ast::*;
pub use rustc_ast::token::TokenKind;
pub use rustc_ast::tokenstream::*;

use rustc_span::Span;
use rustc_span::symbol::{Ident, Symbol};

use crate::analysis::Descr;

#[derive(Clone, Debug)]
pub struct FnItem<'ast> {
    pub id: ast::NodeId,
    pub span: Span,
    pub ctx: visit::FnCtxt,
    pub vis: &'ast ast::Visibility,
    pub fn_data: &'ast ast::Fn,
}

impl<'ast> FnItem<'ast> {
    pub fn from_item(item: &'ast ast::Item) -> Option<Self> {
        let &ast::Item { id, span, ref vis, ref kind, .. } = item;
        let ast::ItemKind::Fn(fn_item) = kind else { return None; };
        let ctx = visit::FnCtxt::Free;
        Some(Self { id, span, ctx, vis, fn_data: fn_item })
    }

    pub fn from_assoc_item(item: &'ast ast::AssocItem) -> Option<Self> {
        let &ast::Item { id, span, ref vis, ref kind, .. } = item;
        let ast::AssocItemKind::Fn(fn_item) = kind else { return None; };
        let ctx = visit::FnCtxt::Free; // FIXME
        Some(Self { id, span, ctx, vis, fn_data: fn_item })
    }
}

pub enum DefItemKind<'ast> {
    ExternCrate(Option<Symbol>, Ident),
    Use(&'ast ast::UseTree),
    Static(&'ast ast::StaticItem),
    Const(&'ast ast::ConstItem),
    ConstBlock(&'ast ast::ConstBlockItem),
    Fn(&'ast ast::Fn),
    Mod(ast::Safety, Ident, &'ast ast::ModKind),
    ForeignMod(&'ast ast::ForeignMod),
    GlobalAsm(&'ast ast::InlineAsm),
    TyAlias(&'ast ast::TyAlias),
    Enum(Ident, &'ast ast::Generics, &'ast ast::EnumDef),
    Struct(Ident, &'ast ast::Generics, &'ast ast::VariantData),
    Union(Ident, &'ast ast::Generics, &'ast ast::VariantData),
    Trait(&'ast ast::Trait),
    TraitAlias(&'ast ast::TraitAlias),
    Impl(&'ast ast::Impl),
    MacCall(&'ast ast::MacCall),
    MacroDef(Ident, &'ast ast::MacroDef),
    Delegation(&'ast ast::Delegation),
    DelegationMac(&'ast ast::DelegationMac),
}

impl<'ast> DefItemKind<'ast> {
    pub fn from_item_kind(item_kind: &'ast ast::ItemKind) -> Self {
        match item_kind {
            ast::ItemKind::ExternCrate(symbol, ident) => Self::ExternCrate(*symbol, *ident),
            ast::ItemKind::Use(use_tree) => Self::Use(use_tree),
            ast::ItemKind::Static(static_item) => Self::Static(static_item),
            ast::ItemKind::Const(const_item) => Self::Const(const_item),
            ast::ItemKind::ConstBlock(const_block_item) => Self::ConstBlock(const_block_item),
            ast::ItemKind::Fn(fn_item) => Self::Fn(fn_item),
            ast::ItemKind::Mod(safety, ident, mod_kind) => Self::Mod(*safety, *ident, mod_kind),
            ast::ItemKind::ForeignMod(foreign_mod) => Self::ForeignMod(foreign_mod),
            ast::ItemKind::GlobalAsm(inline_asm) => Self::GlobalAsm(inline_asm),
            ast::ItemKind::TyAlias(ty_alias) => Self::TyAlias(ty_alias),
            ast::ItemKind::Enum(ident, generics, enum_def) => Self::Enum(*ident, generics, enum_def),
            ast::ItemKind::Struct(ident, generics, variant_data) => Self::Struct(*ident, generics, variant_data),
            ast::ItemKind::Union(ident, generics, variant_data) => Self::Union(*ident, generics, variant_data),
            ast::ItemKind::Trait(trait_) => Self::Trait(trait_),
            ast::ItemKind::TraitAlias(trait_alias) => Self::TraitAlias(trait_alias),
            ast::ItemKind::Impl(impl_) => Self::Impl(impl_),
            ast::ItemKind::MacCall(mac_call) => Self::MacCall(mac_call),
            ast::ItemKind::MacroDef(ident, macro_def) => Self::MacroDef(*ident, macro_def),
            ast::ItemKind::Delegation(delegation) => Self::Delegation(delegation),
            ast::ItemKind::DelegationMac(delegation_mac) => Self::DelegationMac(delegation_mac),
        }
    }

    pub fn from_foreign_item_kind(item_kind: &'ast ast::ForeignItemKind) -> Self {
        match item_kind {
            ast::ForeignItemKind::Static(static_item) => Self::Static(static_item),
            ast::ForeignItemKind::Fn(fn_item) => Self::Fn(fn_item),
            ast::ForeignItemKind::TyAlias(ty_alias) => Self::TyAlias(ty_alias),
            ast::ForeignItemKind::MacCall(mac_call) => Self::MacCall(mac_call),
        }
    }

    pub fn from_assoc_item_kind(item_kind: &'ast ast::AssocItemKind) -> Self {
        match item_kind {
            ast::AssocItemKind::Const(const_item) => Self::Const(const_item),
            ast::AssocItemKind::Fn(fn_item) => Self::Fn(fn_item),
            ast::AssocItemKind::Type(ty_alias) => Self::TyAlias(ty_alias),
            ast::AssocItemKind::MacCall(mac_call) => Self::MacCall(mac_call),
            ast::AssocItemKind::Delegation(delegation) => Self::Delegation(delegation),
            ast::AssocItemKind::DelegationMac(delegation_mac) => Self::DelegationMac(delegation_mac),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum DefItem<'ast> {
    Item(&'ast ast::Item),
    ForeignItem(&'ast ast::ForeignItem),
    AssocItem(&'ast ast::AssocItem, visit::AssocCtxt),
}

impl<'ast> DefItem<'ast> {
    pub fn node_id(&self) -> NodeId {
        match self {
            Self::Item(item) => item.id,
            Self::ForeignItem(item) => item.id,
            Self::AssocItem(item, _) => item.id,
        }
    }

    pub fn ident(&self) -> Ident {
        match self {
            Self::Item(item) if let Some(ident) = item.kind.ident() => ident,
            Self::Item(item) => panic!("{} does not have ident", item.kind.descr()),
            Self::ForeignItem(item) if let Some(ident) = item.kind.ident() => ident,
            Self::ForeignItem(item) => panic!("{} does not have ident", item.kind.descr()),
            Self::AssocItem(item, _) if let Some(ident) = item.kind.ident() => ident,
            Self::AssocItem(item, _) => panic!("{} does not have ident", item.kind.descr()),
        }
    }

    pub fn span(&self) -> Span {
        match self {
            Self::Item(item) => item.span,
            Self::ForeignItem(item) => item.span,
            Self::AssocItem(item, _) => item.span,
        }
    }

    pub fn kind(&self) -> DefItemKind<'ast> {
        match self {
            Self::Item(item) => DefItemKind::from_item_kind(&item.kind),
            Self::ForeignItem(item) => DefItemKind::from_foreign_item_kind(&item.kind),
            Self::AssocItem(item, _) => DefItemKind::from_assoc_item_kind(&item.kind),
        }
    }

    pub fn owned_item_kind(&self) -> ast::ItemKind {
        match self {
            Self::Item(item) => item.kind.clone(),
            Self::ForeignItem(item) => item.kind.clone().into(),
            Self::AssocItem(item, _) => item.kind.clone().into(),
        }
    }
}

impl Descr for ast::ForeignItemKind {
    fn descr(&self) -> &'static str {
        match self {
            ast::ForeignItemKind::Static(..) => "static item",
            ast::ForeignItemKind::TyAlias(..) => "type alias",
            ast::ForeignItemKind::Fn(..) => "function",
            ast::ForeignItemKind::MacCall(..) => "item macro invocation",
        }
    }
}

impl Descr for ast::AssocItemKind {
    fn descr(&self) -> &'static str {
        match self {
            ast::AssocItemKind::Const(..) => "const item",
            ast::AssocItemKind::Type(..) => "type alias",
            ast::AssocItemKind::Fn(..) => "function",
            ast::AssocItemKind::MacCall(..) => "item macro invocation",
            ast::AssocItemKind::Delegation(..) => "delegation",
            ast::AssocItemKind::DelegationMac(..) => "delegation macro",
        }
    }
}

impl Descr for ast::StmtKind {
    fn descr(&self) -> &'static str {
        match self {
            ast::StmtKind::Item(..) => "item",
            ast::StmtKind::Let(..) => "let",
            ast::StmtKind::Semi(..) => "statement expression",
            ast::StmtKind::Expr(..) => "trailing expression",
            ast::StmtKind::MacCall(..) => "macro call",
            ast::StmtKind::Empty => "empty",
        }
    }
}

impl Descr for ast::ExprKind {
    fn descr(&self) -> &'static str {
        match self {
            ast::ExprKind::Array(..) => "array literal",
            ast::ExprKind::ConstBlock(..) => "const block",
            ast::ExprKind::Call(..) => "call",
            ast::ExprKind::MethodCall(..) => "method call",
            ast::ExprKind::Tup(..) => "tuple literal",
            ast::ExprKind::Binary(..) => "binary operation",
            ast::ExprKind::Unary(..) => "unary operation",
            ast::ExprKind::Move(..) => "move",
            ast::ExprKind::Lit(..) => "literal",
            ast::ExprKind::Cast(..) => "cast",
            ast::ExprKind::Type(..) => "type ascription",
            ast::ExprKind::Let(..) => "let",
            ast::ExprKind::If(..) => "if",
            ast::ExprKind::While(..) => "while",
            ast::ExprKind::ForLoop { .. } => "for loop",
            ast::ExprKind::Loop(..) => "loop",
            ast::ExprKind::Match(..) => "match",
            ast::ExprKind::Closure(..) => "closure",
            ast::ExprKind::Block(..) => "block",
            ast::ExprKind::Gen(_, _, ast::GenBlockKind::Async, _) => "async block",
            ast::ExprKind::Gen(_, _, ast::GenBlockKind::Gen, _) => "generator block",
            ast::ExprKind::Gen(_, _, ast::GenBlockKind::AsyncGen, _) => "async generator block",
            ast::ExprKind::Await(..) => "await",
            ast::ExprKind::TryBlock(..) => "try block",
            ast::ExprKind::Use(..) => "use",
            ast::ExprKind::Assign(..) => "assignment",
            ast::ExprKind::AssignOp(..) => "assignment with operator",
            ast::ExprKind::Field(..) => "field access",
            ast::ExprKind::Index(..) => "index",
            ast::ExprKind::Range(..) => "range",
            ast::ExprKind::Underscore => "_",
            ast::ExprKind::Path(..) => "path",
            ast::ExprKind::AddrOf(..) => "reference",
            ast::ExprKind::Break(..) => "break",
            ast::ExprKind::Continue(..) => "continue",
            ast::ExprKind::Ret(..) => "return",
            ast::ExprKind::InlineAsm(..) => "inline assembly",
            ast::ExprKind::OffsetOf(..) => "field offset",
            ast::ExprKind::MacCall(..) => "macro call",
            ast::ExprKind::Struct(..) => "struct literal",
            ast::ExprKind::Repeat(..) => "array from repetition",
            ast::ExprKind::Paren(..) => "parentheses",
            ast::ExprKind::Try(..) => "try",
            ast::ExprKind::Yield(..) => "yield",
            ast::ExprKind::Yeet(..) => "yeet",
            ast::ExprKind::Become(..) => "become",
            ast::ExprKind::IncludedBytes(..) => "included bytes",
            ast::ExprKind::FormatArgs(..) => "format_args",
            ast::ExprKind::UnsafeBinderCast(..) => "unsafe binder cast",
            ast::ExprKind::DirectConstArg(..) => "direct const arg",
            ast::ExprKind::Err(..) => "error",
            ast::ExprKind::Dummy => "dummy",
        }
    }
}

impl Descr for ast::PatKind {
    fn descr(&self) -> &'static str {
        match self {
            ast::PatKind::Missing => "missing",
            ast::PatKind::Wild => "_",
            ast::PatKind::Never => "!",
            ast::PatKind::Ident(..) => "ident",
            ast::PatKind::Path(..) => "path",
            ast::PatKind::Tuple(..) => "tuple",
            ast::PatKind::Struct(..) => "struct",
            ast::PatKind::TupleStruct(..) => "tuple struct",
            ast::PatKind::Rest => "..",
            ast::PatKind::Box(..) => "box",
            ast::PatKind::Ref(..) => "reference",
            ast::PatKind::Deref(..) => "deref",
            ast::PatKind::Or(..) => "or",
            ast::PatKind::Range(..) => "range",
            ast::PatKind::Slice(..) => "slice",
            ast::PatKind::Expr(..) => "expression",
            ast::PatKind::Guard(..) => "guard",
            ast::PatKind::MacCall(..) => "macro call",
            ast::PatKind::Paren(..) => "parentheses",
            ast::PatKind::Err(..) => "error",
        }
    }
}

impl Descr for ast::TyKind {
    fn descr(&self) -> &'static str {
        match self {
            ast::TyKind::Never => "!",
            ast::TyKind::Path(..) => "path",
            ast::TyKind::Ptr(..) => "raw pointer",
            ast::TyKind::Ref(..) => "reference",
            ast::TyKind::PinnedRef(..) => "pinned reference",
            ast::TyKind::Slice(..) => "slice",
            ast::TyKind::Array(..) => "array",
            ast::TyKind::Tup(..) => "tuple",
            ast::TyKind::FnPtr(..) => "fn pointer",
            ast::TyKind::TraitObject(..) => "trait object",
            ast::TyKind::ImplTrait(..) => "impl trait",
            ast::TyKind::ImplicitSelf => "self",
            ast::TyKind::Infer => "infer",
            ast::TyKind::CVarArgs => "C var args (va_list)",
            ast::TyKind::UnsafeBinder(..) => "unsafe binder",
            ast::TyKind::Pat(..) => "pattern",
            ast::TyKind::FieldOf(..) => "field of",
            ast::TyKind::View(..) => "view",
            ast::TyKind::DirectConstArg(..) => "direct const arg",
            ast::TyKind::Paren(..) => "parentheses",
            ast::TyKind::MacCall(..) => "macro call",
            ast::TyKind::Err(..) => "error",
            ast::TyKind::Dummy => "dummy",
        }
    }
}
