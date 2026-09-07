//! Editor guidance for a direct pipeline call. Call ownership and types come from the Pest AST;
//! the lexical cursor context only identifies which written argument is being edited.
//!
//! This is a set of possible edits, not an overload decision for evaluation. In particular,
//! inserting an argument moves the piped positional value to the right. Keep those possibilities
//! separate from the parameters the pipe supplies in the expression as currently written.

use geoscript::{
  builtins::fn_defs::{DefaultValue, FnSignature},
  ty::AbstractType,
  ArgType, Sym,
};
use nanoserde::SerJson;

use crate::{scope::FunctionCallInfo, source_scan::CallContext};

#[derive(Clone, Debug, SerJson)]
pub struct PipelineHelp {
  pub label: String,
  pub ty: Option<String>,
  /// True when another possible overload fully applies the RHS before the pipe is evaluated.
  pub uncertain: bool,
  /// Only annotate a bound parameter when viable overloads agree on its name and binding.
  pub params: Vec<Option<usize>>,
  pub available_kwargs: Vec<Vec<String>>,
}

pub(crate) struct PipelineGuidance {
  pub help: PipelineHelp,
  pub compatible: Vec<bool>,
  pub can_insert_positional: Vec<bool>,
  pub positional_params: Vec<Option<usize>>,
  pub preferred_signature: usize,
}

struct Binding {
  positional: Vec<usize>,
  free: Vec<usize>,
  complete: bool,
  certain: bool,
}

fn type_flags(ty: &AbstractType) -> u16 {
  match ty {
    AbstractType::Concrete(ty) => ty.as_bitflags(),
    AbstractType::Union(types) => types.iter().fold(0, |flags, ty| flags | ty.as_bitflags()),
    AbstractType::Callable(_) | AbstractType::PartiallyApplied(_) => {
      ArgType::Callable.as_bitflags()
    }
    AbstractType::Unknown => ArgType::Any.as_bitflags(),
  }
}

/// Replay the runtime's keyword-first, positional-second, default-last binding order. Missing
/// required parameters are allowed for possible future edits; incompatible known types aren't.
fn bind(
  sig: &FnSignature,
  args: &[AbstractType],
  kwargs: &[(Sym, AbstractType)],
) -> Option<Binding> {
  if kwargs
    .iter()
    .any(|(name, _)| !sig.arg_defs.iter().any(|p| p.interned_name == *name))
  {
    return None;
  }
  let mut binding = Binding {
    positional: Vec::new(),
    free: Vec::new(),
    complete: true,
    certain: true,
  };
  for (ix, param) in sig.arg_defs.iter().enumerate() {
    let ty = if let Some((_, ty)) = kwargs.iter().find(|(name, _)| *name == param.interned_name) {
      ty
    } else if let Some(ty) = args.get(binding.positional.len()) {
      binding.positional.push(ix);
      ty
    } else {
      binding.free.push(ix);
      if matches!(param.default_value, DefaultValue::Required) {
        binding.complete = false;
      }
      continue;
    };
    let flags = type_flags(ty);
    if flags & param.valid_types == 0 {
      return None;
    }
    binding.certain &= flags & !param.valid_types == 0;
  }
  // Extra arguments are sometimes ignored at runtime for callbacks. They don't identify a
  // useful parameter to suggest in the editor.
  (binding.positional.len() == args.len()).then_some(binding)
}

#[derive(Clone, Copy)]
struct Candidate {
  extra: usize,
  pipe_param: usize,
  complete: bool,
}

impl Candidate {
  fn score(self) -> (bool, std::cmp::Reverse<usize>) {
    (self.complete, std::cmp::Reverse(self.extra))
  }
}

