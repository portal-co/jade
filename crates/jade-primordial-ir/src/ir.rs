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
    PerTenantCache {
        name: String,
        value_ty: String,
        /// Whether the `WeakMap`'s own value type argument was the `{ identity: object;
        /// primordial: <value_ty> }` wrapper shape (`array-buffer.ts`/`typed-arrays.ts`) rather
        /// than a bare named type. `emit_ts.rs` needs this to re-declare the wrapper faithfully;
        /// `emit_rust.rs` ignores it (the identity check has no Rust-side meaning — see
        /// `docs/proxy-and-buffer-primordial-gap-plan.md`) but the *body*'s own matching
        /// get/set statements still need their extended shape recognized regardless.
        identity_wrapped: bool,
    },
    /// Any other module-level `const`/`let` whose initializer isn't a `PerTenantCache` (e.g.
    /// `typed-arrays.ts`'s `codecs` table).
    ModuleConst { name: String, mutable: bool, init: Expr },
    FnDecl(FnDecl),
    /// A `class ... implements X { ... }` declaration. Lowered only for the closed shape
    /// actually used (see `docs/proxy-and-buffer-primordial-gap-plan.md`'s class-lowering
    /// note): private/public fields (plain-initialized, or declared `!`/`?` with no
    /// initializer and set later), a single constructor, and methods (including generator
    /// methods) that may reference `this`/`this.#field`. No `extends`, no static members, no
    /// accessors/auto-accessors — every one of those is a hard `lower.rs` rejection.
    ///
    /// Rust emission targets a `Rc<RefCell<Inner>>`-backed wrapper type (see `emit_rust.rs`'s
    /// module doc comment): every JS class instance is reference-shared, so this needs no
    /// per-instance mutation/capture analysis, and every generated inherent method takes `&self`
    /// (never `&mut self`) — mutation happens through the shared `RefCell`, which is also
    /// exactly what makes capturing `self` into a nested closure (`const self = this;`, used for
    /// self-recursive/self-referencing closures like `array-buffer.ts`'s `shell`) just an
    /// ordinary cheap `.clone()` like any other captured value.
    ClassDef(ClassDef),
}

#[derive(Debug, Clone)]
pub struct ClassDef {
    pub name: String,
    /// `implements X` interface name(s), as written. TS re-emission reproduces this verbatim;
    /// Rust emission doesn't need it — the generated inherent methods simply exist, with no
    /// trait to satisfy (the source interface's own struct, if any, is never generated for a
    /// name that a class implements instead — see `lower.rs`).
    pub implements: Vec<String>,
    pub fields: Vec<ClassField>,
    /// `constructor(...) { this.#x = ...; ... }`. TS allows a class with no constructor; not
    /// observed in the surveyed source, but not rejected at the IR level either.
    pub constructor: Option<FnDecl>,
    pub methods: Vec<ClassMethod>,
}

#[derive(Debug, Clone)]
pub struct ClassField {
    pub name: String,
    pub is_private: bool,
    pub ty: TypeRef,
    /// An inline initializer (`#records = new WeakMap()`). Mutually exclusive with
    /// `optional`/`definite_assignment` in the surveyed source — a `!`/`?`-declared field is
    /// always set only from the constructor or later, never inline.
    pub init: Option<Expr>,
    /// `field?: T` — TS keeps this `T | undefined` at read sites. The Rust translation stores
    /// `Option<T>` internally either way (see `definite_assignment`'s doc comment), so this only
    /// affects whether a *read* stays `Option`-shaped (this flag) or unwraps (that one).
    pub optional: bool,
    /// `field!: T` (definite-assignment assertion): unset at construction, but TS asserts every
    /// read happens after it's been set from outside the constructor. The Rust translation still
    /// stores `Option<T>` internally (nothing is set at construction time either way) but a
    /// *read* unwraps rather than staying `Option`-shaped, trusting the same assertion the TS
    /// source already makes.
    pub definite_assignment: bool,
}

#[derive(Debug, Clone)]
pub struct ClassMethod {
    pub name: String,
    pub is_private: bool,
    pub func: FnDecl,
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
    /// `EXPR!` — a TypeScript non-null assertion. Every occurrence in the surveyed source
    /// asserts a `Map`/`WeakMap` `.get(...)` lookup (or a class method returning the equivalent
    /// `T | undefined`, e.g. `array-buffer.ts`'s `record`) is non-`undefined` at this point;
    /// `emit_rust.rs` trusts the same assertion the TS source already makes and lowers this to
    /// `.unwrap()` unconditionally, rather than re-deriving whether it's provably safe.
    /// `emit_ts.rs` re-emits the `!` verbatim.
    NonNull(Box<Expr>),
}

#[derive(Debug, Clone)]
pub enum MemberProp {
    Ident(String),
    Computed(Box<Expr>),
    /// `obj.#field` — private field/method access, legal only lexically inside (or nested
    /// within) the declaring class's own body. See `Item::ClassDef`.
    Private(String),
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
    Div,
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
