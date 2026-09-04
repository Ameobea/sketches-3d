use geoscript::ty::AbstractType;
use nanoserde::SerJson;

use crate::{
  analysis::Analysis,
  format::{builtin_docs, BuiltinDocs, ParamDocs, SignatureDocs},
  parse_lenient, resolve_draw_command,
  source_scan::{self, CallContext},
  AnalysisCtx,
};

/// Docs for the call the cursor is inside of, with the param being typed resolved per overload.
#[derive(Clone, Debug, SerJson)]
pub struct SignatureHelp {
  pub docs: BuiltinDocs,
  pub active_signature: usize,
  /// Per signature: index of the param the cursor's argument binds to, if it has one.
  pub active_params: Vec<Option<usize>>,
  /// Per signature: whether the args typed so far could still fit it.
  pub compatible: Vec<bool>,
  /// Where the callee is written; identifies the call across edits.
  pub call_line: u32,
  pub call_col: u32,
}

pub(crate) fn signature_help(
  ctx: &AnalysisCtx,
  src: &str,
  target_line: u32,
  target_col: u32,
  include_prelude: bool,
  ambient_src: &str,
) -> Option<SignatureHelp> {
  let offset = source_scan::line_col_to_offset(src, target_line, target_col)?;
  let call = source_scan::enclosing_call(src, offset)?;
  let (call_line, call_col) = source_scan::offset_to_line_col(src, call.callee_offset);

  let analysis = parse_lenient(&ctx.eval_ctx, src, include_prelude, ambient_src)
    .map(|program| Analysis::build(&ctx.eval_ctx, &program));

  // a draw command inside `path { }` always means the builtin, as in the evaluator's rewrite
  let name = resolve_draw_command(&call.fn_name, call.in_path_block);
  let shadowing_def = if call.uses_global_sigil || name != call.fn_name {
    None
  } else {
    analysis.as_ref().and_then(|a| {
      a.definitions_visible_at(&ctx.eval_ctx, call_line, call_col)
        .into_iter()
        .find(|def| {
          ctx
            .eval_ctx
            .interned_symbols
            .with_resolved(def.name, |s| s == call.fn_name)
            .unwrap_or(false)
        })
        .map(|def| def.loc)
    })
  };

  let (docs, matched_sig) = match shadowing_def {
    Some(def_loc) => {
      let AbstractType::Callable(callable) = analysis.as_ref()?.def_types.get(&def_loc)? else {
        return None;
      };
      let display = |ty: &AbstractType| ty.display_str().unwrap_or_else(|| "?".to_owned());
      let docs = BuiltinDocs {
        name: call.fn_name.clone(),
        module: String::new(),
        signatures: vec![SignatureDocs {
          params: callable
            .params
            .iter()
            .map(|p| ParamDocs {
              name: p.name.clone().unwrap_or_else(|| "_".to_owned()),
              ty: display(&p.ty),
              default: None,
              description: String::new(),
            })
            .collect(),
          description: String::new(),
          return_type: callable.return_type.display_str().unwrap_or_default(),
        }],
      };
      (docs, None)
    }
    None => {
      let (real_name, fn_def) = ctx.lookup_builtin(name)?;
      let matched = analysis.as_ref().and_then(|a| {
        a.function_calls
          .iter()
          .find(|c| ctx.eval_ctx.resolve_loc(c.loc) == (call_line, call_col))
          .and_then(|c| c.matched_sig_ix)
      });
      (builtin_docs(real_name, fn_def), matched)
    }
  };

  let (active_params, compatible): (Vec<_>, Vec<_>) = docs
    .signatures
    .iter()
    .map(|sig| classify_signature(sig, &call))
    .unzip();

  // Prefer an overload with a param under the cursor; the AST's match (from the args typed
  // so far) breaks ties but can't override that, since it doesn't know an arg is being added.
  let has_active = |ix: usize| compatible[ix] && active_params[ix].is_some();
  let active_signature = matched_sig
    .filter(|&ix| has_active(ix))
    .or_else(|| (0..docs.signatures.len()).find(|&ix| has_active(ix)))
    .or(matched_sig)
    .or_else(|| compatible.iter().position(|c| *c))
    .unwrap_or(0);

  Some(SignatureHelp {
    docs,
    active_signature,
    active_params,
    compatible,
    call_line,
    call_col,
  })
}

