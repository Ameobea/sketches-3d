//! Argument binding shared by execution, static inference, and editor guidance. Defaults are
//! evaluated only when materializing `ArgRef`s for execution.

use crate::{
  builtins::fn_defs::{DefaultValue, FnSignature},
  ArgRef, Sym,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgumentSource<P> {
  Positional(usize),
  Keyword(Sym, P),
  Default,
  Missing,
}

/// Keywords bind first; positional arguments fill the remaining parameters in order.
pub struct ArgumentBinder<K> {
  positional_count: usize,
  pub consumed: usize,
  keyword: K,
}

impl<P, K: Fn(Sym) -> Option<P>> ArgumentBinder<K> {
  pub fn new(positional_count: usize, keyword: K) -> Self {
    Self {
      positional_count,
      consumed: 0,
      keyword,
    }
  }

  pub fn next(&mut self, name: Option<Sym>, has_default: bool) -> ArgumentSource<P> {
    if let Some((name, found)) = name.and_then(|name| Some((name, (self.keyword)(name)?))) {
      ArgumentSource::Keyword(name, found)
    } else if self.consumed < self.positional_count {
      let ix = self.consumed;
      self.consumed += 1;
      ArgumentSource::Positional(ix)
    } else if has_default {
      ArgumentSource::Default
    } else {
      ArgumentSource::Missing
    }
  }
}

#[derive(Debug)]
pub struct SignatureBinding {
  pub partial: bool,
  /// Every possible argument type fits the parameters visited by this binding.
  pub certain: bool,
}

/// Binds one signature, reporting each parameter's source to `bound` in declaration order.
/// `stop_on_missing` is execution's partial-application rule: the first missing required
/// parameter ends the walk, and a call supplying nothing never binds. Editor guidance instead
/// walks on to see every remaining parameter.
pub fn bind_signature(
  sig: &FnSignature,
  positional_count: usize,
  positional_flags: impl Fn(usize) -> u32,
  kwargs: impl Iterator<Item = Sym> + Clone,
  keyword_flags: impl Fn(Sym) -> Option<u32>,
  allow_excess: bool,
  stop_on_missing: bool,
  mut bound: impl FnMut(usize, ArgumentSource<u32>),
) -> Option<SignatureBinding> {
  if kwargs
    .clone()
    .any(|name| !sig.arg_defs.iter().any(|arg| arg.interned_name == name))
  {
    return None;
  }
  let any_args = positional_count > 0 || kwargs.clone().next().is_some();
  let mut binder = ArgumentBinder::new(positional_count, keyword_flags);
  let mut result = SignatureBinding {
    partial: false,
    certain: true,
  };
  for (param_ix, arg) in sig.arg_defs.iter().enumerate() {
    let source = binder.next(
      Some(arg.interned_name),
      matches!(arg.default_value, DefaultValue::Optional(_)),
    );
    let flags = match source {
      ArgumentSource::Positional(ix) => positional_flags(ix),
      ArgumentSource::Keyword(_, flags) => flags,
      ArgumentSource::Default => {
        bound(param_ix, source);
        continue;
      }
      ArgumentSource::Missing => {
        result.partial = true;
        if stop_on_missing {
          return any_args.then_some(result);
        }
        bound(param_ix, source);
        continue;
      }
    };
    if flags & arg.valid_types == 0 {
      return None;
    }
    result.certain &= flags & !arg.valid_types == 0;
    bound(param_ix, source);
  }
  (allow_excess || binder.consumed == positional_count).then_some(result)
}

pub(crate) fn arg_ref(sig: &FnSignature, param_ix: usize, source: ArgumentSource<u32>) -> ArgRef {
  match source {
    ArgumentSource::Positional(ix) => ArgRef::Positional(ix),
    ArgumentSource::Keyword(name, _) => ArgRef::Keyword(name),
    ArgumentSource::Default => {
      let DefaultValue::Optional(default) = sig.arg_defs[param_ix].default_value else {
        unreachable!()
      };
      ArgRef::Default(default())
    }
    ArgumentSource::Missing => unreachable!(),
  }
}

pub fn max_positional_capacity(
  sigs: &[FnSignature],
  kwargs: impl Iterator<Item = Sym> + Clone,
) -> usize {
  sigs
    .iter()
    .filter(|sig| {
      kwargs
        .clone()
        .all(|name| sig.arg_defs.iter().any(|arg| arg.interned_name == name))
    })
    .map(|sig| {
      sig
        .arg_defs
        .iter()
        .filter(|arg| !kwargs.clone().any(|name| name == arg.interned_name))
        .count()
    })
    .max()
    .unwrap_or(0)
}
