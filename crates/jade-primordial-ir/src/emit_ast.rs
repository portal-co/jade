//! IR -> real `swc_ecma_ast` construction, mirroring `emit_ts.rs`'s coverage node-for-node but
//! producing genuine AST rather than text. This is what makes in-process composition with
//! `jade-swc-tenant-exposure::transform_program` possible (see
//! `docs/bundle-pass-build-step-plan.md`): that crate's entry point already accepts a
//! `swc_ecma_ast::Program`, so handing it real nodes built here avoids a parse -> emit-text ->
//! reparse round trip.
//!
//! `emit_ts.rs`'s doc comment explains why it emits text directly: nothing needed real AST
//! before now. This module is the AST-shaped sibling that fills that gap; `emit_ts.rs` remains
//! in place as the parity oracle during migration (see the plan's "Migrating `emit_ts.rs`"
//! section) rather than being deleted outright in one pass.
//!
//! Known gap carried over unchanged from `emit_ts.rs`, not introduced here: neither emitter
//! marks any top-level `Item::FnDecl`/`Item::ClassDef`/`Item::ModuleConst` as `export`ed (only
//! `StructDef`/`StringEnumDef` get `export`). That means today's regenerated output cannot
//! actually be imported by another module — harmless for a single-file `tsc --noEmit` check,
//! but a real blocker for shipping a generated bundle other files import from (see the plan's
//! orchestrator, which needs primordial exports like `objectPrimordial` to resolve). The IR
//! itself has no exported-ness field to draw on (`ir::Item` variants carry no visibility flag),
//! so fixing this needs an IR change, not just an emitter change — tracked as follow-up, not
//! fixed here, to keep this module's job to "match what the IR already says," faithfully.

use crate::ir;
use swc_atoms::Atom;
use swc_common::DUMMY_SP;
use swc_ecma_ast::*;

pub fn emit_module(module: &ir::Module) -> Module {
    let body = module.items.iter().flat_map(emit_item).collect();
    Module {
        span: DUMMY_SP,
        body,
        shebang: None,
    }
}

/// Print a [`Module`] (real AST, e.g. from [`emit_module`]) back to TypeScript text via
/// `swc_ecma_codegen` — the one text-serialization point the build-step plan calls for,
/// distinct from `emit_ts.rs`'s from-scratch string builder. Every span here is `DUMMY_SP`
/// (this module never sees a real `SourceMap`), so the fresh, empty one built here only
/// backs the writer's line/column bookkeeping, not span provenance.
pub fn print_module(module: &Module) -> String {
    use swc_common::sync::Lrc;
    use swc_common::SourceMap;
    use swc_ecma_codegen::text_writer::JsWriter;
    use swc_ecma_codegen::{Config as CodegenConfig, Emitter, Node};

    let cm: Lrc<SourceMap> = Default::default();
    let mut buf = Vec::new();
    {
        let mut emitter = Emitter {
            cfg: CodegenConfig::default(),
            cm: cm.clone(),
            comments: None,
            wr: JsWriter::new(cm, "\n", &mut buf, None),
        };
        module
            .emit_with(&mut emitter)
            .expect("printing a module built entirely from real AST nodes should never fail");
    }
    String::from_utf8(buf).expect("swc_ecma_codegen always writes valid UTF-8")
}

fn ident(name: &str) -> Ident {
    Ident::new(Atom::from(name), DUMMY_SP, Default::default())
}

fn binding_ident(name: &str) -> BindingIdent {
    BindingIdent {
        id: ident(name),
        type_ann: None,
    }
}

fn str_lit(value: &str) -> Str {
    Str {
        span: DUMMY_SP,
        value: Atom::from(value).into(),
        raw: None,
    }
}

fn block(block: &ir::Block) -> BlockStmt {
    BlockStmt {
        span: DUMMY_SP,
        ctxt: Default::default(),
        stmts: block.0.iter().map(emit_stmt).collect(),
    }
}

fn emit_item(item: &ir::Item) -> Vec<ModuleItem> {
    match item {
        ir::Item::TypeImport { source, names } => vec![ModuleItem::ModuleDecl(ModuleDecl::Import(
            import_decl(source, names, true),
        ))],
        ir::Item::ValueImport { source, names } => vec![ModuleItem::ModuleDecl(ModuleDecl::Import(
            import_decl(source, names, false),
        ))],
        ir::Item::StructDef(def) => vec![ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl {
            span: DUMMY_SP,
            decl: Decl::TsInterface(Box::new(emit_struct_def(def))),
        }))],
        ir::Item::PerTenantCache {
            name,
            value_ty,
            identity_wrapped,
        } => vec![ModuleItem::Stmt(Stmt::Decl(Decl::Var(Box::new(
            per_tenant_cache_decl(name, value_ty, *identity_wrapped),
        ))))],
        ir::Item::ModuleConst { name, mutable, init } => {
            vec![ModuleItem::Stmt(Stmt::Decl(Decl::Var(Box::new(VarDecl {
                span: DUMMY_SP,
                ctxt: Default::default(),
                kind: if *mutable {
                    VarDeclKind::Let
                } else {
                    VarDeclKind::Const
                },
                declare: false,
                decls: vec![VarDeclarator {
                    span: DUMMY_SP,
                    name: Pat::Ident(binding_ident(name)),
                    init: Some(Box::new(emit_expr(init))),
                    definite: false,
                }],
            }))))]
        }
        ir::Item::FnDecl(func) => vec![ModuleItem::Stmt(Stmt::Decl(Decl::Fn(emit_top_level_fn_decl(
            func,
        ))))],
        ir::Item::ClassDef(def) => vec![ModuleItem::Stmt(Stmt::Decl(Decl::Class(emit_class_decl(def))))],
        ir::Item::StringEnumDef(def) => vec![ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl {
            span: DUMMY_SP,
            decl: Decl::TsTypeAlias(Box::new(emit_string_enum_def(def))),
        }))],
    }
}

