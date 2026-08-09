//! Bundle-level SWC preparation for Jade tenant implementations.
//!
//! This crate deliberately runs before the JIT/frontend collect tenant source.  It retains
//! selected `#private` fields, adds collision-resistant public computed getter/setter
//! properties that forward to them, and rewrites selected public tenant methods to use those
//! properties.  The existing tenant source extractor can then safely splice the rewritten
//! method without ever moving a `PrivateName` out of its class lexical scope.
//!
//! The helper-function portion initially supports a narrow, fail-closed subset: selected
//! top-level synchronous function declarations with simple parameters and captures of preceding
//! top-level `const` bindings. Every other helper is reported as skipped until resolver-aware
//! full capture analysis is implemented. In particular, this crate does not add runtime WASM
//! metadata probes.
//!
//! Input is parsed as TypeScript (`Syntax::Typescript`), a superset of the plain JS text a
//! live class/function's `Function.prototype.toString()` reflects at runtime — the same
//! parser handles both the build-time (real `.ts` source, real type annotations) and dynamic
//! collection-time (already-stripped JS) call sites without a syntax-mode branch.

use std::collections::{BTreeSet, HashMap, HashSet};

use swc_atoms::Atom;
use swc_common::sync::Lrc;
use swc_common::{DUMMY_SP, FileName, Mark, SourceMap};
use swc_ecma_ast::{
    AssignTarget, BlockStmt, Class, ClassMember, ClassMethod, ComputedPropName, Decl, Expr, Id,
    Ident, MemberExpr, MemberProp, MethodKind, Number, Param, Pat, PrivateName, Program, PropName,
    ReturnStmt, SimpleAssignTarget, Stmt, Str, ThisExpr, VarDeclKind,
};
use swc_ecma_codegen::text_writer::JsWriter;
use swc_ecma_codegen::{Config as CodegenConfig, Emitter, Node};
use swc_ecma_parser::lexer::Lexer;
use swc_ecma_parser::{Parser, StringInput, Syntax, TsSyntax};
use swc_ecma_transforms_base::resolver;
use swc_ecma_visit::{Visit, VisitMut, VisitMutWith, VisitWith};

/// Conservative policy for the bundle pre-pass.
///
/// Tenant classes and helpers are deliberately selected by their source binding names at this
/// public boundary.  A host which already has resolver `Id`s can map those to these names before
/// invoking the pre-pass; the rewrite itself never guesses based on method or field names.
#[derive(Debug, Clone, Default)]
pub struct TenantExposureConfig {
    /// Names of class declarations/expressions that are trusted tenant implementations.
    pub tenant_classes: BTreeSet<String>,
    /// Public tenant methods allowed to be rewritten. Empty means no methods are selected.
    pub tenant_methods: BTreeSet<String>,
    /// Selected helper bindings to expose when they satisfy the first conservative subset:
    /// a top-level sync function declaration with simple parameters and preceding top-level
    /// `const` captures.
    pub helper_functions: BTreeSet<String>,
    /// Stable prefix for generated accessor keys.
    pub mangling_namespace: String,
}

