use fxhash::FxHashSet;
use geoscript::{
  builtins::{
    fn_defs::{fn_sigs, FnDef},
    FUNCTION_ALIASES,
  },
  EvalCtx, Program,
};
use nanoserde::SerJson;

mod scope;
mod source_scan;

pub use scope::{ScopeAnalysis, SymbolDef, SymbolKind, SymbolRef};

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
    let parse_result = geoscript::parse_program_maybe_with_prelude(
      &self.eval_ctx,
      src.to_owned(),
      include_prelude,
    );

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
      Ok(program) => self.analyze_program(&program),
    }
  }

  /// Run analysis on an already-parsed program.
  fn analyze_program(&self, program: &Program) -> AnalysisResult {
    let scope_analysis = ScopeAnalysis::build(&self.eval_ctx, program);
    let mut diagnostics = Vec::new();

    // Report undefined references
    for unresolved in &scope_analysis.unresolved_refs {
      let name = self
        .eval_ctx
        .interned_symbols
        .with_resolved(unresolved.name, |s| s.to_string());
      // Skip if name couldn't be resolved (shouldn't happen, but be safe)
      let Some(name) = name else { continue };

      let (line, col) = self.eval_ctx.resolve_loc(unresolved.loc);
      if line == 0 && col == 0 {
        continue;
      }

      diagnostics.push(AnalysisDiagnostic {
        start_line: line,
        start_col: col,
        end_line: line,
        end_col: col + name.len() as u32,
        severity: DiagnosticSeverity::Error,
        message: format!("Undefined variable `{name}`"),
      });
    }

    // Report wrong argument count for builtin function calls
    for call_info in &scope_analysis.function_calls {
      let Some(name_str) = self
        .eval_ctx
        .interned_symbols
        .with_resolved(call_info.name, |s| s.to_string())
      else {
        continue;
      };

      // Only check builtins that aren't shadowed by a local
      if call_info.is_shadowed {
        continue;
      }

      let Some((_real_name, fn_def)) = self.lookup_builtin(&name_str) else {
        continue;
      };

      self.check_call_args(call_info, fn_def, &name_str, &mut diagnostics);
    }

    AnalysisResult { diagnostics }
  }

  fn check_call_args(
    &self,
    call_info: &scope::FunctionCallInfo,
    fn_def: &FnDef,
    name: &str,
    diagnostics: &mut Vec<AnalysisDiagnostic>,
  ) {
    if fn_def.signatures.is_empty() {
      return;
    }

    let n_positional = call_info.arg_count;

    // Only check that there aren't too many positional args.
    // Fewer args than required is allowed because geoscript supports partial application
    // (e.g. `box(1) | translate(vec3(0,1,0))` via the pipeline operator).
    let too_many_positional = !fn_def.signatures.iter().any(|sig| {
      n_positional <= sig.arg_defs.len()
    });

    if too_many_positional {
      let (line, col) = self.eval_ctx.resolve_loc(call_info.loc);
      if line == 0 && col == 0 {
        return;
      }

      let max_params = fn_def
        .signatures
        .iter()
        .map(|sig| sig.arg_defs.len())
        .max()
        .unwrap();

      diagnostics.push(AnalysisDiagnostic {
        start_line: line,
        start_col: col,
        end_line: line,
        end_col: col + name.len() as u32,
        severity: DiagnosticSeverity::Error,
        message: format!(
          "`{name}` accepts at most {max_params} argument(s), but {n_positional} positional argument(s) were provided",
        ),
      });
    }

    // Validate kwargs: check that each kwarg name is valid for at least one signature
    self.check_call_kwargs(call_info, fn_def, name, diagnostics);
  }

  fn check_call_kwargs(
    &self,
    call_info: &scope::FunctionCallInfo,
    fn_def: &FnDef,
    name: &str,
    diagnostics: &mut Vec<AnalysisDiagnostic>,
  ) {
    for &kwarg_sym in &call_info.kwarg_names {
      let kwarg_valid = fn_def.signatures.iter().any(|sig| {
        sig.arg_defs.iter().any(|arg| arg.interned_name == kwarg_sym)
      });

      if !kwarg_valid {
        let (line, col) = self.eval_ctx.resolve_loc(call_info.loc);
        if line == 0 && col == 0 {
          continue;
        }

        let kwarg_name = self
          .eval_ctx
          .interned_symbols
          .with_resolved(kwarg_sym, |s| s.to_string());
        let Some(kwarg_name) = kwarg_name else { continue };

        diagnostics.push(AnalysisDiagnostic {
          start_line: line,
          start_col: col,
          end_line: line,
          end_col: col + name.len() as u32,
          severity: DiagnosticSeverity::Error,
          message: format!(
            "unknown keyword argument `{kwarg_name}` for `{name}`",
          ),
        });
      }
    }
  }

  /// Get hover information at a given source location.
  pub fn hover(&self, src: &str, target_line: u32, target_col: u32, include_prelude: bool) -> Option<HoverInfo> {
    let parse_result = geoscript::parse_program_maybe_with_prelude(
      &self.eval_ctx,
      src.to_owned(),
      include_prelude,
    );

    let program = parse_result.ok()?;
    let scope_analysis = ScopeAnalysis::build(&self.eval_ctx, &program);

    // Find the symbol at this position
    // Check definitions first
    for def in scope_analysis.all_defs() {
      let (line, col) = self.eval_ctx.resolve_loc(def.loc);
      let name = self
        .eval_ctx
        .interned_symbols
        .with_resolved(def.name, |s| s.to_string())?;
      let end_col = col + name.len() as u32;

      if line == target_line && target_col >= col && target_col < end_col {
        let content = match def.kind {
          SymbolKind::Variable => format!("(variable) {name}"),
          SymbolKind::ClosureParam => format!("(parameter) {name}"),
          SymbolKind::Import => format!("(import) {name}"),
        };
        return Some(HoverInfo {
          content,
          start_line: line,
          start_col: col,
          end_line: line,
          end_col,
        });
      }
    }

    // Check references
    for sym_ref in scope_analysis.all_refs() {
      let (line, col) = self.eval_ctx.resolve_loc(sym_ref.loc);
      let name = self
        .eval_ctx
        .interned_symbols
        .with_resolved(sym_ref.name, |s| s.to_string())?;
      let end_col = col + name.len() as u32;

      if line == target_line && target_col >= col && target_col < end_col {
        // Check if it's a builtin
        if let Some((real_name, fn_def)) = self.lookup_builtin(&name) {
          let content = format_builtin_hover(real_name, fn_def);
          return Some(HoverInfo {
            content,
            start_line: line,
            start_col: col,
            end_line: line,
            end_col,
          });
        }

        // User-defined — show what we know
        let content = format!("(variable) {name}");
        return Some(HoverInfo {
          content,
          start_line: line,
          start_col: col,
          end_line: line,
          end_col,
        });
      }
    }

    // Check function calls (the function name part)
    for call_info in &scope_analysis.function_calls {
      let (line, col) = self.eval_ctx.resolve_loc(call_info.loc);
      let name = self
        .eval_ctx
        .interned_symbols
        .with_resolved(call_info.name, |s| s.to_string())?;
      let end_col = col + name.len() as u32;

      if line == target_line && target_col >= col && target_col < end_col {
        if !call_info.is_shadowed {
          if let Some((real_name, fn_def)) = self.lookup_builtin(&name) {
            let content = format_builtin_hover(real_name, fn_def);
            return Some(HoverInfo {
              content,
              start_line: line,
              start_col: col,
              end_line: line,
              end_col,
            });
          }
        }

        let content = format!("(function) {name}");
        return Some(HoverInfo {
          content,
          start_line: line,
          start_col: col,
          end_line: line,
          end_col,
        });
      }
    }

    // Fallback: check if the cursor is on a kwarg name via source-text scanning
    if let Some(hover) = self.hover_kwarg(src, target_line, target_col) {
      return Some(hover);
    }

    None
  }

  /// Try to produce hover info for a kwarg name by scanning the source text.
  fn hover_kwarg(&self, src: &str, target_line: u32, target_col: u32) -> Option<HoverInfo> {
    let offset = source_scan::line_col_to_offset(src, target_line, target_col)?;
    let call_info = source_scan::find_enclosing_call(src, offset)?;
    let kwarg_name = call_info.kwarg_name.as_deref()?;

    let (_canonical, fn_def) = self.lookup_builtin(&call_info.fn_name)?;

    // Find the matching ArgDef across all signatures
    for sig in fn_def.signatures {
      for arg in sig.arg_defs {
        if arg.name == kwarg_name {
          let types = geoscript::ArgType::list_from_bitflags(arg.valid_types);
          let type_str = types
            .iter()
            .map(|t| format!("{t:?}"))
            .collect::<Vec<_>>()
            .join(" | ");
          let mut content = format!("(parameter) **{kwarg_name}**: `{type_str}`");
          if !arg.description.is_empty() {
            content.push_str(&format!("\n{}", arg.description));
          }
          let kwarg_len = kwarg_name.len() as u32;
          return Some(HoverInfo {
            content,
            start_line: target_line,
            start_col: target_col,
            end_line: target_line,
            end_col: target_col + kwarg_len,
          });
        }
      }
    }

    None
  }

  /// Get completions at a given source location.
  pub fn completions(&self, src: &str, target_line: u32, target_col: u32, include_prelude: bool) -> Vec<CompletionItem> {
    let parse_result = geoscript::parse_program_maybe_with_prelude(
      &self.eval_ctx,
      src.to_owned(),
      include_prelude,
    );

    let mut items = Vec::new();

    // Even if parsing fails, we can still offer builtin completions
    if let Ok(program) = parse_result {
      let scope_analysis = ScopeAnalysis::build(&self.eval_ctx, &program);

      // Add in-scope user-defined variables
      for def in scope_analysis.definitions_visible_at(target_line, target_col) {
        let Some(name) = self
          .eval_ctx
          .interned_symbols
          .with_resolved(def.name, |s| s.to_string())
        else {
          continue;
        };

        items.push(CompletionItem {
          label: name,
          kind: match def.kind {
            SymbolKind::Variable => "variable".into(),
            SymbolKind::ClosureParam => "variable".into(),
            SymbolKind::Import => "variable".into(),
          },
          detail: String::new(),
          info: String::new(),
        });
      }
    }

    // Add builtins (sorted by name for consistency)
    let mut builtin_names: Vec<&str> = fn_sigs().entries().map(|(name, _)| *name).collect();
    builtin_names.sort();
    for name in builtin_names {
      let def = fn_sigs().get(name).unwrap();
      let detail = if let Some(sig) = def.signatures.first() {
        format_signature_oneliner(name, sig)
      } else {
        String::new()
      };
      let info = def
        .signatures
        .first()
        .map(|s| s.description.to_string())
        .unwrap_or_default();

      items.push(CompletionItem {
        label: name.to_string(),
        kind: "function".into(),
        detail,
        info,
      });
    }

    // If we're inside a function call, also suggest kwarg names for that function
    self.add_kwarg_completions(src, target_line, target_col, &mut items);

    items
  }

  /// If the cursor is inside a builtin function call, add completions for valid kwarg
  /// names (with `=` suffix) that haven't already been provided.
  fn add_kwarg_completions(
    &self,
    src: &str,
    target_line: u32,
    target_col: u32,
    items: &mut Vec<CompletionItem>,
  ) {
    let Some(offset) = source_scan::line_col_to_offset(src, target_line, target_col) else {
      return;
    };
    let Some(call_info) = source_scan::find_enclosing_call(src, offset) else {
      return;
    };
    let Some((_canonical, fn_def)) = self.lookup_builtin(&call_info.fn_name) else {
      return;
    };

    // Collect all unique kwarg names across all signatures
    let mut seen = FxHashSet::default();
    for sig in fn_def.signatures {
      for arg in sig.arg_defs {
        if arg.name.is_empty() || !seen.insert(arg.name) {
          continue;
        }
        let types = geoscript::ArgType::list_from_bitflags(arg.valid_types);
        let type_str = types
          .iter()
          .map(|t| format!("{t:?}"))
          .collect::<Vec<_>>()
          .join("|");

        items.push(CompletionItem {
          label: format!("{}=", arg.name),
          kind: "property".into(),
          detail: type_str,
          info: arg.description.to_string(),
        });
      }
    }
  }

  /// Get the definition location for the symbol at the given position.
  pub fn goto_definition(
    &self,
    src: &str,
    target_line: u32,
    target_col: u32,
    include_prelude: bool,
  ) -> Option<DefinitionLocation> {
    let program = geoscript::parse_program_maybe_with_prelude(
      &self.eval_ctx,
      src.to_owned(),
      include_prelude,
    )
    .ok()?;

    let scope_analysis = ScopeAnalysis::build(&self.eval_ctx, &program);

    // Find the reference at this position
    for sym_ref in scope_analysis.all_refs() {
      let (line, col) = self.eval_ctx.resolve_loc(sym_ref.loc);
      let name = self
        .eval_ctx
        .interned_symbols
        .with_resolved(sym_ref.name, |s| s.to_string())?;
      let end_col = col + name.len() as u32;

      if line == target_line && target_col >= col && target_col < end_col {
        // Look up the definition
        if let Some(def_loc) = sym_ref.resolved_def {
          let (def_line, def_col) = self.eval_ctx.resolve_loc(def_loc);
          return Some(DefinitionLocation {
            start_line: def_line,
            start_col: def_col,
            end_line: def_line,
            end_col: def_col + name.len() as u32,
          });
        }
        // It's a builtin or unresolved — no definition to go to
        return None;
      }
    }

    None
  }
}

