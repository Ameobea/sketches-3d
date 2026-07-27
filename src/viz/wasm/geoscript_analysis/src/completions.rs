use fxhash::FxHashSet;
use geoscript::{
  ast::PATH_BLOCK_REWRITE_MAP, builtins::fn_defs::fn_sigs,
  parse_program_maybe_with_prelude_and_ambient,
};

use crate::{
  analysis::Analysis, format::format_signature_oneliner, resolve_draw_command, source_scan,
  AnalysisCtx, CompletionItem, SymbolKind,
};

pub(crate) fn completions(
  ctx: &AnalysisCtx,
  src: &str,
  target_line: u32,
  target_col: u32,
  include_prelude: bool,
  ambient_src: &str,
) -> Vec<CompletionItem> {
  let parse_result = parse_program_maybe_with_prelude_and_ambient(
    &ctx.eval_ctx,
    src.to_owned(),
    include_prelude,
    ambient_src,
  );

  let mut items = Vec::new();

  // Even if parsing fails, we can still offer builtin completions
  if let Ok(program) = parse_result {
    let analysis = Analysis::build(&ctx.eval_ctx, &program);

    // Add in-scope user-defined variables
    for def in analysis.definitions_visible_at(&ctx.eval_ctx, target_line, target_col) {
      let Some(name) = ctx
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

  let in_path_block = source_scan::line_col_to_offset(src, target_line, target_col)
    .is_some_and(|offset| source_scan::in_path_block(src, offset));

  // Add builtins (sorted by name for consistency)
  let mut builtin_names: Vec<&str> = fn_sigs().entries().map(|(name, _)| *name).collect();
  builtin_names.sort();
  for name in builtin_names {
    // inside a `path { ... }` block the draw-command meaning of a name wins (`reverse`)
    if in_path_block && is_draw_command(name) {
      continue;
    }
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

  if in_path_block {
    add_draw_command_completions(&mut items);
  }

  // If we're inside a function call, also suggest kwarg names for that function
  add_kwarg_completions(ctx, src, target_line, target_col, in_path_block, &mut items);

  items
}

fn is_draw_command(name: &str) -> bool {
  PATH_BLOCK_REWRITE_MAP.iter().any(|(from, _)| *from == name)
}

/// The short draw-command names (`move`, `bezier`, ...) a `path { ... }` block accepts.  They
/// aren't builtins in their own right, so nothing else would offer them.
fn add_draw_command_completions(items: &mut Vec<CompletionItem>) {
  for (name, builtin) in PATH_BLOCK_REWRITE_MAP {
    let Some(def) = fn_sigs().get(builtin) else {
      continue;
    };
    let Some(sig) = def.signatures.first() else {
      continue;
    };
    items.push(CompletionItem {
      label: name.to_string(),
      kind: "function".into(),
      detail: format_signature_oneliner(name, sig),
      info: sig.description.to_string(),
    });
  }
}

/// If the cursor is inside a builtin function call, add completions for valid kwarg
/// names (with `=` suffix) that haven't already been provided.
fn add_kwarg_completions(
  ctx: &AnalysisCtx,
  src: &str,
  target_line: u32,
  target_col: u32,
  in_path_block: bool,
  items: &mut Vec<CompletionItem>,
) {
  let Some(offset) = source_scan::line_col_to_offset(src, target_line, target_col) else {
    return;
  };
  let Some(call_info) = source_scan::find_enclosing_call(src, offset) else {
    return;
  };
  let Some((_canonical, fn_def)) =
    ctx.lookup_builtin(resolve_draw_command(&call_info.fn_name, in_path_block))
  else {
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
        kind: "property".to_owned(),
        detail: type_str,
        info: arg.description.to_owned(),
      });
    }
  }
}
