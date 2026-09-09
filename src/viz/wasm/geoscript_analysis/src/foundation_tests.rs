use crate::{
  correctness_tests::{core_type, definition_type, runtime_type},
  Analysis, AnalysisCtx,
};
use geoscript::{
  ast::TopLevelStatement,
  call_infer::{builtin_type, resolve_builtin_call, resolve_call},
  ty::AbstractType,
  type_infer::{infer_statement, TypeEnv},
  ArgType, EvalCtx,
};

#[test]
fn real_dynamic_profile_infers_every_intermediate_path() {
  let src = include_str!("../test_data/dynamic_profile.geo");
  let ctx = AnalysisCtx::new();
  let diagnostics = ctx.analyze(src, false, "").diagnostics;
  assert!(diagnostics.is_empty(), "{diagnostics:?}");
  let program = geoscript::parse_program_src(&ctx.eval_ctx, src).unwrap();
  let analysis = Analysis::build(&ctx.eval_ctx, &program);
  let mut count = 0;
  for def in analysis.all_defs() {
    if def.kind == crate::SymbolKind::Variable
      && ctx
        .eval_ctx
        .interned_symbols
        .with_resolved(def.name, |name| matches!(name, "a" | "b" | "p"))
        == Some(true)
    {
      assert!(
        matches!(def.ty, AbstractType::Concrete(ArgType::Path)),
        "{def:?}"
      );
      let (line, col) = ctx.eval_ctx.resolve_loc(def.loc);
      assert!(ctx
        .hover(src, line, col, false, "")
        .unwrap()
        .content
        .ends_with(": path"));
      count += 1;
    }
  }
  assert_eq!(count, 4);
}

#[test]
fn closure_defaults_and_partial_signature_help_use_binding_rules() {
  let ctx = AnalysisCtx::new();
  let src = r#"a = 10
f = |a: int, b: int=(a+1)| b
result = f(2)"#;
  let col = src.lines().nth(1).unwrap().find("(a+1)").unwrap() as u32 + 2;
  let definition = ctx.goto_definition(src, 2, col, false, "").unwrap();
  assert_eq!((definition.start_line, definition.start_col), (1, 1));

  let src = r#"f = |a: int, b: float| a
p = f(1)
result = p(2.0)"#;
  let help = ctx.signature_help(src, 3, 13, false, "").unwrap();
  assert_eq!(help.docs.signatures[0].params.len(), 1);
  assert_eq!(help.docs.signatures[0].params[0].name, "b");
  let hover = ctx.hover(src, 2, 1, false, "").unwrap();
  assert!(hover.content.contains("fn(b: float)"), "{hover:?}");
}

fn assert_result(src: &str, expected: &str) {
  let ctx = AnalysisCtx::new();
  let diagnostics = ctx.analyze(src, false, "").diagnostics;
  assert!(diagnostics.is_empty(), "{src}: {diagnostics:?}");
  assert_eq!(
    definition_type(&ctx, src, "result")
      .display_str()
      .as_deref(),
    Some(expected),
    "{src}"
  );
  assert_eq!(
    core_type(src).display_str().as_deref(),
    Some(expected),
    "{src}"
  );
  assert_eq!(runtime_type(src, "result").as_str(), expected, "{src}");
}

#[test]
fn closure_partials_defaults_keywords_and_callbacks() {
  for src in [
    r#"f = |a: int, b: float| a
p = f(1)
result = 2.0 | p"#,
    r#"f = |a: int, b: float| a
p = f(b=2.0)
result = p(1)"#,
    r#"a = 10
f = |a: int, b: int=(a+1)| b
result = f(2)"#,
    r#"f = |a: int, b: float, c: int| a+c
p = f(b=2.0)
q = p(1)
result = q(3)"#,
  ] {
    assert_result(src, "int");
  }
  let src = r#"f = |a: int, b: float| a
result = f(1)"#;
  let ctx = AnalysisCtx::new();
  assert!(matches!(
    definition_type(&ctx, src, "result"),
    AbstractType::PartiallyApplied(_)
  ));
  assert!(matches!(core_type(src), AbstractType::PartiallyApplied(_)));
  assert!(matches!(runtime_type(src, "result"), ArgType::Callable));
  assert_result(
    r#"f = |a: int, b: int| a+b
p = f(1)
result = [1, 2, 3] -> p | collect"#,
    "seq",
  );
}

#[test]
fn partial_keyword_replacement_rebinds_arguments() {
  assert_result(
    r#"p = sub(a=v3(1))
result = p(2, a=4)"#,
    "int",
  );
  assert_result(
    r#"p = vec3(x=1.0)
result = p(x=texture(1, 1, || 1.0), y=2, z=3)"#,
    "texture",
  );
  assert_result(
    r#"f = |a: int, b: float| a
p = f(a=1)
result = p(a=2, b=3.0)"#,
    "int",
  );
}