fn import_decl(source: &str, names: &[String], type_only: bool) -> ImportDecl {
    ImportDecl {
        span: DUMMY_SP,
        specifiers: names
            .iter()
            .map(|name| {
                ImportSpecifier::Named(ImportNamedSpecifier {
                    span: DUMMY_SP,
                    local: ident(name),
                    imported: None,
                    is_type_only: false,
                })
            })
            .collect(),
        src: Box::new(str_lit(source)),
        type_only,
        with: None,
        phase: ImportPhase::Evaluation,
    }
}

fn emit_struct_def(def: &ir::StructDef) -> TsInterfaceDecl {
    TsInterfaceDecl {
        span: DUMMY_SP,
        id: ident(&def.name),
        declare: false,
        type_params: None,
        extends: vec![],
        body: TsInterfaceBody {
            span: DUMMY_SP,
            body: def
                .fields
                .iter()
                .map(|(name, ty)| {
                    TsTypeElement::TsPropertySignature(TsPropertySignature {
                        span: DUMMY_SP,
                        readonly: false,
                        key: Box::new(Expr::Ident(ident(name))),
                        computed: false,
                        optional: false,
                        type_ann: Some(Box::new(ts_type_ann(ty))),
                    })
                })
                .collect(),
        },
    }
}

fn emit_string_enum_def(def: &ir::StringEnumDef) -> TsTypeAliasDecl {
    let mut variants = def
        .variants
        .iter()
        .map(|v| {
            TsType::TsLitType(TsLitType {
                span: DUMMY_SP,
                lit: TsLit::Str(str_lit(v)),
            })
        })
        .peekable();
    let type_ann = if def.variants.len() == 1 {
        variants.next().expect("checked len == 1")
    } else {
        TsType::TsUnionOrIntersectionType(TsUnionOrIntersectionType::TsUnionType(TsUnionType {
            span: DUMMY_SP,
            types: variants.map(Box::new).collect(),
        }))
    };
    TsTypeAliasDecl {
        span: DUMMY_SP,
        declare: false,
        id: ident(&def.name),
        type_params: None,
        type_ann: Box::new(type_ann),
    }
}

/// The `const cache = new WeakMap<Tenant, X>()` (or the identity-wrapped variant) idiom, built
/// directly rather than through `emit_expr`/`ts_type_ref` — its `new WeakMap<...>()` type
/// argument list isn't representable as an `ir::Expr`, only as this item's own fixed shape.
fn per_tenant_cache_decl(name: &str, value_ty: &str, identity_wrapped: bool) -> VarDecl {
    let value_type = if identity_wrapped {
        TsType::TsTypeLit(TsTypeLit {
            span: DUMMY_SP,
            members: vec![
                TsTypeElement::TsPropertySignature(TsPropertySignature {
                    span: DUMMY_SP,
                    readonly: false,
                    key: Box::new(Expr::Ident(ident("identity"))),
                    computed: false,
                    optional: false,
                    type_ann: Some(Box::new(TsTypeAnn {
                        span: DUMMY_SP,
                        type_ann: Box::new(ts_named_type("object")),
                    })),
                }),
                TsTypeElement::TsPropertySignature(TsPropertySignature {
                    span: DUMMY_SP,
                    readonly: false,
                    key: Box::new(Expr::Ident(ident("primordial"))),
                    computed: false,
                    optional: false,
                    type_ann: Some(Box::new(TsTypeAnn {
                        span: DUMMY_SP,
                        type_ann: Box::new(ts_named_type(value_ty)),
                    })),
                }),
            ],
        })
    } else {
        ts_named_type(value_ty)
    };
    VarDecl {
        span: DUMMY_SP,
        ctxt: Default::default(),
        kind: VarDeclKind::Const,
        declare: false,
        decls: vec![VarDeclarator {
            span: DUMMY_SP,
            name: Pat::Ident(binding_ident(name)),
            init: Some(Box::new(Expr::New(NewExpr {
                span: DUMMY_SP,
                ctxt: Default::default(),
                callee: Box::new(Expr::Ident(ident("WeakMap"))),
                args: Some(vec![]),
                type_args: Some(Box::new(TsTypeParamInstantiation {
                    span: DUMMY_SP,
                    params: vec![Box::new(ts_named_type("Tenant")), Box::new(value_type)],
                })),
            }))),
            definite: false,
        }],
    }
}

fn ts_named_type(name: &str) -> TsType {
    TsType::TsTypeRef(TsTypeRef {
        span: DUMMY_SP,
        type_name: TsEntityName::Ident(ident(name)),
        type_params: None,
    })
}

fn ts_type_ann(ty: &ir::TypeRef) -> TsTypeAnn {
    TsTypeAnn {
        span: DUMMY_SP,
        type_ann: Box::new(emit_type_ref(ty)),
    }
}