fn format_signature_oneliner(name: &str, sig: &geoscript::builtins::fn_defs::FnSignature) -> String {
  let args: Vec<String> = sig
    .arg_defs
    .iter()
    .map(|arg| {
      let types = geoscript::ArgType::list_from_bitflags(arg.valid_types);
      let type_str = types
        .iter()
        .map(|t| format!("{t:?}"))
        .collect::<Vec<_>>()
        .join("|");
      match &arg.default_value {
        geoscript::builtins::fn_defs::DefaultValue::Required => {
          format!("{}: {type_str}", arg.name)
        }
        geoscript::builtins::fn_defs::DefaultValue::Optional(get_default) => {
          format!("{}: {type_str} = {:?}", arg.name, get_default())
        }
      }
    })
    .collect();
  format!("{}({})", name, args.join(", "))
}

fn format_builtin_hover(name: &str, fn_def: &FnDef) -> String {
  let mut parts = Vec::new();
  parts.push(format!("(builtin) **{}**", name));
  if !fn_def.module.is_empty() {
    parts.push(format!("Module: {}", fn_def.module));
  }

  for (i, sig) in fn_def.signatures.iter().enumerate() {
    if fn_def.signatures.len() > 1 {
      parts.push(format!("\nOverload {}:", i + 1));
    }
    parts.push(format!("`{}`", format_signature_oneliner(name, sig)));
    if !sig.description.is_empty() {
      parts.push(sig.description.to_string());
    }
  }

  parts.join("\n")
}

#[cfg(test)]
mod tests {
  use super::*;

  fn analyze(src: &str) -> AnalysisResult {
    let ctx = AnalysisCtx::new();
    ctx.analyze(src, false)
  }

  fn analyze_with_prelude(src: &str) -> AnalysisResult {
    let ctx = AnalysisCtx::new();
    ctx.analyze(src, true)
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
      result.diagnostics.iter().any(|d| d.message.contains("at most")),
      "Expected too-many-args diagnostic, got: {:?}",
      result.diagnostics
    );
  }

  #[test]
  fn test_invalid_kwarg_error() {
    let result = analyze("m = box(nonexistent_kwarg=5)");
    assert!(
      result.diagnostics.iter().any(|d| d.message.contains("unknown keyword argument")),
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
}