impl TenantExposureConfig {
    /// A conservative explicit configuration for the tenant operation methods currently known
    /// to the JIT source extractor.
    pub fn with_tenant_class(class: impl Into<String>) -> Self {
        Self {
            tenant_classes: [class.into()].into_iter().collect(),
            tenant_methods: ["make", "get", "set", "define", "assign", "ownKeys"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            helper_functions: Default::default(),
            mangling_namespace: "jade$tenant".to_owned(),
        }
    }

    fn namespace(&self) -> &str {
        if self.mangling_namespace.is_empty() {
            "jade$tenant"
        } else {
            &self.mangling_namespace
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExposureSkipReason {
    /// A helper was explicitly requested but does not satisfy the deliberately narrow first
    /// exposed-helper subset (top-level sync declaration, simple identifier parameters, and
    /// only preceding top-level `const` captures).
    UnsupportedHelper,
    /// A selected method touched a private field through an unsupported shape (e.g. a nested
    /// function or a non-`this` receiver). It is retained unchanged and therefore non-inlinable.
    UnsupportedPrivateUse,
    /// The generated public name collided with existing class surface and no deterministic
    /// alternate could be selected.
    AccessorNameCollision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExposedHelper {
    /// Guest-visible shim binding.
    pub name: String,
    /// Binding of the non-closing implementation a collector should inspect/invoke.
    pub implementation: String,
    /// Stable explicit environment-slot order for `implementation`'s leading `env` parameter.
    pub captures: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TenantExposureReport {
    /// Selected tenant classes whose source was changed.
    pub transformed_tenants: Vec<String>,
    /// Helpers for which collection may use the exposed implementation instead of the shim.
    pub exposed_helpers: Vec<ExposedHelper>,
    /// Fully qualified `Class.method` labels left untouched with a reason.
    pub skipped: Vec<(String, ExposureSkipReason)>,
}

/// Parse and transform one whole source bundle, returning normalized source and a non-fatal
/// report. Parse/codegen failures are strings because this is a collection/preparation boundary,
/// not runtime guest execution.
pub fn transform_bundle_source(
    source: &str,
    config: &TenantExposureConfig,
) -> Result<(String, TenantExposureReport), String> {
    swc_common::GLOBALS.set(&swc_common::Globals::new(), || {
        let cm: Lrc<SourceMap> = Default::default();
        let fm = cm.new_source_file(
            Lrc::new(FileName::Custom("tenant-bundle.ts".into())),
            source.to_owned(),
        );
        let lexer = Lexer::new(
            Syntax::Typescript(TsSyntax {
                tsx: false,
                decorators: false,
                ..Default::default()
            }),
            Default::default(),
            StringInput::from(&*fm),
            None,
        );
        let mut parser = Parser::new_from(lexer);
        let program = parser
            .parse_program()
            .map_err(|err| format!("failed to parse tenant bundle: {err:?}"))?;
        let mut resolved = resolve(program, true);
        let report = transform_program(&mut resolved, config);
        let program = resolved.into_inner();
        Ok((codegen_program(cm, &program)?, report))
    })
}

/// A [`Program`] known to have had swc's resolver pass run over it — every identifier's
/// `SyntaxContext` reflects real lexical binding identity, not source spelling alone. Only
/// [`resolve`] can construct one.
///
/// [`transform_program`] requires this rather than a plain `Program` so its free-variable and
/// capture decisions can rely on binding identity, matching the original design's explicit
/// requirement (`docs/swc-tenant-accessors-and-exposed-functions-plan.md`: "must use SWC
/// binding identity... not spelling alone, for every free-variable decision") as an enforced
/// precondition rather than a documented convention a caller could forget.
pub struct ResolvedProgram(Program);

impl ResolvedProgram {
    /// Unwrap back to the (now possibly transformed) `Program`, e.g. to hand to codegen.
    pub fn into_inner(self) -> Program {
        self.0
    }
}

/// Run swc's resolver pass over `program`, assigning real lexical `SyntaxContext`s to every
/// binding and reference. Must be called from inside an active `swc_common::GLOBALS` scope
/// (the same one used to parse `program`) — it allocates fresh [`Mark`]s, which panic outside
/// one. `typescript` should be `true` whenever `program` was parsed with `Syntax::Typescript`
/// (see swc's own `resolver` doc: enable it "if you are going to strip types or apply
/// type-aware passes").
pub fn resolve(mut program: Program, typescript: bool) -> ResolvedProgram {
    let unresolved_mark = Mark::new();
    let top_level_mark = Mark::new();
    program.visit_mut_with(&mut resolver(unresolved_mark, top_level_mark, typescript));
    ResolvedProgram(program)
}

/// Transform a resolved bundle in place. It only rewrites explicitly selected class names and
/// only direct `this.#field` occurrences in explicitly selected methods. That narrow policy
/// means it is safe to call in a bundle pipeline before the source is later reparsed/resolved
/// by tenant collection.
pub fn transform_program(
    program: &mut ResolvedProgram,
    config: &TenantExposureConfig,
) -> TenantExposureReport {
    let mut pass = TenantExposurePass {
        config,
        report: TenantExposureReport::default(),
    };
    program.0.visit_mut_with(&mut pass);
    transform_selected_helpers(&mut program.0, config, &mut pass.report);
    pass.report
}

struct TenantExposurePass<'a> {
    config: &'a TenantExposureConfig,
    report: TenantExposureReport,
}

impl VisitMut for TenantExposurePass<'_> {
    fn visit_mut_class_decl(&mut self, class_decl: &mut swc_ecma_ast::ClassDecl) {
        if self
            .config
            .tenant_classes
            .contains(class_decl.ident.sym.as_ref())
        {
            transform_tenant_class(
                &class_decl.ident.sym,
                &mut class_decl.class,
                self.config,
                &mut self.report,
            );
        }
        class_decl.visit_mut_children_with(self);
    }

    fn visit_mut_class_expr(&mut self, class_expr: &mut swc_ecma_ast::ClassExpr) {
        if let Some(name) = &class_expr.ident
            && self.config.tenant_classes.contains(name.sym.as_ref())
        {
            transform_tenant_class(
                &name.sym,
                &mut class_expr.class,
                self.config,
                &mut self.report,
            );
        }
        class_expr.visit_mut_children_with(self);
    }
}

fn transform_tenant_class(
    class_name: &Atom,
    class: &mut Class,
    config: &TenantExposureConfig,
    report: &mut TenantExposureReport,
) {
    let existing_names = public_member_names(class);
    let declared_instance_fields = private_instance_field_names(class);
    let declared_instance_methods = private_instance_method_names(class);
    let mut planned: HashMap<Atom, PlannedAccessor> = HashMap::new();
    let mut planned_forwarders: HashMap<Atom, String> = HashMap::new();
    let mut rewritten_any = false;

    for member in &mut class.body {
        let ClassMember::Method(method) = member else {
            continue;
        };
        let PropName::Ident(method_name) = &method.key else {
            continue;
        };
        if method.is_static
            || method.kind != MethodKind::Method
            || !config.tenant_methods.contains(method_name.sym.as_ref())
        {
            continue;
        }
        let Some(body) = &mut method.function.body else {
            continue;
        };

        let mut analyzer = PrivateUseAnalyzer::new(&declared_instance_methods);
        body.visit_mut_with(&mut analyzer);
        if analyzer.unsupported
            || analyzer
                .field_uses
                .keys()
                .any(|field| !declared_instance_fields.contains(field))
            || analyzer
                .method_call_uses
                .iter()
                .any(|name| !declared_instance_methods.contains(name))
        {
            report.skipped.push((
                format!("{class_name}.{}", method_name.sym),
                ExposureSkipReason::UnsupportedPrivateUse,
            ));
            continue;
        }
        if analyzer.field_uses.is_empty() && analyzer.method_call_uses.is_empty() {
            continue;
        }

        let mut keys = HashMap::new();
        let mut collision = false;
        for (field, write) in &analyzer.field_uses {
            let key = mangled_key(config.namespace(), class_name, field);
            if existing_names.contains(&key) && planned.get(field).is_none_or(|p| p.key != key) {
                collision = true;
                break;
            }
            let entry = planned
                .entry(field.clone())
                .or_insert_with(|| PlannedAccessor {
                    key: key.clone(),
                    needs_setter: false,
                });
            entry.needs_setter |= *write;
            keys.insert(field.clone(), key);
        }
        if !collision {
            for name in &analyzer.method_call_uses {
                let key = mangled_key(config.namespace(), class_name, name);
                if existing_names.contains(&key)
                    && planned_forwarders.get(name).is_none_or(|k| *k != key)
                {
                    collision = true;
                    break;
                }
                planned_forwarders.entry(name.clone()).or_insert_with(|| key.clone());
                keys.insert(name.clone(), key);
            }
        }
        if collision {
            report.skipped.push((
                format!("{class_name}.{}", method_name.sym),
                ExposureSkipReason::AccessorNameCollision,
            ));
            continue;
        }
        body.visit_mut_with(&mut PrivateUseRewriter { keys });
        rewritten_any = true;
    }

    if planned.is_empty() && planned_forwarders.is_empty() {
        return;
    }
    // Generated accessors/forwarders are appended after existing members. They retain lexical
    // access to the original private field/method; tenant source extraction never tries to
    // splice these helpers (their mangled names are never in `TENANT_METHOD_NAMES`).
    for (field, accessor) in planned {
        class.body.push(make_getter(&accessor.key, &field));
        if accessor.needs_setter {
            class.body.push(make_setter(&accessor.key, &field));
        }
    }
    for (method_name, key) in planned_forwarders {
        class.body.push(make_method_forwarder(&key, &method_name));
    }
    if rewritten_any {
        report.transformed_tenants.push(class_name.to_string());
    }
}

#[derive(Debug)]
struct PlannedAccessor {
    key: String,
    needs_setter: bool,
}

fn public_member_names(class: &Class) -> HashSet<String> {
    class
        .body
        .iter()
        .filter_map(|member| match member {
            ClassMember::Method(method) => prop_name_string(&method.key),
            ClassMember::ClassProp(prop) => prop_name_string(&prop.key),
            _ => None,
        })
        .collect()
}

fn private_instance_field_names(class: &Class) -> HashSet<Atom> {
    class
        .body
        .iter()
        .filter_map(|member| match member {
            ClassMember::PrivateProp(prop) if !prop.is_static => Some(prop.key.name.clone()),
            _ => None,
        })
        .collect()
}

/// Ordinary (non-getter/setter) private instance methods only — private accessors stay out of
/// scope, matching `docs/swc-tenant-accessors-and-exposed-functions-plan.md`'s "Out of scope
/// for version 1" list ("Private methods, private accessors...").  A private *getter*/*setter*
/// can't be forwarded the same way a call can (`this.#foo` vs `this.#foo()` have different
/// call-site shapes), so it's simply never added to this set and any reference to one still
/// falls through to the existing unsupported-private-use rejection.
fn private_instance_method_names(class: &Class) -> HashSet<Atom> {
    class
        .body
        .iter()
        .filter_map(|member| match member {
            ClassMember::PrivateMethod(method) if !method.is_static && method.kind == MethodKind::Method => {
                Some(method.key.name.clone())
            }
            _ => None,
        })
        .collect()
}

fn prop_name_string(key: &PropName) -> Option<String> {
    match key {
        PropName::Ident(name) => Some(name.sym.to_string()),
        PropName::Str(value) => Some(value.value.to_string_lossy().into_owned()),
        PropName::Computed(computed) => match &*computed.expr {
            Expr::Lit(swc_ecma_ast::Lit::Str(value)) => {
                Some(value.value.to_string_lossy().into_owned())
            }
            _ => None,
        },
        _ => None,
    }
}

/// Deterministic and source-stable; collisions are still checked before insertion. The class and
/// private field spelling appear only after a simple FNV-1a digest so generated names are
/// recognizable in debugging but are not normal user-facing API names.
fn mangled_key(namespace: &str, class_name: &Atom, field: &Atom) -> String {
    let input = format!("{namespace}\0{class_name}\0{field}");
    let mut hash = 0xcbf29ce484222325u64;
    for byte in input.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("__{namespace}${hash:016x}${field}")
}

struct PrivateUseAnalyzer<'a> {
    /// `(private field name, needs setter)`.
    field_uses: HashMap<Atom, bool>,
    /// Private instance methods called as `this.#name(...)` directly from the selected public
    /// method — see `make_method_forwarder`'s doc comment for why a *call* is the only shape
    /// that can be widened, and why a private helper method's own body never needs rewriting.
    method_call_uses: HashSet<Atom>,
    /// The class's own declared ordinary private methods (see `private_instance_method_names`) —
    /// needed here, not just in the post-analysis membership check, to tell a private *method*
    /// call apart from a private *field* holding a callable value (`this.#callableField(...)`),
    /// which must stay on the existing field-read path instead.
    known_private_methods: &'a HashSet<Atom>,
    unsupported: bool,
    nested_function_depth: usize,
}

impl<'a> PrivateUseAnalyzer<'a> {
    fn new(known_private_methods: &'a HashSet<Atom>) -> Self {
        Self {
            field_uses: HashMap::new(),
            method_call_uses: HashSet::new(),
            known_private_methods,
            unsupported: false,
            nested_function_depth: 0,
        }
    }

    fn direct_this_private(member: &MemberExpr) -> Option<Atom> {
        if matches!(&*member.obj, Expr::This(_))
            && let MemberProp::PrivateName(PrivateName { name, .. }) = &member.prop
        {
            Some(name.clone())
        } else {
            None
        }
    }

    fn record_member(&mut self, member: &MemberExpr, write: bool) {
        if self.nested_function_depth > 0 {
            if matches!(member.prop, MemberProp::PrivateName(_)) {
                self.unsupported = true;
            }
            return;
        }
        if let Some(field) = Self::direct_this_private(member) {
            self.field_uses
                .entry(field)
                .and_modify(|old| *old |= write)
                .or_insert(write);
        } else if matches!(member.prop, MemberProp::PrivateName(_)) {
            self.unsupported = true;
        }
    }
}

impl VisitMut for PrivateUseAnalyzer<'_> {
    fn visit_mut_class(&mut self, _class: &mut Class) {
        // A nested class owns a distinct private-name scope. It is never safe to infer that a
        // `this.#x` inside it refers to the selected tenant class.
        self.unsupported = true;
    }

    fn visit_mut_function(&mut self, function: &mut swc_ecma_ast::Function) {
        self.nested_function_depth += 1;
        function.visit_mut_children_with(self);
        self.nested_function_depth -= 1;
    }

    fn visit_mut_call_expr(&mut self, call: &mut swc_ecma_ast::CallExpr) {
        // `this.#privateMethod(...)`, only at depth 0 (matching every other direct-`this`-only,
        // no-nested-function rule in this analyzer): record it as a method-call use and recurse
        // only into the arguments, not the callee — visiting the callee generically afterward
        // would otherwise hit `visit_mut_member_expr` and mis-flag it as an unsupported private
        // field use (`#privateMethod` is never in `declared_instance_fields`). A callee that
        // resolves to a private *field* (a callable stored in a field, `this.#callableField(...)`)
        // deliberately falls through unchanged to the generic member-expr path below, staying on
        // the existing, already-supported field-read behavior.
        if self.nested_function_depth == 0
            && let swc_ecma_ast::Callee::Expr(callee) = &call.callee
            && let Expr::Member(member) = &**callee
            && let Some(name) = Self::direct_this_private(member)
            && self.known_private_methods.contains(&name)
        {
            self.method_call_uses.insert(name);
            call.args.visit_mut_with(self);
            return;
        }
        call.visit_mut_children_with(self);
    }

    fn visit_mut_member_expr(&mut self, member: &mut MemberExpr) {
        self.record_member(member, false);
        member.visit_mut_children_with(self);
    }

    fn visit_mut_assign_expr(&mut self, assign: &mut swc_ecma_ast::AssignExpr) {
        if let AssignTarget::Simple(SimpleAssignTarget::Member(member)) = &assign.left {
            self.record_member(member, true);
        }
        assign.visit_mut_children_with(self);
    }

    fn visit_mut_update_expr(&mut self, update: &mut swc_ecma_ast::UpdateExpr) {
        if let Expr::Member(member) = &*update.arg {
            self.record_member(member, true);
        }
        update.visit_mut_children_with(self);
    }
}

struct PrivateUseRewriter {
    keys: HashMap<Atom, String>,
}

impl PrivateUseRewriter {
    fn rewrite_member(&self, member: &mut MemberExpr) {
        if matches!(&*member.obj, Expr::This(_))
            && let MemberProp::PrivateName(private) = &member.prop
            && let Some(key) = self.keys.get(&private.name)
        {
            member.prop = member_prop_for(key);
        }
    }
}

impl VisitMut for PrivateUseRewriter {
    fn visit_mut_member_expr(&mut self, member: &mut MemberExpr) {
        self.rewrite_member(member);
        member.visit_mut_children_with(self);
    }
}

fn computed_key(key: &str) -> PropName {
    PropName::Computed(ComputedPropName {
        span: DUMMY_SP,
        expr: Box::new(Expr::Lit(swc_ecma_ast::Lit::Str(Str {
            span: DUMMY_SP,
            value: key.into(),
            raw: None,
        }))),
    })
}

fn member_prop_for(key: &str) -> MemberProp {
    MemberProp::Computed(ComputedPropName {
        span: DUMMY_SP,
        expr: Box::new(Expr::Lit(swc_ecma_ast::Lit::Str(Str {
            span: DUMMY_SP,
            value: key.into(),
            raw: None,
        }))),
    })
}

fn private_member(field: &Atom) -> Expr {
    Expr::Member(MemberExpr {
        span: DUMMY_SP,
        obj: Box::new(Expr::This(ThisExpr { span: DUMMY_SP })),
        prop: MemberProp::PrivateName(PrivateName {
            span: DUMMY_SP,
            name: field.clone(),
        }),
    })
}

fn make_getter(key: &str, field: &Atom) -> ClassMember {
    ClassMember::Method(ClassMethod {
        span: DUMMY_SP,
        key: computed_key(key),
        function: Box::new(swc_ecma_ast::Function {
            span: DUMMY_SP,
            body: Some(BlockStmt {
                span: DUMMY_SP,
                ctxt: Default::default(),
                stmts: vec![Stmt::Return(ReturnStmt {
                    span: DUMMY_SP,
                    arg: Some(Box::new(private_member(field))),
                })],
            }),
            ..Default::default()
        }),
        kind: MethodKind::Getter,
        is_static: false,
        accessibility: None,
        is_abstract: false,
        is_optional: false,
        is_override: false,
    })
}

fn make_setter(key: &str, field: &Atom) -> ClassMember {
    let value = Ident::new("value".into(), DUMMY_SP, Default::default());
    let assignment = swc_ecma_ast::AssignExpr {
        span: DUMMY_SP,
        op: swc_ecma_ast::AssignOp::Assign,
        left: AssignTarget::Simple(SimpleAssignTarget::Member(match private_member(field) {
            Expr::Member(member) => member,
            _ => unreachable!(),
        })),
        right: Box::new(Expr::Ident(value.clone())),
    };
    ClassMember::Method(ClassMethod {
        span: DUMMY_SP,
        key: computed_key(key),
        function: Box::new(swc_ecma_ast::Function {
            params: vec![Param {
                span: DUMMY_SP,
                decorators: vec![],
                pat: Pat::Ident(value.into()),
            }],
            span: DUMMY_SP,
            body: Some(BlockStmt {
                span: DUMMY_SP,
                ctxt: Default::default(),
                stmts: vec![Stmt::Expr(swc_ecma_ast::ExprStmt {
                    span: DUMMY_SP,
                    expr: Box::new(Expr::Assign(assignment)),
                })],
            }),
            ..Default::default()
        }),
        kind: MethodKind::Setter,
        is_static: false,
        accessibility: None,
        is_abstract: false,
        is_optional: false,
        is_override: false,
    })
}

/// Generates a public mangled forwarding *method* (not an accessor property) for a private
/// instance method the selected public tenant method calls directly (`this.#name(...)`):
/// `[MANGLED](...args) { return this.#name(...args); }`. Unlike the field getter/setter pair,
/// this forwards a *call*, not a property read/write — arity-agnostic rest-parameter forwarding
/// works regardless of `#name`'s own declared parameter list, and calling a generator/async
/// private method normally (no `yield*`/`await` here) already returns exactly the Generator/
/// Promise object the original call site expects, so no special-casing is needed for either.
///
/// Its own body stays lexically inside the class and is never itself spliced (its mangled key
/// is never added to `TENANT_METHOD_NAMES`), so `#name`'s own implementation — whatever private
/// fields or further private methods it happens to use — never needs to change or be traced any
/// further than this one level: only the *rewritten selected public method*'s call site needs a
/// syntax that's legal outside the class, and `this[MANGLED](...)` is exactly that, the same
/// "native member access, not a bespoke lowering" approach the field accessors already use.
fn make_method_forwarder(key: &str, method_name: &Atom) -> ClassMember {
    let args = Ident::new("args".into(), DUMMY_SP, Default::default());
    let call = Expr::Call(swc_ecma_ast::CallExpr {
        span: DUMMY_SP,
        ctxt: Default::default(),
        callee: swc_ecma_ast::Callee::Expr(Box::new(private_member(method_name))),
        args: vec![swc_ecma_ast::ExprOrSpread {
            spread: Some(DUMMY_SP),
            expr: Box::new(Expr::Ident(args.clone())),
        }],
        type_args: None,
    });
    ClassMember::Method(ClassMethod {
        span: DUMMY_SP,
        key: computed_key(key),
        function: Box::new(swc_ecma_ast::Function {
            params: vec![Param {
                span: DUMMY_SP,
                decorators: vec![],
                pat: Pat::Rest(swc_ecma_ast::RestPat {
                    span: DUMMY_SP,
                    dot3_token: DUMMY_SP,
                    arg: Box::new(Pat::Ident(args.into())),
                    type_ann: None,
                }),
            }],
            span: DUMMY_SP,
            body: Some(BlockStmt {
                span: DUMMY_SP,
                ctxt: Default::default(),
                stmts: vec![Stmt::Return(ReturnStmt {
                    span: DUMMY_SP,
                    arg: Some(Box::new(call)),
                })],
            }),
            ..Default::default()
        }),
        kind: MethodKind::Method,
        is_static: false,
        accessibility: None,
        is_abstract: false,
        is_optional: false,
        is_override: false,
    })
}

fn transform_selected_helpers(
    program: &mut Program,
    config: &TenantExposureConfig,
    report: &mut TenantExposureReport,
) {
    if config.helper_functions.is_empty() {
        return;
    }
    let mut appended = Vec::new();
    let Program::Script(script) = program else {
        for helper in &config.helper_functions {
            report
                .skipped
                .push((helper.clone(), ExposureSkipReason::UnsupportedHelper));
        }
        return;
    };
    let mut visible_consts: HashSet<Id> = HashSet::new();
    for stmt in &mut script.body {
        if let Stmt::Decl(Decl::Var(var)) = stmt
            && var.kind == VarDeclKind::Const
        {
            for declaration in &var.decls {
                if let Pat::Ident(binding) = &declaration.name {
                    visible_consts.insert(binding.id.to_id());
                }
            }
            continue;
        }
        let Stmt::Decl(Decl::Fn(function)) = stmt else {
            continue;
        };
        let name = function.ident.sym.to_string();
        if !config.helper_functions.contains(&name) {
            continue;
        }
        let Some(captures) = collect_helper_captures(&function.function, &visible_consts) else {
            report
                .skipped
                .push((name, ExposureSkipReason::UnsupportedHelper));
            continue;
        };
        let Some(params) = simple_param_names(&function.function) else {
            report
                .skipped
                .push((name, ExposureSkipReason::UnsupportedHelper));
            continue;
        };
        let impl_name = format!("__jade$exposed${name}");
        let Some(shim_body) = make_helper_shim_body(&name, &params) else {
            report
                .skipped
                .push((name, ExposureSkipReason::UnsupportedHelper));
            continue;
        };
        let mut implementation = (*function.function).clone();
        implementation.params.insert(
            0,
            Param {
                span: DUMMY_SP,
                decorators: vec![],
                pat: Pat::Ident(Ident::new("env".into(), DUMMY_SP, Default::default()).into()),
            },
        );
        implementation.visit_mut_with(&mut CaptureRewriter {
            captures: captures.clone(),
        });
        let capture_exprs = captures.join(", ");
        let definition = format!(
            "Object.defineProperty({name}, Symbol.for(\"jade.exposedFunction.v1\"), {{ value: {{ version: 1, kind: \"sync\", impl: {impl_name}, captures: [{capture_exprs}] }}, enumerable: false, writable: false, configurable: false }});"
        );
        match parse_script_fragment(&definition) {
            Ok(mut stmts) => {
                appended.push(Stmt::Decl(Decl::Fn(swc_ecma_ast::FnDecl {
                    ident: Ident::new(impl_name.clone().into(), DUMMY_SP, Default::default()),
                    declare: false,
                    function: Box::new(implementation),
                })));
                appended.append(&mut stmts);
                // Commit the observable shim only after generated metadata parsed too.
                function.function.body = Some(shim_body);
                report.exposed_helpers.push(ExposedHelper {
                    name,
                    implementation: impl_name,
                    captures,
                });
            }
            Err(_) => report
                .skipped
                .push((name, ExposureSkipReason::UnsupportedHelper)),
        }
    }
    script.body.append(&mut appended);
}

fn simple_param_names(function: &swc_ecma_ast::Function) -> Option<Vec<String>> {
    function
        .params
        .iter()
        .map(|param| match &param.pat {
            Pat::Ident(id) => Some(id.id.sym.to_string()),
            _ => None,
        })
        .collect()
}

fn make_helper_shim_body(name: &str, params: &[String]) -> Option<BlockStmt> {
    let args = std::iter::once("__jade_record.captures".to_owned())
        .chain(params.iter().cloned())
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        "function __jade_shim({}) {{ const __jade_record = {}[Symbol.for(\"jade.exposedFunction.v1\")]; if (new.target) return Reflect.construct(__jade_record.impl, [{}], new.target); return Reflect.apply(__jade_record.impl, this, [{}]); }}",
        params.join(", "),
        name,
        args,
        args,
    );
    let Program::Script(script) = parse_script_program(&source).ok()? else {
        return None;
    };
    let Stmt::Decl(Decl::Fn(function)) = script.body.into_iter().next()? else {
        return None;
    };
    function.function.body
}

fn collect_helper_captures(
    function: &swc_ecma_ast::Function,
    top_level_consts: &HashSet<Id>,
) -> Option<Vec<String>> {
    if function.is_async || function.is_generator || !function.decorators.is_empty() {
        return None;
    }
    let params = function
        .params
        .iter()
        .map(|param| match &param.pat {
            Pat::Ident(id) => Some(id.id.to_id()),
            _ => None,
        })
        .collect::<Option<HashSet<_>>>()?;
    let body = function.body.as_ref()?;
    let mut finder = HelperCaptureFinder {
        top_level_consts,
        params,
        captures: BTreeSet::new(),
        unsupported: false,
        nested_functions: 0,
    };
    body.visit_with(&mut finder);
    (!finder.unsupported).then(|| finder.captures.into_iter().collect())
}

/// Bindings are compared by resolved [`Id`] (`(Atom, SyntaxContext)`), not `Atom` spelling
/// alone — required so that, for example, an arrow function's own parameter of the same name
/// as a captured top-level `const` is recognized as a distinct binding rather than folded into
/// the capture. `swc_ecma_visit`'s default traversal doesn't treat `ArrowExpr` as a new
/// "nested function" the way `visit_function` below does for plain `function` expressions, so
/// without `Id`-based comparison an arrow parameter's own in-body references would spelling-
/// match the outer capture and `CaptureRewriter` would silently rewrite them to environment-slot
/// lookups. `Id`-based comparison makes that binding fail the `top_level_consts`/`params` check
/// (different `SyntaxContext`) and fall through to the unsupported-global-callee branch instead,
/// correctly rejecting the helper rather than silently miscompiling it. Requires `resolve()` to
/// have run over the program first (see `ResolvedProgram`) — before that, every identifier
/// shares the same empty `SyntaxContext` and `Id`-based comparison degrades to spelling-based.
struct HelperCaptureFinder<'a> {
    top_level_consts: &'a HashSet<Id>,
    params: HashSet<Id>,
    captures: BTreeSet<String>,
    unsupported: bool,
    nested_functions: usize,
}