/// Mirrors `emit_ts.rs::emit_type_ref` exactly, including its `__ApplyClosure` sentinel
/// special-case (see that function's own doc comment for why the fixed text is safe here).
fn emit_type_ref(ty: &ir::TypeRef) -> TsType {
    match ty {
        ir::TypeRef::Named(name) if name == "__ApplyClosure" => {
            TsType::TsFnOrConstructorType(TsFnOrConstructorType::TsFnType(TsFnType {
                span: DUMMY_SP,
                params: vec![
                    ts_fn_param("thisArg", ts_keyword(TsKeywordTypeKind::TsUnknownKeyword)),
                    TsFnParam::Ident(BindingIdent {
                        id: ident("args"),
                        type_ann: Some(Box::new(TsTypeAnn {
                            span: DUMMY_SP,
                            type_ann: Box::new(TsType::TsArrayType(TsArrayType {
                                span: DUMMY_SP,
                                elem_type: Box::new(ts_keyword(TsKeywordTypeKind::TsUnknownKeyword)),
                            })),
                        })),
                    }),
                ],
                type_params: None,
                type_ann: Box::new(TsTypeAnn {
                    span: DUMMY_SP,
                    type_ann: Box::new(TsType::TsTypeRef(TsTypeRef {
                        span: DUMMY_SP,
                        type_name: TsEntityName::Ident(ident("TenantGenerator")),
                        type_params: Some(Box::new(TsTypeParamInstantiation {
                            span: DUMMY_SP,
                            params: vec![Box::new(ts_keyword(TsKeywordTypeKind::TsUnknownKeyword))],
                        })),
                    })),
                }),
            }))
        }
        ir::TypeRef::Named(name) => ts_named_type(name),
        ir::TypeRef::Generic { name, args } => TsType::TsTypeRef(TsTypeRef {
            span: DUMMY_SP,
            type_name: TsEntityName::Ident(ident(name)),
            type_params: Some(Box::new(TsTypeParamInstantiation {
                span: DUMMY_SP,
                params: args.iter().map(|a| Box::new(emit_type_ref(a))).collect(),
            })),
        }),
        ir::TypeRef::Optional(inner) => {
            TsType::TsUnionOrIntersectionType(TsUnionOrIntersectionType::TsUnionType(TsUnionType {
                span: DUMMY_SP,
                types: vec![
                    Box::new(emit_type_ref(inner)),
                    Box::new(ts_keyword(TsKeywordTypeKind::TsNullKeyword)),
                ],
            }))
        }
        ir::TypeRef::Array(inner) => TsType::TsArrayType(TsArrayType {
            span: DUMMY_SP,
            elem_type: Box::new(emit_type_ref(inner)),
        }),
    }
}

fn ts_keyword(kind: TsKeywordTypeKind) -> TsType {
    TsType::TsKeywordType(TsKeywordType {
        span: DUMMY_SP,
        kind,
    })
}

fn ts_fn_param(name: &str, ty: TsType) -> TsFnParam {
    TsFnParam::Ident(BindingIdent {
        id: ident(name),
        type_ann: Some(Box::new(TsTypeAnn {
            span: DUMMY_SP,
            type_ann: Box::new(ty),
        })),
    })
}

fn emit_class_decl(def: &ir::ClassDef) -> ClassDecl {
    ClassDecl {
        ident: ident(&def.name),
        declare: false,
        class: Box::new(emit_class(def)),
    }
}

fn emit_class(def: &ir::ClassDef) -> Class {
    let mut body = Vec::new();
    for field in &def.fields {
        body.push(emit_class_field(field));
    }
    if let Some(ctor) = &def.constructor {
        body.push(ClassMember::Constructor(Constructor {
            span: DUMMY_SP,
            ctxt: Default::default(),
            key: PropName::Ident(IdentName::new("constructor".into(), DUMMY_SP)),
            params: ctor.params.iter().map(emit_param_or_ts_param_prop).collect(),
            body: Some(block(&ctor.body)),
            accessibility: None,
            is_optional: false,
        }));
    }
    for method in &def.methods {
        body.push(ClassMember::Method(emit_class_method(method)));
    }
    Class {
        span: DUMMY_SP,
        ctxt: Default::default(),
        decorators: vec![],
        body,
        super_class: None,
        is_abstract: false,
        type_params: None,
        super_type_params: None,
        implements: def
            .implements
            .iter()
            .map(|name| TsExprWithTypeArgs {
                span: DUMMY_SP,
                expr: Box::new(Expr::Ident(ident(name))),
                type_args: None,
            })
            .collect(),
    }
}

fn emit_class_field(field: &ir::ClassField) -> ClassMember {
    let type_ann = Some(Box::new(ts_type_ann(&field.ty)));
    let value = field.init.as_ref().map(|e| Box::new(emit_expr(e)));
    if field.is_private {
        ClassMember::PrivateProp(PrivateProp {
            span: DUMMY_SP,
            ctxt: Default::default(),
            key: PrivateName {
                span: DUMMY_SP,
                name: Atom::from(field.name.as_str()),
            },
            value,
            type_ann,
            is_static: false,
            decorators: vec![],
            accessibility: None,
            is_optional: field.optional,
            is_override: false,
            readonly: false,
            definite: field.definite_assignment,
        })
    } else {
        ClassMember::ClassProp(ClassProp {
            span: DUMMY_SP,
            key: PropName::Ident(IdentName::new(field.name.as_str().into(), DUMMY_SP)),
            value,
            type_ann,
            is_static: false,
            decorators: vec![],
            accessibility: None,
            is_abstract: false,
            is_optional: field.optional,
            is_override: false,
            readonly: false,
            declare: false,
            definite: field.definite_assignment,
        })
    }
}

fn emit_class_method(method: &ir::ClassMethod) -> ClassMethod {
    let key = if method.is_private {
        PropName::Computed(ComputedPropName {
            span: DUMMY_SP,
            expr: Box::new(Expr::PrivateName(PrivateName {
                span: DUMMY_SP,
                name: Atom::from(method.name.as_str()),
            })),
        })
    } else {
        PropName::Ident(IdentName::new(method.name.as_str().into(), DUMMY_SP))
    };
    ClassMethod {
        span: DUMMY_SP,
        key,
        function: Box::new(emit_function(&method.func)),
        kind: MethodKind::Method,
        is_static: false,
        accessibility: None,
        is_abstract: false,
        is_optional: false,
        is_override: false,
    }
}

