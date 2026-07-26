//! The intermediate representation itself. Node coverage is deliberately closed over the
//! constructs actually surveyed in `packages/jade-js/primordials/*.ts` (see the "IR node set"
//! section of the primordial-IR plan) rather than modeling general TypeScript — anything
//! outside this set is a hard `lower.rs` rejection, never silently dropped or guessed at.

#[derive(Debug, Clone)]
pub struct Module {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone)]
pub enum Item {
    /// A type-only import, tracked for signature purposes and erased from both emission
    /// targets (e.g. `import type { Tenant, TenantGenerator } from "../tenants/types.ts"`).
    TypeImport { source: String, names: Vec<String> },
    /// A value import needed at runtime by both emission targets (rare in the primordials —
    /// only `promise.ts` imports runtime helpers from `async-host.ts`).
    ValueImport { source: String, names: Vec<String> },
    /// An `interface` declaration (a plain type-alias is erased entirely instead — see
    /// `lower.rs`). Round-trips through the IR for both backends: TS emission reconstructs
    /// ordinary interface syntax from the field list (not the original source text verbatim,
    /// but semantically identical for every shape observed in the surveyed source), and it
    /// drives the generated Rust struct's field layout.
    StructDef(StructDef),
    /// The repeated `const cache = new WeakMap<Tenant, X>()` per-tenant-cache idiom, recognized
    /// specially rather than as a generic `WeakMap` intrinsic — see the plan's "Rust has no
    /// `WeakMap`, but doesn't need one" note. `value_ty` is the cached record's struct name.
    PerTenantCache { name: String, value_ty: String },
    /// Any other module-level `const`/`let` whose initializer isn't a `PerTenantCache` (e.g.
    /// `typed-arrays.ts`'s `codecs` table).
    ModuleConst { name: String, mutable: bool, init: Expr },
    FnDecl(FnDecl),
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<(String, TypeRef)>,
}

#[derive(Debug, Clone)]
pub enum TypeRef {
    /// A named type as written in the source (`Function`, `object`, `Tenant`, ...). Emission
    /// backends map recognized names to their own equivalents (`Function`/`object` -> the
    /// tenant's `Value` type in Rust) and pass everything else through verbatim in TS.
    Named(String),
    /// A named type with type arguments (`TenantGenerator<X>`, `Record<K, V>`, ...).
    Generic { name: String, args: Vec<TypeRef> },
    /// `T | null` / `T | undefined`, as written.
    Optional(Box<TypeRef>),
    /// `T[]`.
    Array(Box<TypeRef>),
}

#[derive(Debug, Clone)]
pub struct FnDecl {
    pub name: Option<String>,
    pub is_generator: bool,
    pub params: Vec<Param>,
    pub body: Block,
    /// Names captured from an enclosing scope. Empty for a top-level declaration; computed
    /// during lowering for nested function expressions/closures, since Rust closures need an
    /// explicit capture list.
    pub captures: Vec<String>,
    /// The declared return type, as written (e.g. `TenantGenerator<ObjectPrimordial>`). Absent
    /// for closures/arrows, which are never independently type-annotated in the source.
    /// `emit_rust` unwraps a `TenantGenerator<X>` return type to `Result<rust_type(X),
    /// TenantError>` (see `emit_rust.rs`'s type-mapping table); `emit_ts` re-emits it verbatim.
    pub return_type: Option<TypeRef>,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub pattern: Pattern,
    /// Whether the parameter had a `= default` initializer (`args[1] ?? 0`-style defaulting is
    /// expressed at call sites in the surveyed source, not as parameter defaults, so this is
    /// currently always `None`; kept for completeness).
    pub default: Option<Expr>,
    /// The declared parameter type, as written (e.g. `Tenant`, `object`, `PropertyKey`).
    pub ty: Option<TypeRef>,
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Ident(String),
    /// Shallow object destructuring only (`const { ObjectPrototype } = ...`) — the only shape
    /// observed in the source. Nested/array destructuring is a hard lowering rejection.
    ObjectShallow(Vec<ShallowBinding>),
}

#[derive(Debug, Clone)]
pub struct ShallowBinding {
    pub key: String,
    pub binding: String,
}

#[derive(Debug, Clone, Default)]
pub struct Block(pub Vec<Stmt>);