/// Positionals fill params in order skipping those bound by name, mirroring the runtime's
/// `match_signature_by_arg_types`.
fn classify_signature(sig: &SignatureDocs, call: &CallContext) -> (Option<usize>, bool) {
  if sig.params.first().is_some_and(|p| p.name.is_empty()) {
    return (None, true);
  }
  let has = |name: &str| sig.params.iter().any(|p| p.name == name);
  if !call.kwargs_before.iter().all(|k| has(k)) {
    return (None, false);
  }
  if let Some(kwarg) = &call.current_kwarg {
    let ix = sig.params.iter().position(|p| &p.name == kwarg);
    return (ix, ix.is_some());
  }
  let mut free = sig
    .params
    .iter()
    .enumerate()
    .filter(|(_, p)| !call.kwargs_before.contains(&p.name));
  let capacity = free.clone().count();
  (
    free.nth(call.positional_index).map(|(ix, _)| ix),
    call.positional_index <= capacity,
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  const CURSOR: char = '‸';

  fn help(src: &str) -> Option<SignatureHelp> {
    let offset = src.find(CURSOR).expect("cursor marker");
    let src = src.replacen(CURSOR, "", 1);
    let (line, col) = source_scan::offset_to_line_col(&src, offset);
    AnalysisCtx::new().signature_help(&src, line, col, false, "")
  }

  fn active(h: &SignatureHelp) -> Option<&str> {
    h.active_params[h.active_signature].map(|ix| {
      h.docs.signatures[h.active_signature].params[ix]
        .name
        .as_str()
    })
  }

  #[test]
  fn positional_then_kwarg() {
    let h = help("path_difference(‸)").unwrap();
    assert_eq!(h.docs.name, "path_difference");
    assert_eq!(active(&h), Some("subject"));

    let h = help("path_difference(a, ‸)").unwrap();
    assert_eq!(active(&h), Some("clip"));

    let h = help("path_difference(a, cl‸ip=b)").unwrap();
    assert_eq!(active(&h), Some("clip"));

    let h = help("translate(translation=v3(1, 0, 0), ‸)").unwrap();
    assert_eq!(active(&h), Some("mesh"));
  }

  #[test]
  fn overload_selection_follows_typed_args() {
    let h = help("box(‸)").unwrap();
    assert!(h.docs.signatures.len() > 1);
    assert!(h.compatible[h.active_signature]);
    assert!(active(&h).is_some());

    let h = help("box(1, ‸)").unwrap();
    assert_eq!(active(&h), Some("height"));

    let h = help("box(1, 2, 3, ‸)").unwrap();
    assert_eq!(h.compatible, vec![true, false]);

    let h = help("box(nonsense_kwarg=1, ‸)").unwrap();
    assert!(h.compatible.iter().all(|c| !c));
  }

  #[test]
  fn no_help_outside_calls_or_for_unknown_callees() {
    assert!(help("x = 1 + ‸2").is_none());
    assert!(help("garbage_fn(‸)").is_none());
    assert!(help("box(\"‸\")").is_none());
  }

  #[test]
  fn shadowed_callee_uses_closure_params() {
    let h = help("box = |w: float, h: float| w * h\nbox(‸)").unwrap();
    assert_eq!(h.docs.module, "");
    assert_eq!(active(&h), Some("w"));
    let h = help("box = 1\nbox(‸)");
    assert!(h.is_none());
    let h = help("box = 1\n@box(‸)").unwrap();
    assert_eq!(h.docs.module, "mesh");
  }

  #[test]
  fn broken_line_elsewhere_keeps_ast_refinement() {
    let h = help("b = 1\nfoo(a, b, curve=)\nbox(‸)").unwrap();
    assert!(h.compatible.iter().any(|c| *c));
  }

  #[test]
  fn draw_commands_in_path_blocks() {
    let h = help("p = path {\n  move(‸)\n}").unwrap();
    assert_eq!(h.docs.name, "path_move");
    assert_eq!(h.call_line, 2);
    assert_eq!(h.call_col, 3);
    let h = help("circle = |r: float| r\np = path {\n  circle(1‸)\n}").unwrap();
    assert_eq!(h.docs.name, "path_circle");
  }

  #[test]
  fn shadowed_builtin_hovers_are_suppressed() {
    let ctx = AnalysisCtx::new();
    let src = "box = |width| width\nm = box(width=2)";
    assert!(ctx.hover(src, 2, 10, false, "").is_none());
    let hover = ctx.hover("box = 1\ny = box", 2, 6, false, "").unwrap();
    assert!(hover.builtin.is_none(), "got: {hover:?}");
  }
}