fn emit_top_level_fn_decl(func: &ir::FnDecl) -> FnDecl {
    FnDecl {
        ident: ident(func.name.as_deref().unwrap_or("")),
        declare: false,
        function: Box::new(emit_function(func)),
    }
}

fn emit_function(func: &ir::FnDecl) -> Function {
    Function {
        params: func.params.iter().map(emit_param).collect(),
        decorators: vec![],
        span: DUMMY_SP,
        ctxt: Default::default(),
        body: Some(block(&func.body)),
        is_generator: func.is_generator,
        is_async: false,
        type_params: None,
        return_type: func
            .return_type
            .as_ref()
            .map(|ty| Box::new(ts_type_ann(ty))),
    }
}

fn emit_param(param: &ir::Param) -> Param {
    Param {
        span: DUMMY_SP,
        decorators: vec![],
        pat: emit_pattern_typed(&param.pattern, param.ty.as_ref(), param.default.as_ref()),
    }
}

fn emit_param_or_ts_param_prop(param: &ir::Param) -> ParamOrTsParamProp {
    ParamOrTsParamProp::Param(emit_param(param))
}

fn emit_pattern_typed(pattern: &ir::Pattern, ty: Option<&ir::TypeRef>, default: Option<&ir::Expr>) -> Pat {
    let base = match pattern {
        ir::Pattern::Ident(name) => Pat::Ident(BindingIdent {
            id: ident(name),
            type_ann: ty.map(|t| Box::new(ts_type_ann(t))),
        }),
        ir::Pattern::ObjectShallow(bindings) => Pat::Object(ObjectPat {
            span: DUMMY_SP,
            props: bindings
                .iter()
                .map(|binding| {
                    if binding.key == binding.binding {
                        ObjectPatProp::Assign(AssignPatProp {
                            span: DUMMY_SP,
                            key: BindingIdent {
                                id: ident(&binding.key),
                                type_ann: None,
                            },
                            value: None,
                        })
                    } else {
                        ObjectPatProp::KeyValue(KeyValuePatProp {
                            key: PropName::Ident(IdentName::new(binding.key.as_str().into(), DUMMY_SP)),
                            value: Box::new(Pat::Ident(binding_ident(&binding.binding))),
                        })
                    }
                })
                .collect(),
            optional: false,
            type_ann: ty.map(|t| Box::new(ts_type_ann(t))),
        }),
    };
    match default {
        Some(default) => Pat::Assign(AssignPat {
            span: DUMMY_SP,
            left: Box::new(base),
            right: Box::new(emit_expr(default)),
        }),
        None => base,
    }
}

fn emit_stmt(stmt: &ir::Stmt) -> Stmt {
    match stmt {
        ir::Stmt::Let { pattern, init } => Stmt::Decl(Decl::Var(Box::new(VarDecl {
            span: DUMMY_SP,
            ctxt: Default::default(),
            kind: VarDeclKind::Const,
            declare: false,
            decls: vec![VarDeclarator {
                span: DUMMY_SP,
                name: emit_pattern_typed(pattern, None, None),
                init: init.as_ref().map(|e| Box::new(emit_expr(e))),
                definite: false,
            }],
        }))),
        ir::Stmt::Expr(expr) => Stmt::Expr(ExprStmt {
            span: DUMMY_SP,
            expr: Box::new(emit_expr(expr)),
        }),
        ir::Stmt::Return(value) => Stmt::Return(ReturnStmt {
            span: DUMMY_SP,
            arg: value.as_ref().map(|e| Box::new(emit_expr(e))),
        }),
        ir::Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => Stmt::If(IfStmt {
            span: DUMMY_SP,
            test: Box::new(emit_expr(cond)),
            cons: Box::new(Stmt::Block(block(then_branch))),
            alt: else_branch.as_ref().map(|b| Box::new(Stmt::Block(block(b)))),
        }),
        ir::Stmt::ForOf { binding, iter, body } => Stmt::ForOf(ForOfStmt {
            span: DUMMY_SP,
            is_await: false,
            left: ForHead::VarDecl(Box::new(VarDecl {
                span: DUMMY_SP,
                ctxt: Default::default(),
                kind: VarDeclKind::Const,
                declare: false,
                decls: vec![VarDeclarator {
                    span: DUMMY_SP,
                    name: emit_pattern_typed(binding, None, None),
                    init: None,
                    definite: false,
                }],
            })),
            right: Box::new(emit_expr(iter)),
            body: Box::new(Stmt::Block(block(body))),
        }),
        ir::Stmt::ForCounting {
            binding,
            start,
            bound,
            body,
        } => Stmt::For(ForStmt {
            span: DUMMY_SP,
            init: Some(VarDeclOrExpr::VarDecl(Box::new(VarDecl {
                span: DUMMY_SP,
                ctxt: Default::default(),
                kind: VarDeclKind::Let,
                declare: false,
                decls: vec![VarDeclarator {
                    span: DUMMY_SP,
                    name: Pat::Ident(binding_ident(binding)),
                    init: Some(Box::new(emit_expr(start))),
                    definite: false,
                }],
            }))),
            test: Some(Box::new(Expr::Bin(BinExpr {
                span: DUMMY_SP,
                op: BinaryOp::Lt,
                left: Box::new(Expr::Ident(ident(binding))),
                right: Box::new(emit_expr(bound)),
            }))),
            update: Some(Box::new(Expr::Update(UpdateExpr {
                span: DUMMY_SP,
                op: UpdateOp::PlusPlus,
                prefix: false,
                arg: Box::new(Expr::Ident(ident(binding))),
            }))),
            body: Box::new(Stmt::Block(block(body))),
        }),
        ir::Stmt::TryCatch {
            try_block,
            catch_param,
            catch_block,
        } => Stmt::Try(Box::new(TryStmt {
            span: DUMMY_SP,
            block: block(try_block),
            handler: Some(CatchClause {
                span: DUMMY_SP,
                param: catch_param.as_ref().map(|p| Pat::Ident(binding_ident(p))),
                body: block(catch_block),
            }),
            finalizer: None,
        })),
        ir::Stmt::Throw(expr) => Stmt::Throw(ThrowStmt {
            span: DUMMY_SP,
            arg: Box::new(emit_expr(expr)),
        }),
        ir::Stmt::Continue => Stmt::Continue(ContinueStmt {
            span: DUMMY_SP,
            label: None,
        }),
    }
}