impl Visit for HelperCaptureFinder<'_> {
    fn visit_function(&mut self, function: &swc_ecma_ast::Function) {
        self.nested_functions += 1;
        function.visit_children_with(self);
        self.nested_functions -= 1;
        self.unsupported = true;
    }

    fn visit_var_decl(&mut self, decl: &swc_ecma_ast::VarDecl) {
        // Local declarations require a real lexical binding pass to distinguish their own uses
        // from captures. Fail closed rather than using source spelling heuristics.
        self.unsupported = true;
        decl.visit_children_with(self);
    }

    fn visit_ident(&mut self, ident: &Ident) {
        if self.nested_functions > 0 || self.params.contains(&ident.to_id()) {
            return;
        }
        if self.top_level_consts.contains(&ident.to_id()) {
            self.captures.insert(ident.sym.to_string());
        } else if !matches!(
            ident.sym.as_ref(),
            "undefined" | "Math" | "Object" | "Array" | "Reflect" | "Promise"
        ) {
            self.unsupported = true;
        }
    }

    fn visit_member_expr(&mut self, member: &MemberExpr) {
        member.obj.visit_with(self);
        if let MemberProp::Computed(computed) = &member.prop {
            computed.expr.visit_with(self);
        }
    }
}

struct CaptureRewriter {
    captures: Vec<String>,
}

