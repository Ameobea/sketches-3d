use crate::{analysis::Analysis, AnalysisCtx, AnalysisDiagnostic, DiagnosticSeverity};

pub(super) fn check(
  ctx: &AnalysisCtx,
  analysis: &Analysis,
  diagnostics: &mut Vec<AnalysisDiagnostic>,
) {
  for unresolved in &analysis.unresolved_refs {
    let name = ctx
      .eval_ctx
      .interned_symbols
      .with_resolved(unresolved.name, |s| s.to_string());
    // Skip if name couldn't be resolved (shouldn't happen, but be safe)
    let Some(name) = name else { continue };

    let (line, col) = ctx.eval_ctx.resolve_loc(unresolved.loc);
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
}