#[test]
fn pipeline_invokes_the_rhs_value_for_closures_and_factories() {
  assert_result(r#"result = 1 | (|x: int| x+1)"#, "int");
  assert_result(
    r#"factory = |x: int| |y: int| x+y
result = 3 | factory(2)"#,
    "int",
  );
  assert_result(
    r#"f = |x: int, y: int| x+y
result = 3 | f(2)"#,
    "int",
  );
  let src = r#"f = |x: int| v3(x)
result = 1 | f(2)"#;
  let ctx = AnalysisCtx::new();
  assert!(matches!(
    definition_type(&ctx, src, "result"),
    AbstractType::Unknown
  ));
  assert!(matches!(core_type(src), AbstractType::Unknown));
  assert!(ctx
    .analyze(src, false, "")
    .diagnostics
    .iter()
    .any(|d| d.message.contains("bit_or")));
}

#[test]
fn builtin_values_keep_identity_through_aliases_and_shadowing() {
  assert_result(
    r#"f = sin
result = f(1.0)"#,
    "float",
  );
  assert_result(
    r#"f = @v3
result = 3 | f(1, 2)"#,
    "vec3",
  );
  assert_result(
    r#"f = sin
sin = |x: float| v3(x)
result = f(1.0)"#,
    "float",
  );
  assert_result(
    r#"f = |x: int| x
g = f
result = g(1)"#,
    "int",
  );
}

#[test]
fn closure_and_path_argument_errors_are_definite() {
  for (src, message) in [
    (
      r#"f = |x: int| x
result = f("bad")"#,
      "type mismatch",
    ),
    (
      r#"f = |x: int| x
result = f()"#,
      "missing required",
    ),
    (
      r#"p = path()
result = p(1, 2)"#,
      "exactly one numeric",
    ),
    (
      r#"f = 1
result = f(2)"#,
      "not callable",
    ),
  ] {
    let ctx = AnalysisCtx::new();
    assert!(matches!(
      definition_type(&ctx, src, "result"),
      AbstractType::Unknown
    ));
    assert!(
      ctx
        .analyze(src, false, "")
        .diagnostics
        .iter()
        .any(|d| d.message.contains(message)),
      "{src}"
    );
  }
}

#[test]
fn return_inference_does_not_require_an_overload_proof() {
  let ctx = AnalysisCtx::new();
  let src = r#"contours = [path(), path(), path()]
profile = |u: float| {
  a = lerp_paths(contours[0], contours[1], u)
  b = lerp_paths(contours[1], contours[2], u)
  p = lerp_paths(a, b, u)
  p = path_union(p, p)
  p
}
result = profile"#;
  let program = geoscript::parse_program_src(&ctx.eval_ctx, src).unwrap();
  let analysis = Analysis::build(&ctx.eval_ctx, &program);
  for name in ["a", "b", "p"] {
    for def in analysis.all_defs().iter().filter(|def| {
      ctx
        .eval_ctx
        .interned_symbols
        .with_resolved(def.name, |s| s == name)
        == Some(true)
    }) {
      assert!(
        matches!(def.ty, AbstractType::Concrete(ArgType::Path)),
        "{name}: {:?}",
        def.ty
      );
    }
  }
  let first_lerp = analysis
    .function_calls
    .iter()
    .find(|call| ctx.eval_ctx.resolve_loc(call.loc).0 == 3)
    .unwrap();
  assert_eq!(first_lerp.matched_sig_ix, None);
  let AbstractType::Callable(callable) = core_type(src) else {
    panic!("expected callable");
  };
  assert!(matches!(
    *callable.return_type,
    AbstractType::Concrete(ArgType::Path)
  ));
  let args = [
    AbstractType::Unknown,
    AbstractType::Unknown,
    AbstractType::Concrete(ArgType::Float),
  ];
  let result = resolve_builtin_call(
    &ctx.eval_ctx,
    ctx.eval_ctx.interned_symbols.intern("lerp_paths"),
    &args,
    &[],
  );
  assert!(matches!(
    result.return_ty,
    AbstractType::Concrete(ArgType::Path)
  ));
  assert_eq!(result.matched_signature, None);
  assert!(result.error.is_none());
  let invalid = resolve_builtin_call(
    &ctx.eval_ctx,
    ctx.eval_ctx.interned_symbols.intern("lerp_paths"),
    &[
      AbstractType::Unknown,
      AbstractType::Unknown,
      AbstractType::Concrete(ArgType::String),
    ],
    &[],
  );
  assert!(invalid.error.is_some());
}

#[test]
fn uncertain_complete_and_partial_outcomes_stay_distinct() {
  let ctx = EvalCtx::default();
  let target = builtin_type(&ctx, ctx.interned_symbols.intern("v3")).unwrap();
  let result = resolve_call(
    &ctx,
    &target,
    &[AbstractType::Union(vec![ArgType::Int, ArgType::Vec2])],
    &[],
  );
  assert_eq!(result.matched_signature, None);
  assert_eq!(
    result.return_ty.possible_type_flags(),
    ArgType::Vec3.as_bitflags() | ArgType::Callable.as_bitflags()
  );
  let partial = resolve_call(
    &ctx,
    &target,
    &[
      AbstractType::Concrete(ArgType::Float),
      AbstractType::Unknown,
    ],
    &[],
  );
  assert!(partial.return_ty.possible_type_flags() & ArgType::Callable.as_bitflags() != 0);
}

#[test]
fn known_results_flow_through_dynamic_builtin_calls() {
  let ctx = EvalCtx::default();
  let src = r#"make_path = |items: seq, t: float| {
  a = lerp_paths(items[0], items[1], t)
  b = lerp_paths(items[1], items[0], t)
  path_union(a, b)
}"#;
  let mut program = geoscript::parse_program_src(&ctx, src).unwrap();
  let mut env = TypeEnv::new(&ctx);
  for statement in &program.statements {
    if let TopLevelStatement::Statement(statement) = statement {
      infer_statement(&ctx, &mut env, statement);
    }
  }
  let AbstractType::Callable(callable) = env
    .lookup(ctx.interned_symbols.intern("make_path"))
    .unwrap()
  else {
    panic!("expected callable");
  };
  assert!(matches!(
    *callable.return_type,
    AbstractType::Concrete(ArgType::Path)
  ));
  // Optimizing the still-dynamic closure must also accept the inferred path intermediates.
  geoscript::optimizer::optimize_ast(&ctx, &mut program).unwrap();
}
