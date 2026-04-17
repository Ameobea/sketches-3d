use geoscript::ty::AbstractType;

use crate::{
  analysis::Analysis,
  format::{format_builtin_hover, format_builtin_hover_with_sig, format_partial_application},
  source_scan, AnalysisCtx, HoverInfo, SymbolKind,
};

pub(crate) fn hover(
  ctx: &AnalysisCtx,
  src: &str,
  target_line: u32,
  target_col: u32,
  include_prelude: bool,
) -> Option<HoverInfo> {
  let parse_result = geoscript::parse_program_maybe_with_prelude(
    &ctx.eval_ctx,
    src.to_owned(),
    include_prelude,
  );

  let program = parse_result.ok()?;
  let analysis = Analysis::build(&ctx.eval_ctx, &program);
  let def_types = &analysis.def_types;

  // Find the symbol at this position
  // Check definitions first
  for def in analysis.all_defs() {
    let (line, col) = ctx.eval_ctx.resolve_loc(def.loc);
    let name = ctx
      .eval_ctx
      .interned_symbols
      .with_resolved(def.name, |s| s.to_string())?;
    let end_col = col + name.len() as u32;

    if line == target_line && target_col >= col && target_col < end_col {
      let ty = def_types.get(&def.loc);
      let content = if let Some(AbstractType::PartiallyApplied(paf)) = ty {
        let kind_str = match def.kind {
          SymbolKind::Variable => "variable",
          SymbolKind::ClosureParam => "parameter",
          SymbolKind::Import => "import",
        };
        format!(
          "({kind_str}) {name}\n\n{}",
          format_partial_application(paf, &ctx.eval_ctx)
        )
      } else {
        let type_suffix = ty
          .and_then(|t| t.display_str())
          .map(|s| format!(": {s}"))
          .unwrap_or_default();
        match def.kind {
          SymbolKind::Variable => format!("(variable) {name}{type_suffix}"),
          SymbolKind::ClosureParam => format!("(parameter) {name}{type_suffix}"),
          SymbolKind::Import => format!("(import) {name}{type_suffix}"),
        }
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

  // Check function calls (the function name part) before references — both populate an entry
  // at the same loc for builtin call targets, but only the call entry knows which signature
  // matched, so we want it to win.
  for call_info in &analysis.function_calls {
    let (line, col) = ctx.eval_ctx.resolve_loc(call_info.loc);
    let name = ctx
      .eval_ctx
      .interned_symbols
      .with_resolved(call_info.name, |s| s.to_string())?;
    let end_col = col + name.len() as u32;

    if line == target_line && target_col >= col && target_col < end_col {
      if !call_info.is_shadowed {
        if let Some((real_name, fn_def)) = ctx.lookup_builtin(&name) {
          let content = match call_info.matched_sig_ix {
            Some(sig_ix) => format_builtin_hover_with_sig(real_name, fn_def, sig_ix),
            None => format_builtin_hover(real_name, fn_def),
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

  // Check references (non-call references to builtins, or references to user variables)
  for sym_ref in analysis.all_refs() {
    let (line, col) = ctx.eval_ctx.resolve_loc(sym_ref.loc);
    let name = ctx
      .eval_ctx
      .interned_symbols
      .with_resolved(sym_ref.name, |s| s.to_string())?;
    let end_col = col + name.len() as u32;

    if line == target_line && target_col >= col && target_col < end_col {
      // Check if it's a builtin (referenced as a value, not called)
      if let Some((real_name, fn_def)) = ctx.lookup_builtin(&name) {
        let content = format_builtin_hover(real_name, fn_def);
        return Some(HoverInfo {
          content,
          start_line: line,
          start_col: col,
          end_line: line,
          end_col,
        });
      }

      // User-defined — show what we know, including type from definition site
      let ty = sym_ref
        .resolved_def
        .and_then(|def_loc| def_types.get(&def_loc));
      let content = if let Some(AbstractType::PartiallyApplied(paf)) = ty {
        format!(
          "(variable) {name}\n\n{}",
          format_partial_application(paf, &ctx.eval_ctx)
        )
      } else {
        let type_suffix = ty
          .and_then(|t| t.display_str())
          .map(|s| format!(": {s}"))
          .unwrap_or_default();
        format!("(variable) {name}{type_suffix}")
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

  // Fallback: check if the cursor is on a kwarg name via source-text scanning
  hover_kwarg(ctx, src, target_line, target_col)
}

/// Try to produce hover info for a kwarg name by scanning the source text.
fn hover_kwarg(
  ctx: &AnalysisCtx,
  src: &str,
  target_line: u32,
  target_col: u32,
) -> Option<HoverInfo> {
  let offset = source_scan::line_col_to_offset(src, target_line, target_col)?;
  let call_info = source_scan::find_enclosing_call(src, offset)?;
  let kwarg_name = call_info.kwarg_name.as_deref()?;

  let (_canonical, fn_def) = ctx.lookup_builtin(&call_info.fn_name)?;

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
