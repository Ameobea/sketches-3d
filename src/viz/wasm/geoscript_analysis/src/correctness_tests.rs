use geoscript::{
  ast::{Statement, TopLevelStatement},
  ty::AbstractType,
  type_infer::{infer_expr, infer_statement, TypeEnv},
  ArgType, EvalCtx,
};

use crate::{Analysis, AnalysisCtx};

#[test]
fn complete_rhs_calls_are_not_reapplied_by_core_inference() {
  let src = r#"result = 1 | vec3(2)"#;
  let ctx = AnalysisCtx::new();
  assert!(matches!(core_type(src), AbstractType::Unknown));
  assert!(matches!(
    definition_type(&ctx, src, "result"),
    AbstractType::Unknown
  ));
  assert!(ctx
    .analyze(src, false, "")
    .diagnostics
    .iter()
    .any(|d| d.message.contains("bit_or")));

  // Unknown coordinates may make translate complete (two vec2/path operands) or partial.
  // Neither interpretation is proven, so neither consumer may claim a result type.
  let src = r#"result = |a, b| path() | translate(a, b)"#;
  for ty in [core_type(src), definition_type(&ctx, src, "result")] {
    let AbstractType::Callable(callable) = ty else {
      panic!("expected callable");
    };
    assert!(
      matches!(*callable.return_type, AbstractType::Unknown),
      "{callable:?}"
    );
  }
}

#[test]
fn named_paths_and_destructured_parameter_annotations() {
  let src = r#"p = path() | move(0, 0) | line(1, 0)
result = p(0.5)"#;
  assert!(matches!(runtime_type(src, "result"), ArgType::Vec2));
  assert!(matches!(
    core_type(src),
    AbstractType::Concrete(ArgType::Vec2)
  ));
  assert!(matches!(
    definition_type(&AnalysisCtx::new(), src, "result"),
    AbstractType::Concrete(ArgType::Vec2)
  ));

  let src = r#"result = |[x, y]: seq| x"#;
  for ty in [
    core_type(src),
    definition_type(&AnalysisCtx::new(), src, "result"),
  ] {
    let AbstractType::Callable(callable) = ty else {
      panic!("expected callable");
    };
    assert!(matches!(
      callable.params[0].ty,
      AbstractType::Concrete(ArgType::Sequence)
    ));
    assert!(matches!(*callable.return_type, AbstractType::Unknown));
  }
}

#[test]
fn recursive_calls_and_rebinding_keep_definition_identity() {
  let ctx = AnalysisCtx::new();
  let src = r#"f = |n: int| if n > 0 { f(n - 1) } else { 0 }
x = 1
x = x + 1
y = x
"#;
  let recursive_col = src.lines().next().unwrap().find("f(n").unwrap() as u32 + 1;
  let def = ctx
    .goto_definition(src, 1, recursive_col, false, "")
    .unwrap();
  assert_eq!((def.start_line, def.start_col), (1, 1));
  for (line, col, expected_line) in [(3, 5, 2), (4, 5, 3)] {
    let def = ctx.goto_definition(src, line, col, false, "").unwrap();
    assert_eq!((def.start_line, def.start_col), (expected_line, 1));
  }
}

pub(super) fn definition_type(ctx: &AnalysisCtx, src: &str, name: &str) -> AbstractType {
  let program = geoscript::parse_program_src(&ctx.eval_ctx, src).unwrap();
  let analysis = Analysis::build(&ctx.eval_ctx, &program);
  let def = analysis
    .all_defs()
    .iter()
    .rev()
    .find(|def| {
      ctx
        .eval_ctx
        .interned_symbols
        .with_resolved(def.name, |s| s == name)
        == Some(true)
    })
    .unwrap();
  def.ty.clone()
}

pub(super) fn core_type(src: &str) -> AbstractType {
  let ctx = EvalCtx::default();
  let program = geoscript::parse_program_src(&ctx, src).unwrap();
  let mut env = TypeEnv::new(&ctx);
  let mut ty = AbstractType::Unknown;
  for statement in &program.statements {
    if let TopLevelStatement::Statement(statement) = statement {
      if let Statement::Assignment { expr, .. } = statement {
        ty = infer_expr(&ctx, &mut env, expr);
      }
      infer_statement(&ctx, &mut env, statement);
    }
  }
  ty
}

pub(super) fn runtime_type(src: &str, name: &str) -> ArgType {
  let ctx = EvalCtx::default();
  let mut program = geoscript::parse_program_src(&ctx, &format!("{src}\n{name}")).unwrap();
  geoscript::optimizer::optimize_ast(&ctx, &mut program).unwrap();
  geoscript::eval_program_into_scope(&ctx, &program, &ctx.globals)
    .unwrap()
    .get_type()
}

