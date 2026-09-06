use fxhash::FxHashSet;
use geoscript::builtins::fn_defs::fn_sigs;

use crate::{
  analysis::Analysis,
  format::{format_arg_type, format_signature_oneliner},
  parse_lenient,
  pipeline_help::pipeline_guidance,
  source_scan, AnalysisCtx, CompletionItem, SymbolKind,
};

pub(crate) fn completions(
  ctx: &AnalysisCtx,
  src: &str,
  target_line: u32,
  target_col: u32,
  include_prelude: bool,
  ambient_src: &str,
) -> Vec<CompletionItem> {
  let mut items = Vec::new();

  // Even if parsing fails, we can still offer builtin completions
  let analysis = parse_lenient(&ctx.eval_ctx, src, include_prelude, ambient_src)
    .map(|program| Analysis::build(&ctx.eval_ctx, &program));
  if let Some(analysis) = &analysis {
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
        boost: 0,
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
      boost: 0,
    });
  }

  // If we're inside a function call, also suggest kwarg names for that function
  add_kwarg_completions(
    ctx,
    src,
    target_line,
    target_col,
    analysis.as_ref(),
    &mut items,
  );

  items
}

fn add_kwarg_completions(
  ctx: &AnalysisCtx,
  src: &str,
  target_line: u32,
  target_col: u32,
  analysis: Option<&Analysis>,
  items: &mut Vec<CompletionItem>,
) {
  let Some(offset) = source_scan::line_col_to_offset(src, target_line, target_col) else {
    return;
  };
  let Some(call_info) = source_scan::enclosing_call(src, offset) else {
    return;
  };
  let Some((_canonical, fn_def)) = ctx.lookup_builtin(&call_info.fn_name) else {
    return;
  };

  let call_pos = source_scan::offset_to_line_col(src, call_info.callee_offset);
  let guidance = analysis.and_then(|analysis| {
    let info = analysis
      .function_calls
      .iter()
      .find(|info| ctx.eval_ctx.resolve_loc(info.loc) == call_pos)?;
    pipeline_guidance(fn_def.signatures, info, &call_info, true)
  });

  // Collect all unique kwarg names across all signatures
  let mut seen = FxHashSet::default();
  for sig in fn_def.signatures {
    for arg in sig.arg_defs {
      if arg.name.is_empty() || !seen.insert(arg.name) {
        continue;
      }
      items.push(CompletionItem {
        label: format!("{}=", arg.name),
        kind: "property".to_owned(),
        detail: format_arg_type(arg),
        info: arg.description.to_owned(),
        boost: guidance.as_ref().map_or(0, |g| {
          if g.help.available_kwargs[g.preferred_signature]
            .iter()
            .any(|name| name == arg.name)
          {
            20
          } else if g
            .help
            .available_kwargs
            .iter()
            .any(|names| names.iter().any(|name| name == arg.name))
          {
            10
          } else {
            -10
          }
        }),
      });
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn pipeline_keywords_are_ranked_but_all_completions_remain_available() {
    for suffix in ["", "us"] {
      let src = format!(
        r#"diffuse = texture(8, 8, |uv| uv.x)
diffuse | render_texture(name="diffuse", {suffix})"#
      );
      let offset = src.len() - 1;
      let (line, col) = source_scan::offset_to_line_col(&src, offset);
      let items = AnalysisCtx::new().completions(&src, line, col, false, "");
      let boost = |name| items.iter().find(|item| item.label == name).unwrap().boost;
      assert!(boost("usage=") > boost("texture="));
      assert!(boost("usage=") > boost("name="));
      assert!(items.iter().any(|item| item.label == "diffuse"));
      assert!(items.iter().any(|item| item.label == "path"));
    }
  }
}
