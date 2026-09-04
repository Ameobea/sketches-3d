use crate::{analysis::Analysis, parse_lenient, source_scan, AnalysisCtx, DefinitionLocation};

pub(crate) fn goto_definition(
  ctx: &AnalysisCtx,
  src: &str,
  target_line: u32,
  target_col: u32,
  include_prelude: bool,
  ambient_src: &str,
) -> Option<DefinitionLocation> {
  let program = parse_lenient(&ctx.eval_ctx, src, include_prelude, ambient_src)?;

  let analysis = Analysis::build(&ctx.eval_ctx, &program);

  for sym_ref in analysis.all_refs() {
    let (line, col) = ctx.eval_ctx.resolve_loc(sym_ref.loc);
    let name = ctx
      .eval_ctx
      .interned_symbols
      .with_resolved(sym_ref.name, |s| s.to_string())?;
    if line != target_line || target_col < col {
      continue;
    }
    let end_col = source_scan::ident_end_col(src, line, col, name.len() as u32);

    if target_col < end_col {
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
