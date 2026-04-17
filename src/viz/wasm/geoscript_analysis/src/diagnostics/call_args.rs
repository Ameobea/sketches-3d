use geoscript::builtins::fn_defs::FnDef;

use crate::{
  analysis::Analysis, scope::FunctionCallInfo, AnalysisCtx, AnalysisDiagnostic,
  DiagnosticSeverity,
};

pub(super) fn check(
  ctx: &AnalysisCtx,
  analysis: &Analysis,
  diagnostics: &mut Vec<AnalysisDiagnostic>,
) {
  for call_info in &analysis.function_calls {
    let Some(name_str) = ctx
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

    let Some((_real_name, fn_def)) = ctx.lookup_builtin(&name_str) else {
      continue;
    };

    check_arity(ctx, call_info, fn_def, &name_str, diagnostics);
    check_kwarg_names(ctx, call_info, fn_def, &name_str, diagnostics);
  }
}

fn check_arity(
  ctx: &AnalysisCtx,
  call_info: &FunctionCallInfo,
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
  let too_many_positional = !fn_def
    .signatures
    .iter()
    .any(|sig| n_positional <= sig.arg_defs.len());

  if !too_many_positional {
    return;
  }

  let (line, col) = ctx.eval_ctx.resolve_loc(call_info.loc);
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

fn check_kwarg_names(
  ctx: &AnalysisCtx,
  call_info: &FunctionCallInfo,
  fn_def: &FnDef,
  name: &str,
  diagnostics: &mut Vec<AnalysisDiagnostic>,
) {
  for &kwarg_sym in &call_info.kwarg_names {
    let kwarg_valid = fn_def
      .signatures
      .iter()
      .any(|sig| sig.arg_defs.iter().any(|arg| arg.interned_name == kwarg_sym));

    if kwarg_valid {
      continue;
    }

    let (line, col) = ctx.eval_ctx.resolve_loc(call_info.loc);
    if line == 0 && col == 0 {
      continue;
    }

    let kwarg_name = ctx
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
      message: format!("unknown keyword argument `{kwarg_name}` for `{name}`"),
    });
  }
}