fn emit_expr(expr: &ir::Expr) -> Expr {
    match expr {
        ir::Expr::Ident(name) => Expr::Ident(ident(name)),
        ir::Expr::ThisArg => Expr::This(ThisExpr { span: DUMMY_SP }),
        ir::Expr::Lit(lit) => emit_lit(lit),
        ir::Expr::TemplateLiteral(parts) => emit_template(parts),
        ir::Expr::Array(elements) => Expr::Array(ArrayLit {
            span: DUMMY_SP,
            elems: elements
                .iter()
                .map(|el| {
                    Some(match el {
                        ir::ArrayElement::Normal(e) => ExprOrSpread {
                            spread: None,
                            expr: Box::new(emit_expr(e)),
                        },
                        ir::ArrayElement::Spread(e) => ExprOrSpread {
                            spread: Some(DUMMY_SP),
                            expr: Box::new(emit_expr(e)),
                        },
                    })
                })
                .collect(),
        }),
        ir::Expr::Object(props) => Expr::Object(ObjectLit {
            span: DUMMY_SP,
            props: props.iter().map(emit_object_prop).collect(),
        }),
        ir::Expr::Member { obj, prop } => Expr::Member(MemberExpr {
            span: DUMMY_SP,
            obj: Box::new(emit_expr(obj)),
            prop: emit_member_prop(prop),
        }),
        ir::Expr::Call { callee, args } => Expr::Call(CallExpr {
            span: DUMMY_SP,
            ctxt: Default::default(),
            callee: Callee::Expr(Box::new(emit_expr(callee))),
            args: args.iter().map(emit_call_arg).collect(),
            type_args: None,
        }),
        ir::Expr::New { callee, args } => Expr::New(NewExpr {
            span: DUMMY_SP,
            ctxt: Default::default(),
            callee: Box::new(emit_expr(callee)),
            args: Some(args.iter().map(emit_call_arg).collect()),
            type_args: None,
        }),
        ir::Expr::Closure(func) => Expr::Fn(FnExpr {
            ident: None,
            function: Box::new(emit_function(func)),
        }),
        ir::Expr::TenantYield(inner) => Expr::Yield(YieldExpr {
            span: DUMMY_SP,
            arg: Some(Box::new(Expr::Call(CallExpr {
                span: DUMMY_SP,
                ctxt: Default::default(),
                callee: Callee::Expr(Box::new(Expr::Member(MemberExpr {
                    span: DUMMY_SP,
                    obj: Box::new(Expr::Ident(ident("tenant"))),
                    prop: MemberProp::Ident(IdentName::new("yieldTenant".into(), DUMMY_SP)),
                }))),
                args: vec![ExprOrSpread {
                    spread: None,
                    expr: Box::new(emit_expr(inner)),
                }],
                type_args: None,
            }))),
            delegate: false,
        }),
        ir::Expr::Bin { op, lhs, rhs } => Expr::Bin(BinExpr {
            span: DUMMY_SP,
            op: emit_bin_op(*op),
            left: Box::new(emit_expr(lhs)),
            right: Box::new(emit_expr(rhs)),
        }),
        ir::Expr::Un { op, arg } => Expr::Unary(UnaryExpr {
            span: DUMMY_SP,
            op: emit_un_op(*op),
            arg: Box::new(emit_expr(arg)),
        }),
        ir::Expr::Cond { test, cons, alt } => Expr::Cond(CondExpr {
            span: DUMMY_SP,
            test: Box::new(emit_expr(test)),
            cons: Box::new(emit_expr(cons)),
            alt: Box::new(emit_expr(alt)),
        }),
        ir::Expr::Assign { target, value } => Expr::Assign(AssignExpr {
            span: DUMMY_SP,
            op: AssignOp::Assign,
            left: expr_to_assign_target(target),
            right: Box::new(emit_expr(value)),
        }),
        ir::Expr::Spread(_) => unreachable!(
            "ir::Expr::Spread is never constructed by lower.rs (spreads only arise inside \
             CallArg::Spread/ArrayElement::Spread, which emit_call_arg/the Array arm handle \
             directly) — swc_ecma_ast has no bare-spread Expr variant to build here anyway"
        ),
        ir::Expr::Paren(inner) => Expr::Paren(ParenExpr {
            span: DUMMY_SP,
            expr: Box::new(emit_expr(inner)),
        }),
        ir::Expr::HostIntrinsic { name, args } => emit_host_intrinsic(name, args),
        ir::Expr::Cast { expr, target } => Expr::TsAs(TsAsExpr {
            span: DUMMY_SP,
            expr: Box::new(emit_expr(expr)),
            type_ann: Box::new(emit_type_ref(target)),
        }),
        ir::Expr::NonNull(inner) => Expr::TsNonNull(TsNonNullExpr {
            span: DUMMY_SP,
            expr: Box::new(emit_expr(inner)),
        }),
        ir::Expr::Sequence(exprs) => Expr::Seq(SeqExpr {
            span: DUMMY_SP,
            exprs: exprs.iter().map(|e| Box::new(emit_expr(e))).collect(),
        }),
    }
}