#[derive(Debug, Clone)]
pub enum Stmt {
    Let {
        pattern: Pattern,
        init: Option<Expr>,
    },
    Expr(Expr),
    Return(Option<Expr>),
    If {
        cond: Expr,
        then_branch: Block,
        else_branch: Option<Block>,
    },
    ForOf {
        binding: Pattern,
        iter: Expr,
        body: Block,
    },
    /// The single observed indexed loop shape: `for (let i = 0; i < N; i++)`.
    ForCounting {
        binding: String,
        start: Expr,
        bound: Expr,
        body: Block,
    },
    TryCatch {
        try_block: Block,
        catch_param: Option<String>,
        catch_block: Block,
    },
    Throw(Expr),
    /// `continue;` — always unlabeled in the surveyed source (`lock`'s `if (!descriptor)
    /// continue;`). `emit_rust.rs` also recognizes the specific `if (!IDENT) continue;` shape as
    /// an Option-narrowing guard (`let Some(IDENT) = IDENT else { continue; };`) rather than a
    /// literal `if`; this variant is what makes that pattern-match possible.
    Continue,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Ident(String),
    Lit(Lit),
    TemplateLiteral(Vec<TemplatePart>),
    Array(Vec<ArrayElement>),
    Object(Vec<ObjectProp>),
    Member {
        obj: Box<Expr>,
        prop: MemberProp,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<CallArg>,
    },
    New {
        callee: Box<Expr>,
        args: Vec<CallArg>,
    },
    /// A nested function/arrow expression. Its `FnDecl::captures` records what it closes over.
    Closure(Box<FnDecl>),
    /// `yield tenant.yieldTenant(EXPR)` — the tenant-operation composition idiom, recognized as
    /// a first-class node distinct from a generic `yield` so each backend can lower it
    /// differently (see the plan's "Key simplification" note): TS emission reproduces the real
    /// generator/yield form verbatim; Rust emission strips it to a direct fallible call.
    TenantYield(Box<Expr>),
    Bin {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Un {
        op: UnOp,
        arg: Box<Expr>,
    },
    Cond {
        test: Box<Expr>,
        cons: Box<Expr>,
        alt: Box<Expr>,
    },
    Assign {
        target: Box<Expr>,
        value: Box<Expr>,
    },
    Spread(Box<Expr>),
    Paren(Box<Expr>),
    /// A recognized call against the host-intrinsic mapping table (see `intrinsics.rs`), e.g.
    /// `Number.isInteger(x)`, `Math.min(a, b)`, a `DataView` codec method, or the fixed
    /// array-index regex test. Anything calling a global *not* in that table is a hard
    /// lowering rejection, never represented as a generic/best-effort intrinsic node.
    HostIntrinsic {
        name: &'static str,
        args: Vec<Expr>,
    },
    /// `this`, only ever seen as an ordinary `thisArg`/`_this`-style parameter binding in the
    /// source, never a real method-style `this` — kept as its own node so `lower.rs` can assert
    /// that invariant rather than assuming it.
    ThisArg,
    /// The comma operator, `(e1, e2, ..., en)`: evaluate each for effect, in order, and produce
    /// the last one's value. Rust has no comma operator; `emit_rust` lowers this to a block
    /// expression (`{ e1; e2; ...; en }`), which has exactly the same "evaluate in order, value
    /// is the last one" semantics.
    Sequence(Vec<Expr>),
    /// `EXPR as TARGET`. Unlike `TsConstAssertion` (pure erasure, stripped during lowering),
    /// `as` casts recur at exactly the sites where a raw `T::Value` needs a real, non-erasable
    /// conversion on the Rust side (`x as PropertyKey` -> `Tenant::to_property_key`; `x as object
    /// | null` -> `Tenant::nullable`) — see `emit_rust.rs`'s `Expr::Cast` handling. `emit_ts.rs`
    /// re-emits the cast verbatim; dropping it there would under-type the expression at its use
    /// site and could turn a currently-type-checking call into a real `tsc` error.
    Cast { expr: Box<Expr>, target: TypeRef },
}

#[derive(Debug, Clone)]
pub enum MemberProp {
    Ident(String),
    Computed(Box<Expr>),
}

#[derive(Debug, Clone)]
pub enum CallArg {
    Normal(Expr),
    Spread(Expr),
}

#[derive(Debug, Clone)]
pub enum ArrayElement {
    Normal(Expr),
    Spread(Expr),
}

#[derive(Debug, Clone)]
pub enum ObjectProp {
    KeyValue { key: PropKey, value: Expr },
    Method { key: PropKey, func: FnDecl },
    Spread(Expr),
}

#[derive(Debug, Clone)]
pub enum PropKey {
    Ident(String),
    Computed(Box<Expr>),
}

#[derive(Debug, Clone)]
pub enum Lit {
    Str(String),
    Num(f64),
    Bool(bool),
    Null,
    Undefined,
}

#[derive(Debug, Clone)]
pub enum TemplatePart {
    Str(String),
    Expr(Expr),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Mod,
    Eq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Nullish,
    /// `key in descriptor`-style presence check. The only occurrences in the surveyed source
    /// test one of the six known `TenantPropertyDescriptor` field names against a descriptor
    /// value — Rust emission lowers this to `DynFields::has_field`, not a generic dynamic `in`.
    In,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Not,
    Neg,
    TypeOf,
}
