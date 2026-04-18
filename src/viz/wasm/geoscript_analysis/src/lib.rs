use fxhash::FxHashSet;
use geoscript::{
  builtins::{
    fn_defs::{fn_sigs, FnDef},
    FUNCTION_ALIASES,
  },
  parse_program_maybe_with_prelude, EvalCtx,
};
use nanoserde::SerJson;

mod analysis;
mod completions;
mod diagnostics;
mod format;
mod goto;
mod hover;
mod scope;
mod source_scan;

pub use analysis::Analysis;
pub use scope::{SymbolDef, SymbolKind, SymbolRef};

/// Severity of a diagnostic message.
#[derive(Clone, Debug, PartialEq, Eq, SerJson)]
pub enum DiagnosticSeverity {
  Error,
  Warning,
  Info,
}

/// A diagnostic produced by static analysis of geoscript source code.
#[derive(Clone, Debug, SerJson)]
pub struct AnalysisDiagnostic {
  /// Byte offset of the start of the diagnostic span in the *original user source* (prelude
  /// lines excluded).
  pub start_line: u32,
  pub start_col: u32,
  pub end_line: u32,
  pub end_col: u32,
  pub severity: DiagnosticSeverity,
  pub message: String,
}

/// Information returned for a hover request.
#[derive(Clone, Debug, SerJson)]
pub struct HoverInfo {
  pub content: String,
  pub start_line: u32,
  pub start_col: u32,
  pub end_line: u32,
  pub end_col: u32,
}

/// A single completion suggestion.
#[derive(Clone, Debug, SerJson)]
pub struct CompletionItem {
  pub label: String,
  /// "function", "variable", "keyword"
  pub kind: String,
  /// Short detail text (e.g. type signature)
  pub detail: String,
  /// Longer description
  pub info: String,
}

/// Result of a go-to-definition request.
#[derive(Clone, Debug, SerJson)]
pub struct DefinitionLocation {
  pub start_line: u32,
  pub start_col: u32,
  pub end_line: u32,
  pub end_col: u32,
}

/// Full analysis result for a piece of geoscript source code.
#[derive(Clone, Debug, SerJson)]
pub struct AnalysisResult {
  pub diagnostics: Vec<AnalysisDiagnostic>,
}

/// The main analysis context.  Created once per editor session and reused across analysis
/// requests.  Holds a lightweight `EvalCtx` used only for parsing (symbol interning, source map).
pub struct AnalysisCtx {
  /// Lightweight eval context used only for parsing — no meshes, materials, etc. are ever
  /// created through this.
  pub eval_ctx: EvalCtx,
  /// Cached set of builtin function names (including aliases) for quick lookup.
  builtin_names: FxHashSet<String>,
}

impl Default for AnalysisCtx {
  fn default() -> Self {
    Self::new()
  }
}

impl AnalysisCtx {
  pub fn new() -> Self {
    let mut builtin_names = FxHashSet::default();
    for (name, _def) in fn_sigs().entries() {
      builtin_names.insert(name.to_string());
    }
    for (alias, _target) in FUNCTION_ALIASES.entries() {
      builtin_names.insert(alias.to_string());
    }

    AnalysisCtx {
      eval_ctx: EvalCtx::default(),
      builtin_names,
    }
  }

  /// Returns true if `name` is a builtin function (or alias).
  pub fn is_builtin(&self, name: &str) -> bool {
    self.builtin_names.contains(name)
  }