pub(crate) fn pipeline_guidance(
  sigs: &[FnSignature],
  info: &FunctionCallInfo,
  call: &CallContext,
  completing_kwarg: bool,
) -> Option<PipelineGuidance> {
  let input = info.pipeline.as_ref()?;
  if info.is_shadowed
    || sigs
      .iter()
      .any(|sig| sig.arg_defs.first().is_none_or(|p| p.name.is_empty()))
  {
    return None;
  }
  let mut args = info.arg_types.clone();
  // `render_texture(name="out", us)` parses as a positional identifier. For keyword suggestions,
  // consider replacing that identifier with a keyword, rather than treating it as already bound.
  if completing_kwarg
    && call.current_kwarg.is_none()
    && info.arg_is_ident.get(call.positional_index) == Some(&true)
  {
    args.remove(call.positional_index);
  }
  let kwargs = &info.kwarg_types;
  let bindings: Vec<_> = sigs.iter().map(|sig| bind(sig, &args, kwargs)).collect();
  // A definite complete RHS is evaluated first, whether it returns a value for bit-or or a
  // new callable. Neither case pipes into this function's own parameter list.
  if bindings.iter().flatten().any(|b| b.complete && b.certain) {
    return None;
  }
  let uncertain = bindings.iter().flatten().any(|b| b.complete);
  let insertion = call.positional_index.min(args.len());
  let mut best = vec![None; sigs.len()];
  let mut can_insert_positional = vec![false; sigs.len()];
  for (ix, sig) in sigs.iter().enumerate() {
    // Several additional args may be needed before the pipe: e.g. mesh | translate(x, y, z).
    for extra in 0..sig.arg_defs.len().saturating_sub(args.len()) {
      let mut written = args.clone();
      written.splice(
        insertion..insertion,
        std::iter::repeat_n(AbstractType::Unknown, extra),
      );
      let Some(rhs) = bind(sig, &written, kwargs) else {
        continue;
      };
      if rhs.complete {
        continue;
      }
      if sigs
        .iter()
        .any(|other| bind(other, &written, kwargs).is_some_and(|b| b.complete && b.certain))
      {
        continue;
      }
      written.push(input.ty.clone());
      let Some(applied) = bind(sig, &written, kwargs) else {
        continue;
      };
      let candidate = Candidate {
        extra,
        pipe_param: *applied.positional.last()?,
        complete: applied.complete,
      };
      can_insert_positional[ix] |= extra > 0;
      if best[ix].is_none_or(|previous: Candidate| candidate.score() > previous.score()) {
        best[ix] = Some(candidate);
      }
    }
  }
  let preferred_signature = best
    .iter()
    .enumerate()
    .filter_map(|(ix, c)| c.map(|c| (ix, c)))
    .max_by_key(|(ix, c)| (c.score(), std::cmp::Reverse(*ix)))?
    .0;
  let compatible = best.iter().map(Option::is_some).collect();
  let mut params: Vec<_> = best
    .iter()
    .map(|candidate| {
      candidate.and_then(|c| {
        (c.extra == 0 && (!args.is_empty() || !kwargs.is_empty())).then_some(c.pipe_param)
      })
    })
    .collect();
  let name = params[preferred_signature].map(|ix| sigs[preferred_signature].arg_defs[ix].name);
  if uncertain
    || name.is_none()
    || best.iter().enumerate().any(|(ix, candidate)| {
      candidate.is_some() && params[ix].map(|p| sigs[ix].arg_defs[p].name) != name
    })
  {
    params.fill(None);
  }
  let available_kwargs = sigs
    .iter()
    .enumerate()
    .map(|(ix, sig)| {
      if best[ix].is_none() {
        return Vec::new();
      }
      bindings[ix]
        .as_ref()
        .map(|binding| {
          binding
            .free
            .iter()
            .copied()
            .filter(|p| best[ix].is_none_or(|candidate| *p != candidate.pipe_param))
            .map(|p| sig.arg_defs[p].name.to_owned())
            .collect()
        })
        .unwrap_or_default()
    })
    .collect();
  Some(PipelineGuidance {
    help: PipelineHelp {
      label: input.label.clone(),
      ty: input.ty.display_str(),
      uncertain,
      params,
      available_kwargs,
    },
    compatible,
    can_insert_positional,
    positional_params: sigs
      .iter()
      .map(|sig| {
        sig
          .arg_defs
          .iter()
          .enumerate()
          .filter(|(_, param)| !kwargs.iter().any(|(name, _)| *name == param.interned_name))
          .nth(call.positional_index)
          .map(|(ix, _)| ix)
      })
      .collect(),
    preferred_signature,
  })
}
