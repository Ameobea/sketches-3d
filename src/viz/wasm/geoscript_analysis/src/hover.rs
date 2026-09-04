use geoscript::{builtins::fn_defs::FnDef, ty::AbstractType};

use crate::{
  analysis::Analysis,
  format::{builtin_docs, format_arg_type, format_partial_application},
  parse_lenient, resolve_draw_command, source_scan, AnalysisCtx, HoverInfo, SymbolKind,
};

fn plain(content: String, line: u32, col: u32, end_col: u32) -> HoverInfo {
  HoverInfo {
    content,
    builtin: None,
    active_signature: None,
    start_line: line,
    start_col: col,
    end_line: line,
    end_col,
  }
}

fn builtin(
  name: &str,
  fn_def: &FnDef,
  active_signature: Option<usize>,
  line: u32,
  col: u32,
  end_col: u32,
) -> HoverInfo {
  HoverInfo {
    content: String::new(),
    builtin: Some(builtin_docs(name, fn_def)),
    active_signature,
    start_line: line,
    start_col: col,
    end_line: line,
    end_col,
  }
}

fn describe_binding(
  kind: &str,
  name: &str,
  ty: Option<&AbstractType>,
  ctx: &AnalysisCtx,
) -> String {
  if let Some(AbstractType::PartiallyApplied(paf)) = ty {
    return format!(
      "({kind}) {name}\n\n{}",
      format_partial_application(paf, &ctx.eval_ctx)
    );
  }
  let type_suffix = ty
    .and_then(|t| t.display_str())
    .map(|s| format!(": {s}"))
    .unwrap_or_default();
  format!("({kind}) {name}{type_suffix}")
}

pub(crate) fn hover(
  ctx: &AnalysisCtx,
  src: &str,
  target_line: u32,
  target_col: u32,
  include_prelude: bool,
  ambient_src: &str,
) -> Option<HoverInfo> {
  let program = parse_lenient(&ctx.eval_ctx, src, include_prelude, ambient_src)?;
  let analysis = Analysis::build(&ctx.eval_ctx, &program);
  let def_types = &analysis.def_types;
  let resolve = |sym| {
    ctx
      .eval_ctx
      .interned_symbols
      .with_resolved(sym, |s| s.to_string())
  };

  for def in analysis.all_defs() {
    let (line, col) = ctx.eval_ctx.resolve_loc(def.loc);
    let name = resolve(def.name)?;
    // Not `ident_end_col`: a def's name always matches the source, and destructured bindings
    // record the RHS position, where probing would stretch the range over the whole RHS.
    let end_col = col + name.len() as u32;

    if line == target_line && target_col >= col && target_col < end_col {
      let kind = match def.kind {
        SymbolKind::Variable => "variable",
        SymbolKind::ClosureParam => "parameter",
        SymbolKind::Import => "import",
      };
      let content = describe_binding(kind, &name, def_types.get(&def.loc), ctx);
      return Some(plain(content, line, col, end_col));
    }
  }

  // Check function calls (the function name part) before references — both populate an entry
  // at the same loc for builtin call targets, but only the call entry knows which signature
  // matched, so we want it to win.
  for call_info in &analysis.function_calls {
    let (line, col) = ctx.eval_ctx.resolve_loc(call_info.loc);
    let name = resolve(call_info.name)?;
    if line != target_line || target_col < col {
      continue;
    }
    let end_col = source_scan::ident_end_col(src, line, col, name.len() as u32);

    if target_col < end_col {
      if !call_info.is_shadowed {
        if let Some((real_name, fn_def)) = ctx.lookup_builtin(&name) {
          return Some(builtin(
            real_name,
            fn_def,
            call_info.matched_sig_ix,
            line,
            col,
            end_col,
          ));
        }
      }
      return Some(plain(format!("(function) {name}"), line, col, end_col));
    }
  }

  // Check references (non-call references to builtins, or references to user variables)
  for sym_ref in analysis.all_refs() {
    let (line, col) = ctx.eval_ctx.resolve_loc(sym_ref.loc);
    let name = resolve(sym_ref.name)?;
    if line != target_line || target_col < col {
      continue;
    }
    let end_col = source_scan::ident_end_col(src, line, col, name.len() as u32);

    if target_col < end_col {
      if sym_ref.resolved_def.is_none() {
        if let Some((real_name, fn_def)) = ctx.lookup_builtin(&name) {
          return Some(builtin(real_name, fn_def, None, line, col, end_col));
        }
      }
      let ty = sym_ref
        .resolved_def
        .and_then(|def_loc| def_types.get(&def_loc));
      return Some(plain(
        describe_binding("variable", &name, ty, ctx),
        line,
        col,
        end_col,
      ));
    }
  }

  hover_kwarg(ctx, &analysis, src, target_line, target_col)
}

/// Hover for a kwarg name inside a builtin call, found by source scanning.
fn hover_kwarg(
  ctx: &AnalysisCtx,
  analysis: &Analysis,
  src: &str,
  target_line: u32,
  target_col: u32,
) -> Option<HoverInfo> {
  let offset = source_scan::line_col_to_offset(src, target_line, target_col)?;
  let (call, kwarg_name) = source_scan::kwarg_at(src, offset)?;
  let callee_loc = source_scan::offset_to_line_col(src, call.callee_offset);
  let shadowed = analysis
    .function_calls
    .iter()
    .any(|c| c.is_shadowed && ctx.eval_ctx.resolve_loc(c.loc) == callee_loc);
  if shadowed {
    return None;
  }
  let (_canonical, fn_def) =
    ctx.lookup_builtin(resolve_draw_command(&call.fn_name, call.in_path_block))?;
  let arg = fn_def
    .signatures
    .iter()
    .flat_map(|sig| sig.arg_defs)
    .find(|arg| arg.name == kwarg_name)?;

  let mut content = format!("(parameter) **{kwarg_name}**: `{}`", format_arg_type(arg));
  if !arg.description.is_empty() {
    content.push_str(&format!("\n{}", arg.description));
  }
  let (start, end) = source_scan::word_at(src, offset)?;
  let (line, col) = source_scan::offset_to_line_col(src, start);
  Some(plain(content, line, col, col + (end - start) as u32))
}