#[test]
fn subdivision_return_types_agree_with_execution() {
  for src in [
    r#"result = box(1) | subdivide_by_plane(v3(0, 1, 0), 0)"#,
    r#"result = box(1) | subdivide_by_plane([v3(0, 1, 0)], [0])"#,
  ] {
    assert!(matches!(runtime_type(src, "result"), ArgType::Mesh));
    assert!(matches!(
      core_type(src),
      AbstractType::Concrete(ArgType::Mesh)
    ));
    assert!(matches!(
      definition_type(&AnalysisCtx::new(), src, "result"),
      AbstractType::Concrete(ArgType::Mesh)
    ));
  }
}

#[test]
fn pipeline_overload_consumption_and_global_sigil() {
  for (src, expected) in [
    (r#"result = 3.0 | vec3(1, 2)"#, "vec3"),
    (r#"result = 3.0 | @v3(1, 2)"#, "vec3"),
    (r#"result = box(1) | @translate(1, 2, 3)"#, "mesh"),
    (r#"result = 4.0 | vec3(x=1, 2)"#, "vec3"),
  ] {
    let ctx = AnalysisCtx::new();
    assert!(
      ctx.analyze(src, false, "").diagnostics.is_empty(),
      "{src}: {:?}",
      ctx.analyze(src, false, "").diagnostics
    );
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
}

#[test]
fn references_follow_active_bindings_and_local_call_targets() {
  let ctx = AnalysisCtx::new();
  let src = r#"x = 1
y = {
  x = "inner"
  x
}
z = x
f = |v: int| v
a = f(1)
b = 1 | f(2)
"#;
  let outer = ctx.goto_definition(src, 6, 5, false, "").unwrap();
  assert_eq!((outer.start_line, outer.start_col), (1, 1));
  assert_eq!(
    ctx.hover(src, 6, 5, false, "").unwrap().content,
    "(variable) x: int"
  );
  let inner = ctx.goto_definition(src, 4, 3, false, "").unwrap();
  assert_eq!((inner.start_line, inner.start_col), (3, 3));
  for (line, col) in [(8, 5), (9, 9)] {
    let def = ctx.goto_definition(src, line, col, false, "").unwrap();
    assert_eq!((def.start_line, def.start_col), (7, 1));
    assert!(ctx
      .hover(src, line, col, false, "")
      .unwrap()
      .content
      .contains("fn(v: int)"));
  }
}

#[test]
fn pattern_bindings_keep_individual_locations_and_types() {
  let ctx = AnalysisCtx::new();
  let src = r#"f = |x: int, y: float| {
  a = x
  b = y
  a + b
}
{ outer: { inner }, renamed: value } = { outer: { inner: true }, renamed: 1 }
copy = inner
other = value
"#;
  for (line, col, definition_col, expected) in [(2, 7, 6, "int"), (3, 7, 14, "float")] {
    let def = ctx.goto_definition(src, line, col, false, "").unwrap();
    assert_eq!((def.start_line, def.start_col), (1, definition_col));
    assert!(ctx
      .hover(src, line, col, false, "")
      .unwrap()
      .content
      .ends_with(expected));
    assert!(ctx
      .hover(src, 1, definition_col, false, "")
      .unwrap()
      .content
      .ends_with(expected));
  }
  for (line, name) in [(7, "inner"), (8, "value")] {
    let col = src
      .lines()
      .nth(line as usize - 1)
      .unwrap()
      .find(name)
      .unwrap() as u32
      + 1;
    let def = ctx.goto_definition(src, line, col, false, "").unwrap();
    let definition_line = src.lines().nth(5).unwrap();
    assert_eq!(
      (def.start_line, def.start_col),
      (6, definition_line.find(name).unwrap() as u32 + 1)
    );
  }
}

#[test]
fn reachable_unknown_return_is_not_narrowed_by_known_exit() {
  for src in [
    r#"f = |g, c| { if c { return 1 }
g(0) }"#,
    r#"f = |g, c| { if c { return g(0) }
return 1 }"#,
  ] {
    let AbstractType::Callable(callable) = definition_type(&AnalysisCtx::new(), src, "f") else {
      panic!("expected callable");
    };
    assert!(
      matches!(*callable.return_type, AbstractType::Unknown),
      "{src}: {callable:?}"
    );
  }
  let src = r#"f = |c| { if c { return 1 }
return 2.0 }"#;
  let AbstractType::Callable(callable) = definition_type(&AnalysisCtx::new(), src, "f") else {
    panic!("expected callable");
  };
  assert_eq!(
    callable.return_type.display_str().as_deref(),
    Some("int | float")
  );
}