/// `lower_assign` (`lower.rs:1216`) only ever constructs an `Expr::Assign`'s `target` from
/// `ast::SimpleAssignTarget::Ident` or `::Member` — never any other shape — so `Ident`/`Member`
/// are the only reachable cases here; anything else would mean `lower.rs` grew a new assign
/// target shape this emitter needs to be updated for, not a case to guess at silently.
fn expr_to_assign_target(expr: &ir::Expr) -> AssignTarget {
    match emit_expr(expr) {
        Expr::Ident(id) => AssignTarget::Simple(SimpleAssignTarget::Ident(BindingIdent {
            id,
            type_ann: None,
        })),
        Expr::Member(member) => AssignTarget::Simple(SimpleAssignTarget::Member(member)),
        other => unreachable!(
            "lower_assign only ever produces an Ident or Member assign target; got {other:?}"
        ),
    }
}

fn emit_member_prop(prop: &ir::MemberProp) -> MemberProp {
    match prop {
        ir::MemberProp::Ident(name) => MemberProp::Ident(IdentName::new(name.as_str().into(), DUMMY_SP)),
        ir::MemberProp::Computed(e) => MemberProp::Computed(ComputedPropName {
            span: DUMMY_SP,
            expr: Box::new(emit_expr(e)),
        }),
        ir::MemberProp::Private(name) => MemberProp::PrivateName(PrivateName {
            span: DUMMY_SP,
            name: Atom::from(name.as_str()),
        }),
    }
}

fn emit_call_arg(arg: &ir::CallArg) -> ExprOrSpread {
    match arg {
        ir::CallArg::Normal(e) => ExprOrSpread {
            spread: None,
            expr: Box::new(emit_expr(e)),
        },
        ir::CallArg::Spread(e) => ExprOrSpread {
            spread: Some(DUMMY_SP),
            expr: Box::new(emit_expr(e)),
        },
    }
}

fn emit_object_prop(prop: &ir::ObjectProp) -> PropOrSpread {
    match prop {
        ir::ObjectProp::KeyValue { key, value } => PropOrSpread::Prop(Box::new(Prop::KeyValue(KeyValueProp {
            key: emit_prop_name(key),
            value: Box::new(emit_expr(value)),
        }))),
        ir::ObjectProp::Method { key, func } => PropOrSpread::Prop(Box::new(Prop::Method(MethodProp {
            key: emit_prop_name(key),
            function: Box::new(emit_function(func)),
        }))),
        ir::ObjectProp::Spread(e) => PropOrSpread::Spread(SpreadElement {
            dot3_token: DUMMY_SP,
            expr: Box::new(emit_expr(e)),
        }),
    }
}

fn emit_prop_name(key: &ir::PropKey) -> PropName {
    match key {
        ir::PropKey::Ident(name) => PropName::Ident(IdentName::new(name.as_str().into(), DUMMY_SP)),
        ir::PropKey::Computed(e) => PropName::Computed(ComputedPropName {
            span: DUMMY_SP,
            expr: Box::new(emit_expr(e)),
        }),
    }
}

fn emit_lit(lit: &ir::Lit) -> Expr {
    match lit {
        ir::Lit::Str(s) => Expr::Lit(Lit::Str(str_lit(s))),
        ir::Lit::Num(n) => Expr::Lit(Lit::Num(Number {
            span: DUMMY_SP,
            value: *n,
            raw: None,
        })),
        ir::Lit::Bool(b) => Expr::Lit(Lit::Bool(Bool {
            span: DUMMY_SP,
            value: *b,
        })),
        ir::Lit::Null => Expr::Lit(Lit::Null(Null { span: DUMMY_SP })),
        ir::Lit::Undefined => Expr::Ident(ident("undefined")),
    }
}

fn emit_template(parts: &[ir::TemplatePart]) -> Expr {
    let mut quasis = Vec::new();
    let mut exprs = Vec::new();
    let mut pending = String::new();
    let mut tail = true;
    for part in parts {
        match part {
            ir::TemplatePart::Str(s) => {
                pending.push_str(s);
                tail = true;
            }
            ir::TemplatePart::Expr(e) => {
                quasis.push(TplElement {
                    span: DUMMY_SP,
                    tail: false,
                    cooked: Some(Atom::from(pending.as_str()).into()),
                    raw: Atom::from(pending.as_str()),
                });
                pending.clear();
                exprs.push(Box::new(emit_expr(e)));
                tail = false;
            }
        }
    }
    quasis.push(TplElement {
        span: DUMMY_SP,
        tail,
        cooked: Some(Atom::from(pending.as_str()).into()),
        raw: Atom::from(pending.as_str()),
    });
    Expr::Tpl(Tpl {
        span: DUMMY_SP,
        exprs,
        quasis,
    })
}

fn emit_bin_op(op: ir::BinOp) -> BinaryOp {
    match op {
        ir::BinOp::Add => BinaryOp::Add,
        ir::BinOp::Sub => BinaryOp::Sub,
        ir::BinOp::Mul => BinaryOp::Mul,
        ir::BinOp::Div => BinaryOp::Div,
        ir::BinOp::Mod => BinaryOp::Mod,
        ir::BinOp::Eq => BinaryOp::EqEqEq,
        ir::BinOp::NotEq => BinaryOp::NotEqEq,
        ir::BinOp::Lt => BinaryOp::Lt,
        ir::BinOp::Le => BinaryOp::LtEq,
        ir::BinOp::Gt => BinaryOp::Gt,
        ir::BinOp::Ge => BinaryOp::GtEq,
        ir::BinOp::And => BinaryOp::LogicalAnd,
        ir::BinOp::Or => BinaryOp::LogicalOr,
        ir::BinOp::Nullish => BinaryOp::NullishCoalescing,
        ir::BinOp::In => BinaryOp::In,
    }
}