impl CaptureRewriter {
    fn rewrite_ident(&self, expr: &mut Expr) {
        let Expr::Ident(ident) = expr else {
            return;
        };
        let Some(index) = self
            .captures
            .iter()
            .position(|name| name == ident.sym.as_ref())
        else {
            return;
        };
        *expr = Expr::Member(MemberExpr {
            span: DUMMY_SP,
            obj: Box::new(Expr::Ident(Ident::new(
                "env".into(),
                DUMMY_SP,
                Default::default(),
            ))),
            prop: MemberProp::Computed(ComputedPropName {
                span: DUMMY_SP,
                expr: Box::new(Expr::Lit(swc_ecma_ast::Lit::Num(Number {
                    span: DUMMY_SP,
                    value: index as f64,
                    raw: None,
                }))),
            }),
        });
    }
}

impl VisitMut for CaptureRewriter {
    fn visit_mut_expr(&mut self, expr: &mut Expr) {
        self.rewrite_ident(expr);
        expr.visit_mut_children_with(self);
    }

    fn visit_mut_member_expr(&mut self, member: &mut MemberExpr) {
        member.obj.visit_mut_with(self);
        if let MemberProp::Computed(computed) = &mut member.prop {
            computed.expr.visit_mut_with(self);
        }
    }
}

