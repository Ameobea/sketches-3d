use geoscript::Program;

use crate::{analysis::Analysis, AnalysisCtx, AnalysisResult};

mod call_args;
mod undefined;

pub(crate) fn analyze_program(ctx: &AnalysisCtx, program: &Program) -> AnalysisResult {
  let analysis = Analysis::build(&ctx.eval_ctx, program);
  let mut diagnostics = analysis.diagnostics.clone();

  undefined::check(ctx, &analysis, &mut diagnostics);
  call_args::check(ctx, &analysis, &mut diagnostics);

  AnalysisResult { diagnostics }
}