fn emit_un_op(op: ir::UnOp) -> UnaryOp {
    match op {
        ir::UnOp::Not => UnaryOp::Bang,
        ir::UnOp::Neg => UnaryOp::Minus,
        ir::UnOp::TypeOf => UnaryOp::TypeOf,
    }
}

/// Mirrors `emit_ts.rs::emit_host_intrinsic` exactly, one recognized shape at a time.
fn emit_host_intrinsic(name: &str, args: &[ir::Expr]) -> Expr {
    let call = |callee: Expr, args: &[ir::Expr]| {
        Expr::Call(CallExpr {
            span: DUMMY_SP,
            ctxt: Default::default(),
            callee: Callee::Expr(Box::new(callee)),
            args: args
                .iter()
                .map(|e| ExprOrSpread {
                    spread: None,
                    expr: Box::new(emit_expr(e)),
                })
                .collect(),
            type_args: None,
        })
    };
    let member_call = |obj: Expr, method: &str, args: &[ir::Expr]| {
        call(
            Expr::Member(MemberExpr {
                span: DUMMY_SP,
                obj: Box::new(obj),
                prop: MemberProp::Ident(IdentName::new(method.into(), DUMMY_SP)),
            }),
            args,
        )
    };
    match name {
        n if n == crate::intrinsics::IS_ARRAY_INDEX_STRING => member_call(
            Expr::Lit(Lit::Regex(Regex {
                span: DUMMY_SP,
                exp: Atom::from("^(0|[1-9][0-9]*)$"),
                flags: Atom::from(""),
            })),
            "test",
            &args[0..1],
        ),
        n if n == crate::intrinsics::IS_STRING_KEY => Expr::Bin(BinExpr {
            span: DUMMY_SP,
            op: BinaryOp::EqEqEq,
            left: Box::new(Expr::Unary(UnaryExpr {
                span: DUMMY_SP,
                op: UnaryOp::TypeOf,
                arg: Box::new(emit_expr(&args[0])),
            })),
            right: Box::new(Expr::Lit(Lit::Str(str_lit("string")))),
        }),
        n if n == crate::intrinsics::TO_NUMBER => call(Expr::Ident(ident("Number")), &args[0..1]),
        n if n == crate::intrinsics::TO_STRING => call(Expr::Ident(ident("String")), &args[0..1]),
        n if n == crate::intrinsics::MAP_GET => member_call(emit_expr(&args[0]), "get", &args[1..]),
        n if n == crate::intrinsics::MAP_SET => member_call(emit_expr(&args[0]), "set", &args[1..]),
        n if n == crate::intrinsics::MAP_HAS => member_call(emit_expr(&args[0]), "has", &args[1..]),
        n if n == crate::intrinsics::MAP_DELETE => member_call(emit_expr(&args[0]), "delete", &args[1..]),
        n if n == crate::intrinsics::ARRAY_FILTER => member_call(emit_expr(&args[0]), "filter", &args[1..]),
        n if n == crate::intrinsics::ARRAY_SORT_BY => member_call(emit_expr(&args[0]), "sort", &args[1..]),
        n if n == crate::intrinsics::ARRAY_PUSH => member_call(emit_expr(&args[0]), "push", &args[1..]),
        n if n == crate::intrinsics::ARRAY_SLICE_FROM => member_call(emit_expr(&args[0]), "slice", &args[1..]),
        n if n == crate::intrinsics::NUMBER_IS_INTEGER => member_call(
            Expr::Ident(ident("Number")),
            "isInteger",
            &args[0..1],
        ),
        n if n == crate::intrinsics::MATH_MIN => member_call(Expr::Ident(ident("Math")), "min", &args[0..2]),
        n if n == crate::intrinsics::MATH_MAX => member_call(Expr::Ident(ident("Math")), "max", &args[0..2]),
        n if n == crate::intrinsics::MATH_ROUND => {
            member_call(Expr::Ident(ident("Math")), "round", &args[0..1])
        }
        other => Expr::Ident(ident(&format!("__jade_unrecognized_intrinsic_{other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lower::lower_module;

    fn lower(source: &str) -> ir::Module {
        let lowered = lower_module("test.ts", source).expect("test fixture should lower cleanly");
        assert!(
            lowered.skipped.is_empty(),
            "test fixture should fully lower with no skipped items: {:?}",
            lowered.skipped
        );
        lowered.module
    }

    /// Asserts a full, drop-free re-lower (see `lower::LoweredModule`'s doc comment on why a
    /// caller must check this rather than trusting `Result::Ok` alone) and returns the module.
    fn expect_full_relower(label: &str, printed: &str) -> ir::Module {
        let reparsed = lower_module(label, printed)
            .unwrap_or_else(|err| panic!("emit_ast's printed output failed to re-lower: {err}\n---\n{printed}"));
        assert!(
            reparsed.skipped.is_empty(),
            "emit_ast's printed output must fully re-lower with no skipped items: {:?}\n---\n{printed}",
            reparsed.skipped
        );
        reparsed.module
    }

    /// The parity oracle described in `docs/bundle-pass-build-step-plan.md`'s "Migrating
    /// `emit_ts.rs`" section, implemented as an automated round trip rather than a manual `tsc`
    /// invocation: printing `emit_ast`'s output and lowering it again must reproduce the exact
    /// same IR, for real (not synthetic) primordial source. `object.ts` is `include_str!`'d
    /// directly from the canonical file — not copy-pasted — so this test tracks the real file
    /// rather than a fixture that can silently drift from it.
    #[test]
    fn object_ts_round_trips_through_ast_construction() {
        let source = include_str!("../../../packages/jade-js/primordials/object.ts");
        let module = lower(source);
        let printed = print_module(&emit_module(&module));
        let reparsed = expect_full_relower("object.ts (round-tripped)", &printed);
        assert_eq!(
            module, reparsed,
            "emit_ast's printed output must lower back to the same IR\n---\n{printed}"
        );
    }

    /// `array-buffer.ts` is this plan's actual motivating case: `BufferPrimordialImpl` is a
    /// real class with `#private` fields (per `docs/primordial-ir-plan.md`'s class-lowering
    /// work), the same shape the whole bundle-pass build step exists to widen into a public
    /// mangled accessor. If class-shaped IR didn't round-trip through `emit_ast`, none of the
    /// rest of the plan would have anything real to compose with.
    /// `array-buffer.ts`'s `nativeBufferHooks` is a *documented, deliberate* exclusion, not a
    /// gap to fix: it builds directly on real host `ArrayBuffer`/`instanceof`, which has "no
    /// natural Rust translation" and is "excluded from IR translation entirely" per
    /// `docs/primordial-ir-plan.md`'s "Named exclusion, not an oversight" note — a human hand-
    /// writes its Rust equivalent. So this test accepts exactly that one known skip (asserted
    /// by content, not just count) rather than requiring zero skips like the other three fully-
    /// covered files — a *different* unexpected skip must still fail this test.
    fn expect_known_array_buffer_skip(skipped: &[crate::IrError]) {
        assert_eq!(
            skipped.len(),
            1,
            "expected exactly the known nativeBufferHooks (`instanceof`) skip, got: {skipped:?}"
        );
        assert!(
            matches!(
                &skipped[0],
                crate::IrError::Unsupported { construct, .. } if construct.contains("instanceof")
            ),
            "expected the known instanceof-related skip, got: {skipped:?}"
        );
    }

    #[test]
    fn array_buffer_ts_round_trips_through_ast_construction() {
        let source = include_str!("../../../packages/jade-js/primordials/array-buffer.ts");
        let lowered = lower_module("array-buffer.ts", source).expect("array-buffer.ts should lower");
        expect_known_array_buffer_skip(&lowered.skipped);
        let module = lowered.module;
        let printed = print_module(&emit_module(&module));
        // Unlike the original source, the *printed* output never contained `nativeBufferHooks`
        // to begin with (it was dropped from `module` before printing) — so re-lowering it
        // should be fully clean, not hit the same skip a second time.
        let reparsed = expect_full_relower("array-buffer.ts (round-tripped)", &printed);
        assert_eq!(
            module, reparsed,
            "emit_ast's printed output must lower back to the same IR\n---\n{printed}"
        );
    }

    #[test]
    fn function_ts_round_trips_through_ast_construction() {
        let source = include_str!("../../../packages/jade-js/primordials/function.ts");
        let module = lower(source);
        let printed = print_module(&emit_module(&module));
        let reparsed = expect_full_relower("function.ts (round-tripped)", &printed);
        assert_eq!(
            module, reparsed,
            "emit_ast's printed output must lower back to the same IR\n---\n{printed}"
        );
    }

    #[test]
    fn reflect_ts_round_trips_through_ast_construction() {
        let source = include_str!("../../../packages/jade-js/primordials/reflect.ts");
        let module = lower(source);
        let printed = print_module(&emit_module(&module));
        let reparsed = expect_full_relower("reflect.ts (round-tripped)", &printed);
        assert_eq!(
            module, reparsed,
            "emit_ast's printed output must lower back to the same IR\n---\n{printed}"
        );
    }

    #[test]
    fn per_tenant_cache_emits_expected_shape() {
        // lower.rs's PerTenantCache recognition needs a resolvable named value type — a bare
        // `object` keyword type doesn't qualify, only a named interface/struct does (matching
        // the real per-tenant-cache shape every primordial file actually uses, e.g. object.ts's
        // `const cache = new WeakMap<Tenant, ObjectPrimordial>();`).
        let module = lower(
            "import type { Tenant } from \"../tenants/types.ts\";\n\
             export interface X { a: number }\n\
             const cache = new WeakMap<Tenant, X>();\n",
        );
        let printed = print_module(&emit_module(&module));
        assert!(printed.contains("new WeakMap<Tenant, X>()"), "{printed}");
    }

    #[test]
    fn private_class_field_and_generator_method_emit_expected_shape() {
        // `TypeRef::Optional` collapses both `T | null` and `T | undefined` source spellings
        // into one IR shape, and both emitters (this one and emit_ts.rs) always re-render it as
        // `| null` — a pre-existing, deliberate IR property (see `ir::TypeRef::Optional`'s doc
        // comment), not something to preserve verbatim, so the return type below is written
        // (and asserted) as `| null` to match rather than fight it.
        let src = r#"
            class T {
                #shadow: Map<string, number> = new Map();
                *get(key: string): Generator<any, number | null, any> {
                    return this.#shadow.get(key);
                }
            }
        "#;
        let module = lower(src);
        let printed = print_module(&emit_module(&module));
        assert!(printed.contains("#shadow"), "{printed}");
        assert!(printed.contains("Map<string, number>"), "{printed}");
        assert!(printed.contains("*get(key: string)"), "{printed}");
        assert!(
            printed.contains("Generator<any, number | null, any>"),
            "{printed}"
        );
        assert!(printed.contains("this.#shadow.get(key)"), "{printed}");
    }
}