fn parse_script_fragment(source: &str) -> Result<Vec<Stmt>, String> {
    let Program::Script(script) = parse_script_program(source)? else {
        return Err("generated exposure fragment unexpectedly parsed as a module".to_owned());
    };
    Ok(script.body)
}

fn parse_script_program(source: &str) -> Result<Program, String> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(
        Lrc::new(FileName::Custom("tenant-exposure-generated.js".into())),
        source.to_owned(),
    );
    let lexer = Lexer::new(
        Syntax::Es(Default::default()),
        Default::default(),
        StringInput::from(&*fm),
        None,
    );
    Parser::new_from(lexer)
        .parse_program()
        .map_err(|err| format!("failed to parse generated exposure fragment: {err:?}"))
}

fn codegen_program(cm: Lrc<SourceMap>, program: &Program) -> Result<String, String> {
    let mut buf = Vec::new();
    {
        let mut emitter = Emitter {
            cfg: CodegenConfig::default(),
            cm: cm.clone(),
            comments: None,
            wr: JsWriter::new(cm, "\n", &mut buf, None),
        };
        program
            .emit_with(&mut emitter)
            .map_err(|err| format!("failed to print transformed tenant bundle: {err:?}"))?;
    }
    String::from_utf8(buf).map_err(|err| format!("transformed source was not UTF-8: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use portal_solutions_jade_vm_frontend::tenant_inline::extract_tenant_methods;

    fn config() -> TenantExposureConfig {
        TenantExposureConfig::with_tenant_class("T")
    }

    #[test]
    fn private_field_stays_private_while_selected_method_uses_public_accessor() {
        let src = r#"
            class T {
                #shadow = new Map();
                *get(obj, key) { return this.#shadow.get(obj)?.[key]; }
                helper() { return this.#shadow; }
            }
        "#;
        assert!(extract_tenant_methods(src).get("get").is_none());
        let (out, report) = transform_bundle_source(src, &config()).unwrap();
        assert_eq!(report.transformed_tenants, vec!["T"]);
        assert!(
            out.contains("#shadow"),
            "private field/helper must remain: {out}"
        );
        assert!(
            out.contains("get [\"__jade$tenant$"),
            "missing generated getter: {out}"
        );
        let methods = extract_tenant_methods(&out);
        let get = methods
            .get("get")
            .expect("rewritten get should be extractable");
        assert!(
            !get.body_block.contains("#shadow"),
            "got: {}",
            get.body_block
        );
        assert!(
            get.body_block.contains("__jade$tenant$"),
            "got: {}",
            get.body_block
        );
    }

    #[test]
    fn writes_generate_a_setter_and_keep_native_update_syntax() {
        let src = r#"
            class T {
                #count = 0;
                *set(obj, key, value) { this.#count += value; return this.#count++; }
            }
        "#;
        let (out, _) = transform_bundle_source(src, &config()).unwrap();
        assert!(out.contains("get [\"__jade$tenant$"), "{out}");
        assert!(out.contains("set [\"__jade$tenant$"), "{out}");
        assert!(
            !extract_tenant_methods(&out)["set"]
                .body_block
                .contains("#count")
        );
        assert!(
            out.contains(" += value"),
            "compound assignment should remain native: {out}"
        );
        assert!(out.contains("++"), "update should remain native: {out}");
    }

    #[test]
    fn unselected_and_unsupported_methods_are_left_private() {
        let src = r#"
            class T {
                #x = 1;
                ordinary() { return this.#x; }
                *get() { return function() { return this.#x; }; }
            }
            class U { #x = 2; get() { return this.#x; } }
        "#;
        let (out, report) = transform_bundle_source(src, &config()).unwrap();
        assert!(report.transformed_tenants.is_empty());
        assert!(report.skipped.iter().any(|(name, reason)| name == "T.get"
            && *reason == ExposureSkipReason::UnsupportedPrivateUse));
        assert!(
            !out.contains("__jade$tenant$"),
            "no accessor expected: {out}"
        );
        assert!(out.contains("class U"));
    }

    #[test]
    fn does_not_create_accessors_for_unknown_private_names() {
        let src = r#"
            class T {
                #real = 1;
                *get() { return this.#missing; }
            }
        "#;
        let (out, report) = transform_bundle_source(src, &config()).unwrap();
        assert!(report.transformed_tenants.is_empty());
        assert!(report.skipped.iter().any(|(name, reason)| {
            name == "T.get" && *reason == ExposureSkipReason::UnsupportedPrivateUse
        }));
        assert!(
            !out.contains("__jade$tenant$"),
            "no accessor expected: {out}"
        );
    }

    #[test]
    fn helper_exposure_adds_non_closing_impl_and_v1_record() {
        let src = "const captured = Math; function helper(value) { return captured.floor(value); }";
        let mut cfg = config();
        cfg.helper_functions.insert("helper".to_owned());
        let (out, report) = transform_bundle_source(src, &cfg).unwrap();
        assert!(report.skipped.is_empty(), "unexpected skips: {report:?}");
        assert_eq!(
            report.exposed_helpers,
            vec![ExposedHelper {
                name: "helper".to_owned(),
                implementation: "__jade$exposed$helper".to_owned(),
                captures: vec!["captured".to_owned()],
            }]
        );
        assert!(
            out.contains("function helper(value) {\n    const __jade_record"),
            "{out}"
        );
        assert!(
            out.contains("Reflect.construct(__jade_record.impl, [\n        __jade_record.captures,\n        value\n    ], new.target)")
                || out.contains("Reflect.construct(__jade_record.impl, [__jade_record.captures, value], new.target)"),
            "{out}"
        );
        assert!(
            out.contains("Reflect.apply(__jade_record.impl, this"),
            "{out}"
        );
        assert!(
            out.contains("function __jade$exposed$helper(env, value)"),
            "{out}"
        );
        assert!(out.contains("env[0].floor(value)"), "{out}");
        assert!(out.contains("jade.exposedFunction.v1"), "{out}");
        assert!(
            out.contains("captures: [\n            captured\n        ]")
                || out.contains("captures: [captured]"),
            "{out}"
        );
    }

    #[test]
    fn unsupported_helper_exposure_fails_closed() {
        let src = "let captured = 1; function helper() { return captured; }";
        let mut cfg = config();
        cfg.helper_functions.insert("helper".to_owned());
        let (out, report) = transform_bundle_source(src, &cfg).unwrap();
        assert!(out.contains("function helper"));
        assert_eq!(
            report.skipped,
            vec![("helper".to_owned(), ExposureSkipReason::UnsupportedHelper)]
        );
    }

    #[test]
    fn typescript_type_annotations_parse_and_survive_the_transform() {
        // Matches the real shape in packages/jade-js/tenants/multi.ts: a typed private field
        // and a typed method parameter/return. Syntax::Es would reject this input outright.
        let src = r#"
            class T {
                #shadow: Map<string, number> = new Map();
                *get(key: string): Generator<any, number | undefined, any> {
                    return this.#shadow.get(key);
                }
            }
        "#;
        let (out, report) = transform_bundle_source(src, &config()).unwrap();
        assert_eq!(report.transformed_tenants, vec!["T"]);
        assert!(
            out.contains("#shadow: Map<string, number>"),
            "private field type annotation should survive: {out}"
        );
        assert!(
            out.contains("key: string"),
            "parameter type annotation should survive: {out}"
        );
        assert!(
            out.contains("Generator<any, number | undefined, any>"),
            "return type annotation should survive: {out}"
        );
        assert!(out.contains("get [\"__jade$tenant$"), "missing generated getter: {out}");
    }

    #[test]
    fn arrow_function_parameter_shadowing_a_capture_fails_closed_not_silently() {
        // Before Id-based (resolver-backed) comparison in `HelperCaptureFinder`, this helper
        // would have been silently accepted: the arrow's own `captured` parameter spelling-
        // matches the outer top-level const, so it would have been misclassified as a capture,
        // and `CaptureRewriter` would have rewritten every `captured` inside the arrow body —
        // including its own parameter's uses — to an environment-slot lookup. That changes
        // `.map`'s callback from "use each array element" to "always return the captured
        // Math object", a silent miscompile. Resolver-assigned `SyntaxContext`s distinguish
        // the two `captured` bindings, so this now correctly fails closed instead.
        let src = r#"
            const captured = Math;
            function helper(value) {
                return [1, 2, 3].map((captured) => captured.toString()).join(value);
            }
        "#;
        let mut cfg = config();
        cfg.helper_functions.insert("helper".to_owned());
        let (out, report) = transform_bundle_source(src, &cfg).unwrap();
        assert!(out.contains("function helper"), "{out}");
        assert!(
            !out.contains("jade.exposedFunction.v1"),
            "helper must not be exposed when a capture name is shadowed: {out}"
        );
        assert_eq!(
            report.skipped,
            vec![("helper".to_owned(), ExposureSkipReason::UnsupportedHelper)]
        );
    }

    #[test]
    fn public_method_calling_a_private_helper_method_gets_a_forwarder() {
        // Mirrors packages/jade-js/tenants/multi.ts's real shape: `get` never touches
        // `#shadowForKey`'s own private field directly, only calls the private *method* that
        // wraps it. Before this feature, that shape was unconditionally rejected as
        // UnsupportedPrivateUse (private methods were listed out of v1 scope) even though the
        // field-accessor mechanism could easily have covered it with the analogous "forward a
        // call" treatment.
        let src = r#"
            class T {
                #shadow = new Map();
                #shadowForKey(key) {
                    return this.#shadow.get(key) ?? this.#shadow.set(key, new Map()).get(key);
                }
                *get(obj, key) { return this.#shadowForKey(key).get(obj); }
            }
        "#;
        let (out, report) = transform_bundle_source(src, &config()).unwrap();
        assert_eq!(report.transformed_tenants, vec!["T"]);
        // The private method and its own body are untouched and stay private.
        assert!(out.contains("#shadowForKey(key)"), "{out}");
        assert!(
            out.contains("this.#shadow.get(key)"),
            "the forwarder's own body must keep using the real private field: {out}"
        );
        // A new public mangled forwarding method was generated...
        assert!(
            out.contains("[\"__jade$tenant$") && out.contains("](...args)"),
            "missing generated forwarder: {out}"
        );
        // ...and `get`'s call site was rewritten to use it instead of the private method name.
        let methods = extract_tenant_methods(&out);
        let get = methods.get("get").expect("rewritten get should be extractable");
        assert!(
            !get.body_block.contains("#shadowForKey"),
            "got: {}",
            get.body_block
        );
        assert!(
            get.body_block.contains("__jade$tenant$"),
            "got: {}",
            get.body_block
        );
    }

    #[test]
    fn private_helper_method_referenced_without_being_called_stays_unsupported() {
        // `#shadowForKey` is passed as a bare value here, never called — no call-forwarding
        // wrapper can represent that, so this must still fail closed exactly like it did before
        // the forwarding feature existed, not be silently accepted now that the class has a
        // known private method by that name.
        let src = r#"
            class T {
                #shadow = new Map();
                #shadowForKey(key) { return this.#shadow.get(key); }
                *get(obj, key) { const fn = this.#shadowForKey; return fn.call(this, key); }
            }
        "#;
        let (out, report) = transform_bundle_source(src, &config()).unwrap();
        assert!(report.transformed_tenants.is_empty());
        assert!(report.skipped.iter().any(|(name, reason)| name == "T.get"
            && *reason == ExposureSkipReason::UnsupportedPrivateUse));
        assert!(!out.contains("__jade$tenant$"), "no accessor expected: {out}");
    }
}
