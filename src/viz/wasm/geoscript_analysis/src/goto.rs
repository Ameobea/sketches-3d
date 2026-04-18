use geoscript::parse_program_maybe_with_prelude;

use crate::{analysis::Analysis, AnalysisCtx, DefinitionLocation};

pub(crate) fn goto_definition(
  ctx: &AnalysisCtx,
  src: &str,
  target_line: u32,
  target_col: u32,
  include_prelude: bool,
) -> Option<DefinitionLocation> {
  let program =
    parse_program_maybe_with_prelude(&ctx.eval_ctx, src.to_owned(), include_prelude).ok()?;

  let analysis = Analysis::build(&ctx.eval_ctx, &program);

  for sym_ref in analysis.all_refs() {
    let (line, col) = ctx.eval_ctx.resolve_loc(sym_ref.loc);
    let name = ctx
      .eval_ctx
      .interned_symbols
      .with_resolved(sym_ref.name, |s| s.to_string())?;
    let end_col = col + name.len() as u32;

    if line == target_line && target_col >= col && target_col < end_col {
      if let Some(def_loc) = sym_ref.resolved_def {
        let (def_line, def_col) = ctx.eval_ctx.resolve_loc(def_loc);
        return Some(DefinitionLocation {
          start_line: def_line,
          start_col: def_col,
          end_line: def_line,
          end_col: def_col + name.len() as u32,
        });
      }
      return None;
    }
  }

  None
}
