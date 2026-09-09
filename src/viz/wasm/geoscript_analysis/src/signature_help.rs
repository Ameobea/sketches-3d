use geoscript::ty::AbstractType;
use nanoserde::SerJson;

use crate::{
  analysis::Analysis,
  format::{builtin_docs, BuiltinDocs, ParamDocs, SignatureDocs},
  parse_lenient,
  pipeline_help::{pipeline_guidance, PipelineHelp},
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
  pub pipeline: Option<PipelineHelp>,
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

  let name = call.fn_name.as_str();
  let shadowing_def = if call.uses_global_sigil {
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
        .map(|def| def.id)
    })
  };

  let (docs, matched_sig) = match shadowing_def {
    Some(id) => {
      let ty = &analysis.as_ref()?.definition(id).ty;
      let remaining;
      let callable = match ty {
        AbstractType::Callable(callable) => callable,
        AbstractType::PartiallyApplied(paf) => {
          remaining = geoscript::call_infer::remaining_closure(paf)?;
          &remaining
        }
        _ => return None,
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
              default: p.has_default.then(|| "…".to_owned()),
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

  let (mut active_params, mut compatible): (Vec<_>, Vec<_>) = docs
    .signatures
    .iter()
    .map(|sig| classify_signature(sig, &call))
    .unzip();

  // Prefer an overload with a param under the cursor; the AST's match (from the args typed
  // so far) breaks ties but can't override that, since it doesn't know an arg is being added.
  let has_active = |ix: usize| compatible[ix] && active_params[ix].is_some();
  let mut active_signature = matched_sig
    .filter(|&ix| has_active(ix))
    .or_else(|| (0..docs.signatures.len()).find(|&ix| has_active(ix)))
    .or(matched_sig)
    .or_else(|| compatible.iter().position(|c| *c))
    .unwrap_or(0);

  let pipeline = analysis.as_ref().and_then(|analysis| {
    let info = analysis
      .function_calls
      .iter()
      .find(|info| ctx.eval_ctx.resolve_loc(info.loc) == (call_line, call_col))?;
    let (_, def) = ctx.lookup_builtin(name)?;
    let guidance = pipeline_guidance(def.signatures, info, &call, false)?;
    if !guidance.help.uncertain {
      if call.current_kwarg.is_none() {
        active_params = guidance.positional_params;
      }
      // At an empty trailing slot, don't highlight the parameter supplied by the pipe unless
      // inserting another positional argument could still form a pipeline into this function.
      if call.current_kwarg.is_none() && call.positional_index >= info.arg_types.len() {
        for (ix, active) in active_params.iter_mut().enumerate() {
          if guidance.compatible[ix] && !guidance.can_insert_positional[ix] {
            *active = None;
          }
        }
      }
      compatible = guidance.compatible;
      active_signature = guidance.preferred_signature;
    }
    Some(guidance.help)
  });

  Some(SignatureHelp {
    docs,
    active_signature,
    active_params,
    compatible,
    call_line,
    call_col,
    pipeline,
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
  fn pen_ops_take_the_path_last() {
    let h = help("p = path() | move(‸)").unwrap();
    assert_eq!(h.docs.name, "move");
    assert_eq!(h.call_line, 1);
    assert_eq!(h.call_col, 14);
  }

  #[test]
  fn shadowed_builtin_hovers_are_suppressed() {
    let ctx = AnalysisCtx::new();
    let src = "box = |width| width\nm = box(width=2)";
    assert!(ctx.hover(src, 2, 10, false, "").is_none());
    let hover = ctx.hover("box = 1\ny = box", 2, 6, false, "").unwrap();
    assert!(hover.builtin.is_none(), "got: {hover:?}");
  }

  fn piped_param(h: &SignatureHelp) -> Option<&str> {
    h.pipeline.as_ref()?.params[h.active_signature].map(|ix| {
      h.docs.signatures[h.active_signature].params[ix]
        .name
        .as_str()
    })
  }

  #[test]
  fn piped_texture_leaves_optional_keyword_available() {
    let h = help(
      r#"diffuse = texture(8, 8, |uv| uv.x)
diffuse | render_texture(name="diffuse", ‸)"#,
    )
    .unwrap();
    assert_eq!(active(&h), None);
    assert_eq!(piped_param(&h), Some("texture"));
    let pipeline = h.pipeline.as_ref().unwrap();
    assert_eq!(pipeline.label, "diffuse");
    assert_eq!(pipeline.ty.as_deref(), Some("texture"));
    assert_eq!(pipeline.available_kwargs[h.active_signature], ["usage"]);
    assert!(!pipeline.uncertain);
  }

  #[test]
  fn pipe_does_not_occupy_the_argument_being_inserted() {
    let h = help(r#"2 | mul(‸)"#).unwrap();
    assert_eq!(active(&h), Some("a"));
    assert_eq!(piped_param(&h), None);
    assert!(h.pipeline.is_some());

    let h = help(r#"2 | mul(3, ‸)"#).unwrap();
    assert_eq!(active(&h), None);
    assert_eq!(piped_param(&h), Some("b"));
    assert!(h.pipeline.as_ref().unwrap().available_kwargs[h.active_signature].is_empty());

    let h = help(r#"2 | mul(‸3)"#).unwrap();
    assert_eq!(active(&h), Some("a"));
    assert_eq!(piped_param(&h), Some("b"));
  }

  #[test]
  fn pipeline_types_rank_overloads_without_losing_longer_edits() {
    let h = help(
      r#"my_mesh = box()
my_mesh | translate(‸)"#,
    )
    .unwrap();
    assert_eq!(active(&h), Some("translation"));
    assert_eq!(h.docs.signatures[h.active_signature].params[1].name, "mesh");
    assert!(h
      .docs
      .signatures
      .iter()
      .enumerate()
      .any(|(ix, sig)| sig.params.len() == 4 && sig.params[3].name == "mesh" && h.compatible[ix]));
    assert!(h.docs.signatures.iter().enumerate().all(|(ix, sig)| !sig
      .params
      .iter()
      .any(|p| p.name == "light")
      || !h.compatible[ix]));

    let h = help(r#"box() | translate(1, ‸)"#).unwrap();
    assert_eq!(active(&h), Some("y"));
    let h = help(r#"box() | translate(1, 2, ‸)"#).unwrap();
    assert_eq!(active(&h), Some("z"));
    let h = help(r#"box() | translate(1, 2, 3, ‸)"#).unwrap();
    assert_eq!(active(&h), None);
    assert_eq!(piped_param(&h), Some("mesh"));
  }

  #[test]
  fn kwargs_anywhere_in_the_call_follow_runtime_binding_order() {
    let h = help(r#"box() | translate(‸1, x=0, z=0)"#).unwrap();
    assert_eq!(active(&h), Some("y"));
    assert_eq!(piped_param(&h), Some("mesh"));
    let h = help(r#"box() | translate(x=0, 1, z=0, ‸)"#).unwrap();
    assert_eq!(active(&h), None);
    assert_eq!(piped_param(&h), Some("mesh"));
  }

  #[test]
  fn unknown_inputs_keep_structural_information_without_picking_a_type() {
    let h = help(r#"f = |diffuse| diffuse | render_texture(name="diffuse", ‸)"#).unwrap();
    assert_eq!(piped_param(&h), Some("texture"));
    assert_eq!(h.pipeline.as_ref().unwrap().ty, None);

    let h = help(r#"f = |obj| obj | translate(v3(0, 1, 0), ‸)"#).unwrap();
    assert_eq!(piped_param(&h), None, "mesh and light overloads disagree");
    assert!(h.pipeline.is_some());
  }

  #[test]
  fn complete_rhs_and_returned_callables_do_not_bind_outer_parameters() {
    for src in [
      r#"2 | mul(3, ‸4)"#,
      r#"box() | box(‸)"#,
      r#"2 | compose([add(1)], ‸)"#,
    ] {
      assert!(help(src).unwrap().pipeline.is_none(), "{src}");
    }
  }

  #[test]
  fn pipe_context_is_ast_owned_and_does_not_leak_into_nested_calls() {
    let h = help(r#"box() | translate(v3(0, ‸1, 0))"#).unwrap();
    assert_eq!(h.docs.name, "v3");
    assert!(h.pipeline.is_none());
    let h = help(r#"box() | scale(2) | translate(v3(0, 1, 0), ‸)"#).unwrap();
    assert_eq!(piped_param(&h), Some("mesh"));
    assert_eq!(h.pipeline.as_ref().unwrap().ty.as_deref(), Some("mesh"));
    let h = help(
      r#"// 2 | mul(
mul(‸)"#,
    )
    .unwrap();
    assert!(h.pipeline.is_none());
    let h = help(
      r#"box() |
// preceding pipeline can span lines
@trans(v3(0, 1, 0), ‸)"#,
    )
    .unwrap();
    assert_eq!(piped_param(&h), Some("mesh"));
  }

  #[test]
  fn incomplete_source_falls_back_to_ordinary_signature_help() {
    let h =
      help(r#"f = |diffuse: texture| diffuse | render_texture(name="diffuse", usage=‸)"#).unwrap();
    assert_eq!(active(&h), Some("usage"));
    assert!(h.pipeline.is_none());
  }
}