  /// Look up the `FnDef` for a builtin function name, resolving aliases.
  /// Returns (canonical_name, def).
  pub fn lookup_builtin<'a>(&self, name: &'a str) -> Option<(&'a str, &'static FnDef)> {
    // First try direct lookup
    if let Some(def) = fn_sigs().get(name) {
      return Some((name, def));
    }
    // Then try alias — return the alias name (what the user wrote), not the canonical name
    if let Some(&real_name) = FUNCTION_ALIASES.get(name) {
      if let Some(def) = fn_sigs().get(real_name) {
        return Some((name, def));
      }
    }
    None
  }

  /// Parse source code and run full analysis, returning diagnostics.
  pub fn analyze(&self, src: &str, include_prelude: bool) -> AnalysisResult {
    let parse_result =
      parse_program_maybe_with_prelude(&self.eval_ctx, src.to_owned(), include_prelude);

    match parse_result {
      Err(err) => {
        // Parse error — return it as a single diagnostic.
        // The error location from Pest is (line, col) in the full source including prelude.
        // `ErrorStack::loc` gives us the innermost error location.
        let (line, col) = err.loc.unwrap_or((1, 1));
        AnalysisResult {
          diagnostics: vec![AnalysisDiagnostic {
            start_line: line,
            start_col: col,
            end_line: line,
            end_col: col,
            severity: DiagnosticSeverity::Error,
            message: format!("{err}"),
          }],
        }
      }
      Ok(program) => diagnostics::analyze_program(self, &program),
    }
  }

  /// Get hover information at a given source location.
  pub fn hover(
    &self,
    src: &str,
    target_line: u32,
    target_col: u32,
    include_prelude: bool,
  ) -> Option<HoverInfo> {
    hover::hover(self, src, target_line, target_col, include_prelude)
  }

  /// Get completions at a given source location.
  pub fn completions(
    &self,
    src: &str,
    target_line: u32,
    target_col: u32,
    include_prelude: bool,
  ) -> Vec<CompletionItem> {
    completions::completions(self, src, target_line, target_col, include_prelude)
  }

  /// Get the definition location for the symbol at the given position.
  pub fn goto_definition(
    &self,
    src: &str,
    target_line: u32,
    target_col: u32,
    include_prelude: bool,
  ) -> Option<DefinitionLocation> {
    goto::goto_definition(self, src, target_line, target_col, include_prelude)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn analyze(src: &str) -> AnalysisResult {
    let ctx = AnalysisCtx::new();
    ctx.analyze(src, false)
  }

  #[test]
  fn test_no_errors_simple_assignment() {
    let result = analyze("x = 1\ny = x + 2");
    assert!(
      result.diagnostics.is_empty(),
      "Expected no diagnostics, got: {:?}",
      result.diagnostics
    );
  }

  #[test]
  fn test_undefined_variable() {
    let result = analyze("y = x + 1");
    assert!(
      result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("Undefined variable `x`")),
      "Expected undefined variable diagnostic for `x`, got: {:?}",
      result.diagnostics
    );
  }

  #[test]
  fn test_builtin_call_ok() {
    let result = analyze("m = box()");
    assert!(
      result.diagnostics.is_empty(),
      "Expected no diagnostics for box(), got: {:?}",
      result.diagnostics
    );
  }

  #[test]
  fn test_variable_used_after_definition() {
    let result = analyze(
      r#"
x = 10
y = 20
z = x + y
"#,
    );
    assert!(
      result.diagnostics.is_empty(),
      "Expected no diagnostics, got: {:?}",
      result.diagnostics
    );
  }

  #[test]
  fn test_closure_params_in_scope() {
    let result = analyze(
      r#"
f = |x, y| x + y
"#,
    );
    assert!(
      result.diagnostics.is_empty(),
      "Expected no diagnostics, got: {:?}",
      result.diagnostics
    );
  }

  #[test]
  fn test_shadowed_builtin_not_checked() {
    // If someone shadows a builtin name with a local, we shouldn't
    // check the call against the builtin signature
    let result = analyze(
      r#"
box = |a, b, c| a + b + c
box(1, 2, 3)
"#,
    );
    assert!(
      result.diagnostics.is_empty(),
      "Expected no diagnostics for shadowed builtin, got: {:?}",
      result.diagnostics
    );
  }

  #[test]
  fn test_block_scoping() {
    let result = analyze(
      r#"
x = {
  inner = 10
  inner + 1
}
y = x
"#,
    );
    assert!(
      result.diagnostics.is_empty(),
      "Expected no diagnostics, got: {:?}",
      result.diagnostics
    );
  }

  #[test]
  fn test_hover_builtin() {
    let ctx = AnalysisCtx::new();
    let src = "m = box()";
    let hover = ctx.hover(src, 1, 5, false);
    assert!(hover.is_some(), "Expected hover info for `box`");
    let hover = hover.unwrap();
    assert!(
      hover.content.contains("builtin"),
      "Expected builtin hover, got: {}",
      hover.content
    );
  }

  #[test]
  fn test_goto_definition() {
    let ctx = AnalysisCtx::new();
    let src = "x = 10\ny = x + 1";
    let def = ctx.goto_definition(src, 2, 5, false);
    assert!(def.is_some(), "Expected definition location for `x`");
    let def = def.unwrap();
    assert_eq!(def.start_line, 1, "Expected definition on line 1");
  }

  #[test]
  fn test_partial_application_no_error() {
    // Partial application: fewer args than required is OK (pipeline operator)
    let result = analyze("m = translate(vec3(0, 1, 0))");
    assert!(
      result.diagnostics.is_empty(),
      "Expected no diagnostics for partial application, got: {:?}",
      result.diagnostics
    );
  }

  #[test]
  fn test_too_many_args_still_errors() {
    // Too many positional args should still be reported
    let result = analyze("m = vec3(1, 2, 3, 4, 5, 6, 7)");
    assert!(
      result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("at most")),
      "Expected too-many-args diagnostic, got: {:?}",
      result.diagnostics
    );
  }

  #[test]
  fn test_invalid_kwarg_error() {
    let result = analyze("m = box(nonexistent_kwarg=5)");
    assert!(
      result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("unknown keyword argument")),
      "Expected invalid kwarg diagnostic, got: {:?}",
      result.diagnostics
    );
  }

  #[test]
  fn test_valid_kwarg_no_error() {
    let result = analyze("m = box(size=2)");
    assert!(
      result.diagnostics.is_empty(),
      "Expected no diagnostics for valid kwarg, got: {:?}",
      result.diagnostics
    );
  }

  #[test]
  fn test_hover_kwarg_name() {
    let ctx = AnalysisCtx::new();
    // "m = box(size=2)" — hover on "size" at col 9
    let src = "m = box(size=2)";
    let hover = ctx.hover(src, 1, 9, false);
    assert!(hover.is_some(), "Expected hover info for kwarg `size`");
    let hover = hover.unwrap();
    assert!(
      hover.content.contains("parameter") && hover.content.contains("size"),
      "Expected parameter hover for `size`, got: {}",
      hover.content
    );
  }

  #[test]
  fn test_hover_variable_with_inferred_type() {
    let ctx = AnalysisCtx::new();

    // Hover on `m` definition — should show inferred type from box() → Mesh
    let hover = ctx.hover("m = box()", 1, 1, false).unwrap();
    assert!(
      hover.content.contains("mesh"),
      "Expected type 'mesh' in hover for box() result, got: {}",
      hover.content
    );

    // Hover on literal assignment — should show Int
    let hover = ctx.hover("x = 42", 1, 1, false).unwrap();
    assert!(
      hover.content.contains("int"),
      "Expected type 'int' in hover, got: {}",
      hover.content
    );

    // Hover on reference to a typed variable
    let hover = ctx.hover("m = box()\nn = m", 2, 5, false).unwrap();
    assert!(
      hover.content.contains("mesh"),
      "Expected type 'mesh' in hover for reference to m, got: {}",
      hover.content
    );

    // Variable assigned from chain — type propagates
    let hover = ctx.hover("v = vec3(1, 0, 0)", 1, 1, false).unwrap();
    assert!(
      hover.content.contains("vec3"),
      "Expected type 'vec3' in hover, got: {}",
      hover.content
    );
  }

  #[test]
  fn test_completions_inside_call_include_kwargs() {
    let ctx = AnalysisCtx::new();
    // cursor inside box() call — should get kwarg suggestions
    let src = "m = box()";
    let completions = ctx.completions(src, 1, 9, false);
    let kwarg_labels: Vec<&str> = completions
      .iter()
      .filter(|c| c.kind == "property")
      .map(|c| c.label.as_str())
      .collect();
    assert!(
      kwarg_labels.contains(&"size="),
      "Expected kwarg completion `size=`, got: {:?}",
      kwarg_labels
    );
  }

  #[test]
  fn test_hover_pipeline_propagates_type() {
    let ctx = AnalysisCtx::new();
    // `m = box() | scale(2)` — pipeline should produce Mesh
    let hover = ctx
      .hover("m = box() | scale(2)", 1, 1, false)
      .expect("hover for `m`");
    assert!(
      hover.content.contains("mesh"),
      "expected `mesh` type for piped chain, got: {}",
      hover.content
    );
  }

  #[test]
  fn test_hover_pipeline_chain_propagates_type() {
    let ctx = AnalysisCtx::new();
    // Multi-stage chain — type should still propagate end-to-end
    let hover = ctx
      .hover(
        "m = box() | scale(2) | translate(vec3(1, 0, 0))",
        1,
        1,
        false,
      )
      .expect("hover for `m`");
    assert!(
      hover.content.contains("mesh"),
      "expected `mesh` type for multi-stage piped chain, got: {}",
      hover.content
    );
  }

  #[test]
  fn test_hover_binop_int_addition() {
    let ctx = AnalysisCtx::new();
    let hover = ctx.hover("x = 1 + 2", 1, 1, false).expect("hover for `x`");
    assert!(
      hover.content.contains("int"),
      "expected `int` type for int+int, got: {}",
      hover.content
    );
  }

  #[test]
  fn test_hover_binop_vec3_addition() {
    let ctx = AnalysisCtx::new();
    let hover = ctx
      .hover("v = vec3(1, 0, 0) + vec3(0, 1, 0)", 1, 1, false)
      .expect("hover for `v`");
    assert!(
      hover.content.contains("vec3"),
      "expected `vec3` type for vec3+vec3, got: {}",
      hover.content
    );
  }

  #[test]
  fn test_hover_static_field_access_swizzle() {
    let ctx = AnalysisCtx::new();
    // `v.x` on a vec3 → float
    let hover = ctx
      .hover("v = vec3(1, 0, 0)\nx = v.x", 2, 1, false)
      .expect("hover for `x`");
    assert!(
      hover.content.contains("float"),
      "expected `float` type for v.x, got: {}",
      hover.content
    );

    // `v.xy` → vec2
    let hover = ctx
      .hover("v = vec3(1, 0, 0)\nxy = v.xy", 2, 1, false)
      .expect("hover for `xy`");
    assert!(
      hover.content.contains("vec2"),
      "expected `vec2` type for v.xy, got: {}",
      hover.content
    );
  }

  #[test]
  fn test_type_hint_mismatch_diagnostic() {
    // `x: mesh = "string"` — string can't be assigned to a mesh-typed binding
    let result = analyze(r#"x: mesh = "string""#);
    assert!(
      result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("type mismatch")
          && d.message.contains("mesh")
          && d.message.contains("str")),
      "expected type-mismatch diagnostic, got: {:?}",
      result.diagnostics
    );
  }

  #[test]
  fn test_type_hint_compatible_no_diagnostic() {
    // `x: num = 1` — `num` accepts both int and float, so no error
    let result = analyze("x: num = 1");
    assert!(
      result.diagnostics.is_empty(),
      "expected no diagnostics for `num = 1`, got: {:?}",
      result.diagnostics
    );
  }

  #[test]
  fn test_no_matching_signature_diagnostic() {
    // box() has no signature accepting a string positional arg
    let result = analyze(r#"m = box("not a valid arg")"#);
    assert!(
      result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("no overload") && d.message.contains("box")),
      "expected no-overload diagnostic, got: {:?}",
      result.diagnostics
    );
  }

  #[test]
  fn test_partial_application_does_not_diagnose() {
    // translate(vec3(...)) is a valid partial app — must not emit no-overload
    let result = analyze("m = translate(vec3(1, 0, 0))");
    assert!(
      result
        .diagnostics
        .iter()
        .all(|d| !d.message.contains("no overload")),
      "expected no no-overload diagnostic for partial app, got: {:?}",
      result.diagnostics
    );
  }

  #[test]
  fn test_hover_paf_var_shows_partial_application() {
    let ctx = AnalysisCtx::new();
    // `f = translate(vec3(1, 0, 0))` — translate needs more args, so f is a partial app.
    // Hover on `f` should describe the partial application and remaining params.
    let hover = ctx
      .hover("f = translate(vec3(1, 0, 0))", 1, 1, false)
      .expect("hover for `f`");
    assert!(
      hover.content.contains("partial application"),
      "expected partial-application label in hover, got: {}",
      hover.content
    );
    assert!(
      hover.content.contains("translate"),
      "expected base function name `translate` in hover, got: {}",
      hover.content
    );
    // The first overload of translate has `mesh` as the remaining param after binding the vec3.
    assert!(
      hover.content.contains("mesh"),
      "expected remaining param info to mention `mesh`, got: {}",
      hover.content
    );
  }

  #[test]
  fn test_paf_call_resolves_to_return_type() {
    let ctx = AnalysisCtx::new();
    // f = translate(vec3); m = f(box())  — calling the PAF should yield Mesh
    let hover = ctx
      .hover("f = translate(vec3(1, 0, 0))\nm = f(box())", 2, 1, false)
      .expect("hover for `m`");
    assert!(
      hover.content.contains("mesh"),
      "expected `mesh` type for f(box()) result, got: {}",
      hover.content
    );
  }

  #[test]
  fn test_paf_pipeline_resolves_to_return_type() {
    let ctx = AnalysisCtx::new();
    // f = translate(vec3); m = box() | f  — piping into the PAF should yield Mesh
    let hover = ctx
      .hover("f = translate(vec3(1, 0, 0))\nm = box() | f", 2, 1, false)
      .expect("hover for `m`");
    assert!(
      hover.content.contains("mesh"),
      "expected `mesh` type for `box() | f` result, got: {}",
      hover.content
    );
  }

  #[test]
  fn test_paf_pipeline_wrong_type_diagnoses() {
    // `1 | translate(vec3)` — combined args become [vec3, int], no overload matches and
    // the prefix isn't valid either (sig 0 wants Mesh|Light second; sig 1 wants Numeric first).
    let result = analyze("x = 1 | translate(vec3(1, 0, 0))");
    assert!(
      result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("no overload") && d.message.contains("translate")),
      "expected no-overload diagnostic when wrong type is piped into PAF, got: {:?}",
      result.diagnostics
    );
  }

  #[test]
  fn test_paf_chained_pipeline_through_var() {
    let ctx = AnalysisCtx::new();
    // Chain: bind a partial, then pipeline through it from a `box()` result.
    let hover = ctx
      .hover(
        "shift = translate(vec3(1, 0, 0))\nm = box() | shift | scale(2)",
        2,
        1,
        false,
      )
      .expect("hover for `m`");
    assert!(
      hover.content.contains("mesh"),
      "expected `mesh` type after chained PAF pipeline, got: {}",
      hover.content
    );
  }

  #[test]
  fn test_hover_call_focused_overload_shows_arg_docs() {
    let ctx = AnalysisCtx::new();
    // Hover on `box` in `box(1, 1, 1)` — args fully specified, exactly one signature matches.
    // Output should include the per-argument descriptions ("Width along the X axis" etc.) and
    // should NOT include an "Overload N:" header since only the matched sig is rendered.
    let hover = ctx
      .hover("m = box(1, 1, 1)", 1, 5, false)
      .expect("hover for `box` call");
    assert!(
      hover.content.contains("Width along the X axis"),
      "expected per-arg description for `width`, got: {}",
      hover.content
    );
    assert!(
      hover.content.contains("Arguments:"),
      "expected `Arguments:` header in focused hover, got: {}",
      hover.content
    );
  }

  #[test]
  fn test_hover_call_unknown_args_shows_all_overloads() {
    let ctx = AnalysisCtx::new();
    // No matched sig (undefined ident → Unknown arg type) — fall back to the all-overloads view.
    let hover = ctx
      .hover("m = box(unknown_var)", 1, 5, false)
      .expect("hover for `box` call");
    // The fallback all-overloads view does NOT include the "Arguments:" header that the
    // focused renderer produces.
    assert!(
      !hover.content.contains("Arguments:"),
      "expected fallback (all-overloads) hover when sig unknown, got: {}",
      hover.content
    );
  }

  #[test]
  fn test_closure_return_type_mismatch_diagnoses() {
    // The user's canonical Layer 3 example: an explicit `return` with the wrong type inside
    // a conditional inside a typed closure should surface a return-type-mismatch diagnostic.
    let src = r#"
my_fn = |x: int|: int {
  if x > 1 { return "a string" }
  x + 3
}
"#;
    let result = analyze(src);
    assert!(
      result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("return type mismatch")
          && d.message.contains("int")
          && d.message.contains("str")),
      "expected return-type-mismatch diagnostic, got: {:?}",
      result.diagnostics
    );
  }

  #[test]
  fn test_closure_implicit_return_matches_annotation() {
    // Typed closure whose implicit (tail) expression matches the declared return type —
    // should not produce any diagnostics.
    let result = analyze("f = |x: int|: int { x + 1 }");
    assert!(
      result.diagnostics.is_empty(),
      "expected no diagnostics for well-typed closure, got: {:?}",
      result.diagnostics
    );
  }

  #[test]
  fn test_closure_implicit_return_wrong_type_diagnoses() {
    // Annotated as int but tail expression is a string — should flag the implicit return.
    let result = analyze(r#"f = |x: int|: int { "hello" }"#);
    assert!(
      result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("return type mismatch")),
      "expected implicit-return mismatch diagnostic, got: {:?}",
      result.diagnostics
    );
  }

  #[test]
  fn test_call_typed_closure_resolves_to_return_type() {
    let ctx = AnalysisCtx::new();
    // Calling a user closure should produce hover info showing its return type.
    let hover = ctx
      .hover("f = |x: int|: int { x + 1 }\nm = f(3)", 2, 1, false)
      .expect("hover for `m`");
    assert!(
      hover.content.contains("int"),
      "expected `int` type for f(3) result, got: {}",
      hover.content
    );
  }

  #[test]
  fn test_pipeline_typed_closure_resolves_to_return_type() {
    let ctx = AnalysisCtx::new();
    let hover = ctx
      .hover("f = |x: int|: int { x + 1 }\nm = 3 | f", 2, 1, false)
      .expect("hover for `m`");
    assert!(
      hover.content.contains("int"),
      "expected `int` type for `3 | f` result, got: {}",
      hover.content
    );
  }

  #[test]
  fn test_hover_closure_var_shows_callable_signature() {
    let ctx = AnalysisCtx::new();
    let hover = ctx
      .hover("f = |x: int|: int { x + 1 }", 1, 1, false)
      .expect("hover for `f`");
    assert!(
      hover.content.contains("fn(") && hover.content.contains("→"),
      "expected callable signature in hover, got: {}",
      hover.content
    );
    assert!(
      hover.content.contains("int"),
      "expected `int` in callable signature, got: {}",
      hover.content
    );
  }

  #[test]
  fn test_closure_inferred_return_type_from_body() {
    let ctx = AnalysisCtx::new();
    // No explicit return type annotation — inferred from the body's tail expression.
    let hover = ctx
      .hover("f = |x: int| x + 1\nm = f(3)", 2, 1, false)
      .expect("hover for `m`");
    assert!(
      hover.content.contains("int"),
      "expected inferred `int` return type, got: {}",
      hover.content
    );
  }

  #[test]
  fn test_block_type_is_last_expression() {
    let ctx = AnalysisCtx::new();
    // A block's type is the type of its final expression statement.
    let hover = ctx
      .hover("x = { a = 10\n a + 1 }", 1, 1, false)
      .expect("hover for `x`");
    assert!(
      hover.content.contains("int"),
      "expected `int` type for block result, got: {}",
      hover.content
    );
  }

  #[test]
  fn test_conditional_type_unions_branches() {
    let ctx = AnalysisCtx::new();
    // if-else branches with different types → Union
    let hover = ctx
      .hover(r#"x = if true { 1 } else { "s" }"#, 1, 1, false)
      .expect("hover for `x`");
    // display_str for union joins variant names with ` | `
    assert!(
      hover.content.contains("int") && hover.content.contains("str"),
      "expected union of `int` and `str` from conditional, got: {}",
      hover.content
    );
  }

  #[test]
  fn test_return_outside_closure_is_noop() {
    // A stray `return` at the top level is a runtime concept; the analyzer should not crash
    // or produce a spurious type error on it.
    let result = analyze("return 1");
    assert!(
      result
        .diagnostics
        .iter()
        .all(|d| !d.message.contains("return type mismatch")),
      "unexpected return-type diagnostic for top-level return, got: {:?}",
      result.diagnostics
    );
  }

  #[test]
  fn test_hover_prefix_neg() {
    let ctx = AnalysisCtx::new();
    // `-x` where x is int → int
    let hover = ctx
      .hover("x = 5\ny = -x", 2, 1, false)
      .expect("hover for `y`");
    assert!(
      hover.content.contains("int"),
      "expected `int` type for -x, got: {}",
      hover.content
    );
  }
}
