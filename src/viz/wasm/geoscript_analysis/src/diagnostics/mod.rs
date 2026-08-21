use geoscript::{desugar::check_exit_positions, Program};

use crate::{analysis::Analysis, AnalysisCtx, AnalysisDiagnostic, AnalysisResult, DiagnosticSeverity};

mod call_args;
mod undefined;

pub(crate) fn analyze_program(ctx: &AnalysisCtx, program: &Program) -> AnalysisResult {
  let analysis = Analysis::build(&ctx.eval_ctx, program);
  let mut diagnostics = analysis.diagnostics.clone();

  undefined::check(ctx, &analysis, &mut diagnostics);
  call_args::check(ctx, &analysis, &mut diagnostics);
  exit_positions(ctx, program, &mut diagnostics);

  AnalysisResult { diagnostics }
}

/// `return`/`break` outside the positions the desugar accepts. Evaluation rejects these too, but
/// only once the program runs; surfacing them here puts a squiggle on the offending keyword.
fn exit_positions(
  ctx: &AnalysisCtx,
  program: &Program,
  diagnostics: &mut Vec<AnalysisDiagnostic>,
) {
  for (loc, message) in check_exit_positions(program) {
    let (line, col) = ctx.eval_ctx.resolve_loc(loc);
    if (line, col) == (0, 0) {
      continue;
    }
    let width = if message.starts_with("`return`") { 6 } else { 5 };
    diagnostics.push(AnalysisDiagnostic {
      start_line: line,
      start_col: col,
      end_line: line,
      end_col: col + width,
      severity: DiagnosticSeverity::Error,
      message,
    });
  }
}
