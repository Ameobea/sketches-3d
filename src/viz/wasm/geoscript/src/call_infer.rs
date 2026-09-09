//! Abstract callable application. Result types describe successful evaluation; only
//! `matched_signature` proves which builtin overload and binding the runtime will select.

use crate::{
  ast::{ClosureArg, DestructurePattern},
  builtins::{
    fn_defs::{fn_sigs, get_builtin_fn_sig_entry_ix},
    FUNCTION_ALIASES,
  },
  call_binding::{bind_signature, max_positional_capacity, ArgumentBinder, ArgumentSource},
  ty::{merge_types, AbstractType, CallableParam, CallableType, PartialApplication},
  ArgType, Callable, EvalCtx, Sym, Value,
};

#[derive(Debug)]
pub struct CallResolution {
  pub return_ty: AbstractType,
  pub matched_signature: Option<usize>,
  pub error: Option<String>,
}

impl CallResolution {
  pub fn inferred(return_ty: AbstractType) -> Self {
    Self {
      return_ty,
      matched_signature: None,
      error: None,
    }
  }
  fn invalid(message: String) -> Self {
    Self {
      return_ty: AbstractType::Unknown,
      matched_signature: None,
      error: Some(message),
    }
  }
  pub fn into_abstract_type(self) -> AbstractType {
    self.return_ty
  }
}

pub fn builtin_type(ctx: &EvalCtx, name: Sym) -> Option<AbstractType> {
  let name = ctx
    .interned_symbols
    .with_resolved(name, |s| s.strip_prefix('@').unwrap_or(s).to_owned())?;
  let name = FUNCTION_ALIASES
    .get(name.as_str())
    .copied()
    .unwrap_or(&name);
  let ix = get_builtin_fn_sig_entry_ix(name)?;
  Some(AbstractType::Builtin(fn_sigs().entries[ix].0.to_owned()))
}

pub fn callable_param(ctx: &EvalCtx, param: &ClosureArg) -> CallableParam {
  let symbol = match param.ident {
    DestructurePattern::Ident(name, _) => Some(name),
    _ => None,
  };
  CallableParam {
    name: symbol.and_then(|name| ctx.interned_symbols.with_resolved(name, str::to_owned)),
    symbol,
    has_default: param.default_val.is_some(),
    ty: param
      .type_hint
      .map(AbstractType::Concrete)
      .unwrap_or(AbstractType::Unknown),
  }
}

/// Retain callable identity when optimization turns a reference or closure into a value.
/// Opaque runtime closures expose annotations and parameter shape, without reanalyzing captures.
pub fn value_type(ctx: &EvalCtx, value: &Value) -> AbstractType {
  match value {
    Value::Callable(callable) => literal_callable_type(ctx, callable),
    _ => AbstractType::Concrete(value.get_type()),
  }
}

pub fn literal_callable_type(ctx: &EvalCtx, callable: &Callable) -> AbstractType {
  match callable {
    Callable::Builtin { fn_entry_ix, .. } => {
      AbstractType::Builtin(fn_sigs().entries[*fn_entry_ix].0.to_owned())
    }
    Callable::Closure(closure) => AbstractType::Callable(CallableType {
      params: closure
        .params
        .iter()
        .map(|param| callable_param(ctx, param))
        .collect(),
      return_type: Box::new(
        callable
          .get_return_type_hint()
          .map(AbstractType::Concrete)
          .unwrap_or(AbstractType::Unknown),
      ),
    }),
    Callable::PartiallyAppliedFn(paf) => AbstractType::PartiallyApplied(PartialApplication {
      target: Box::new(literal_callable_type(ctx, &paf.inner)),
      bound_args: paf
        .args
        .iter()
        .map(|value| value_type(ctx, value))
        .collect(),
      bound_kwargs: paf
        .kwargs
        .iter()
        .map(|(name, value)| (*name, value_type(ctx, value)))
        .collect(),
    }),
    Callable::Dynamic { .. } => AbstractType::OpaqueCallable(Box::new(
      callable
        .get_return_type_hint()
        .map(AbstractType::Concrete)
        .unwrap_or(AbstractType::Unknown),
    )),
    _ => AbstractType::Concrete(ArgType::Callable),
  }
}

pub fn resolve_literal_call(
  ctx: &EvalCtx,
  callable: &Callable,
  args: &[AbstractType],
  kwargs: &[(Sym, AbstractType)],
) -> CallResolution {
  resolve_call(ctx, &literal_callable_type(ctx, callable), args, kwargs)
}

fn partial(
  target: &AbstractType,
  args: &[AbstractType],
  kwargs: &[(Sym, AbstractType)],
) -> AbstractType {
  AbstractType::PartiallyApplied(PartialApplication {
    target: Box::new(target.clone()),
    bound_args: args.to_vec(),
    bound_kwargs: kwargs.to_vec(),
  })
}

pub fn resolve_paf_call(
  ctx: &EvalCtx,
  paf: &PartialApplication,
  args: &[AbstractType],
  kwargs: &[(Sym, AbstractType)],
) -> CallResolution {
  let mut combined_args = paf.bound_args.clone();
  combined_args.extend_from_slice(args);
  let mut combined_kwargs = paf.bound_kwargs.clone();
  for (name, ty) in kwargs {
    if let Some((_, old)) = combined_kwargs.iter_mut().find(|(key, _)| key == name) {
      *old = ty.clone();
    } else {
      combined_kwargs.push((*name, ty.clone()));
    }
  }
  resolve_call(ctx, &paf.target, &combined_args, &combined_kwargs)
}

/// The unbound closure parameters for editor presentation. Captured keywords remain overridable
/// at application time; this view describes the arguments still needed for an ordinary call.
pub fn remaining_closure(paf: &PartialApplication) -> Option<CallableType> {
  let AbstractType::Callable(callable) = &*paf.target else {
    return None;
  };
  let mut binder = ArgumentBinder::new(paf.bound_args.len(), |name| {
    paf.bound_kwargs.iter().find(|(key, _)| *key == name)
  });
  let params = callable
    .params
    .iter()
    .filter(|param| {
      matches!(
        binder.next(param.symbol, param.has_default),
        ArgumentSource::Default | ArgumentSource::Missing
      )
    })
    .cloned()
    .collect();
  Some(CallableType {
    params,
    return_type: callable.return_type.clone(),
  })
}

pub fn resolve_builtin_call(
  ctx: &EvalCtx,
  name: Sym,
  args: &[AbstractType],
  kwargs: &[(Sym, AbstractType)],
) -> CallResolution {
  match builtin_type(ctx, name) {
    Some(target) => resolve_call(ctx, &target, args, kwargs),
    None => CallResolution::inferred(AbstractType::Unknown),
  }
}

fn resolve_builtin(
  target: &AbstractType,
  name: &str,
  args: &[AbstractType],
  kwargs: &[(Sym, AbstractType)],
) -> CallResolution {
  let Some(def) = fn_sigs().get(name) else {
    return CallResolution::inferred(AbstractType::Unknown);
  };
  let sigs = def.signatures;
  if sigs
    .first()
    .and_then(|sig| sig.arg_defs.first())
    .is_some_and(|arg| arg.name.is_empty())
  {
    return CallResolution::inferred(AbstractType::from_return_type(sigs[0].return_type));
  }
  let allow_excess =
    args.len() > max_positional_capacity(sigs, kwargs.iter().map(|(name, _)| *name));
  let mut result: Option<AbstractType> = None;
  let mut possible_partial = false;
  let mut proven_complete = false;
  let mut matched_signature = None;
  for (ix, sig) in sigs.iter().enumerate() {
    let Some(binding) = bind_signature(
      sig,
      args.len(),
      |ix| args[ix].possible_type_flags(),
      kwargs.iter().map(|(name, _)| *name),
      |name| {
        kwargs
          .iter()
          .find(|(key, _)| *key == name)
          .map(|(_, ty)| ty.possible_type_flags())
      },
      allow_excess,
      true,
      |_, _| {},
    ) else {
      continue;
    };
    if binding.partial {
      possible_partial = true;
      continue;
    }
    if binding.certain && result.is_none() {
      matched_signature = Some(ix);
    }
    let ty = AbstractType::from_return_type(sig.return_type);
    result = Some(match result {
      Some(previous) => merge_types(&previous, &ty),
      None => ty,
    });
    // Later signatures cannot be selected when this one accepts every possible input.
    if binding.certain {
      proven_complete = true;
      break;
    }
  }
  if possible_partial && !proven_complete {
    let paf = partial(target, args, kwargs);
    result = Some(match result {
      Some(ty) => merge_types(&ty, &paf),
      None => paf,
    });
  }
  match result {
    Some(return_ty) => CallResolution {
      return_ty,
      matched_signature,
      error: None,
    },
    None => {
      let show = |ty: &AbstractType| ty.display_str().unwrap_or_else(|| "?".to_owned());
      let mut types: Vec<_> = args.iter().map(show).collect();
      types.extend(kwargs.iter().map(|(_, ty)| format!("_={}", show(ty))));
      CallResolution::invalid(format!(
        "no overload of `{name}` matches argument types ({})",
        types.join(", ")
      ))
    }
  }
}

pub fn resolve_call(
  ctx: &EvalCtx,
  target: &AbstractType,
  args: &[AbstractType],
  kwargs: &[(Sym, AbstractType)],
) -> CallResolution {
  match target {
    AbstractType::Builtin(name) => resolve_builtin(target, name, args, kwargs),
    AbstractType::OpaqueCallable(return_ty) => CallResolution::inferred((**return_ty).clone()),
    AbstractType::PartiallyApplied(paf) => resolve_paf_call(ctx, paf, args, kwargs),
    AbstractType::Callable(callable) => {
      let mut binder = ArgumentBinder::new(args.len(), |name| {
        kwargs
          .iter()
          .find(|(key, _)| *key == name)
          .map(|(_, ty)| ty)
      });
      let mut supplied = false;
      let mut missing = None;
      for (ix, param) in callable.params.iter().enumerate() {
        let ty = match binder.next(param.symbol, param.has_default) {
          ArgumentSource::Positional(ix) => &args[ix],
          ArgumentSource::Keyword(_, ty) => ty,
          ArgumentSource::Default => continue,
          ArgumentSource::Missing => {
            missing.get_or_insert(ix);
            continue;
          }
        };
        supplied = true;
        if ty.possible_type_flags() & param.ty.possible_type_flags() == 0 {
          return CallResolution::invalid(format!(
            "type mismatch for closure argument `{}`: expected {}, found {}",
            param.name.clone().unwrap_or_else(|| format!("#{}", ix + 1)),
            param.ty.display_str().unwrap_or_else(|| "?".to_owned()),
            ty.display_str().unwrap_or_else(|| "?".to_owned())
          ));
        }
      }
      if let Some(ix) = missing {
        if supplied {
          CallResolution::inferred(partial(target, args, kwargs))
        } else {
          CallResolution::invalid(format!(
            "missing required closure argument `{}`",
            callable.params[ix]
              .name
              .clone()
              .unwrap_or_else(|| format!("#{}", ix + 1))
          ))
        }
      } else {
        CallResolution::inferred((*callable.return_type).clone())
      }
    }
    AbstractType::Concrete(ArgType::Path) => {
      let arg = match (args, kwargs) {
        ([arg], []) => Some(arg),
        ([], [(name, arg)])
          if ctx
            .interned_symbols
            .with_resolved(*name, |name| name == "t")
            == Some(true) =>
        {
          Some(arg)
        }
        _ => None,
      };
      if arg.is_some_and(|ty| ty.possible_type_flags() & ArgType::Numeric.as_bitflags() != 0) {
        CallResolution::inferred(AbstractType::Concrete(ArgType::Vec2))
      } else {
        CallResolution::invalid("paths take exactly one numeric argument `t`".to_owned())
      }
    }
    _ if target.possible_type_flags()
      & (ArgType::Callable.as_bitflags() | ArgType::Path.as_bitflags())
      != 0 =>
    {
      CallResolution::inferred(AbstractType::Unknown)
    }
    _ => CallResolution::invalid(format!(
      "value of type `{}` is not callable",
      target.display_str().unwrap_or_else(|| "?".to_owned())
    )),
  }
}

/// Evaluate the RHS to a value first, then invoke it if callable or dispatch bit-or.
pub fn resolve_pipeline(ctx: &EvalCtx, lhs: &AbstractType, rhs: &AbstractType) -> CallResolution {
  match rhs {
    AbstractType::Builtin(_)
    | AbstractType::Callable(_)
    | AbstractType::PartiallyApplied(_)
    | AbstractType::OpaqueCallable(_) => resolve_call(ctx, rhs, std::slice::from_ref(lhs), &[]),
    _ if rhs.possible_type_flags() & ArgType::Callable.as_bitflags() != 0 => {
      CallResolution::inferred(AbstractType::Unknown)
    }
    _ => resolve_builtin(
      &AbstractType::Builtin("bit_or".to_owned()),
      "bit_or",
      &[lhs.clone(), rhs.clone()],
      &[],
    ),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn host_callable_return_contract_survives_value_and_pipeline_inference() {
    let ctx = EvalCtx::default();
    let mut program = parse_program_src(&ctx, r#"ramp([[0., 0.], [1., 10.]])"#).unwrap();
    optimize_ast(&ctx, &mut program).unwrap();
    let value = crate::eval_program_into_scope(&ctx, &program, &ctx.globals).unwrap();
    let target = value_type(&ctx, &value);
    let input = AbstractType::Concrete(ArgType::Float);
    assert!(matches!(
      resolve_call(&ctx, &target, std::slice::from_ref(&input), &[]).return_ty,
      AbstractType::Concrete(ArgType::Float)
    ));
    assert!(matches!(
      resolve_pipeline(&ctx, &input, &target).return_ty,
      AbstractType::Concrete(ArgType::Float)
    ));
  }

  #[test]
  fn preselection_respects_earlier_possible_overloads() {
    use crate::builtins::fn_defs::{ArgDef, DefaultValue, FnSignature};
    static SIGS: &[FnSignature] = &[
      FnSignature {
        arg_defs: &[ArgDef {
          name: "x",
          interned_name: Sym(0),
          valid_types: ArgType::Int.as_bitflags(),
          default_value: DefaultValue::Required,
          description: "",
        }],
        return_type: &[ArgType::Int],
        description: "",
      },
      FnSignature {
        arg_defs: &[ArgDef {
          name: "x",
          interned_name: Sym(0),
          valid_types: ArgType::Numeric.as_bitflags(),
          default_value: DefaultValue::Required,
          description: "",
        }],
        return_type: &[ArgType::Float],
        description: "",
      },
    ];
    assert!(crate::match_signature_by_arg_types(SIGS, &[ArgType::Numeric], &[]).is_none());
    assert_eq!(
      crate::match_signature_by_arg_types(SIGS, &[ArgType::Int], &[])
        .unwrap()
        .def_ix,
      0
    );
    assert_eq!(
      crate::match_signature_by_arg_types(SIGS, &[ArgType::Float], &[])
        .unwrap()
        .def_ix,
      1
    );
  }

  #[test]
  fn optimized_and_unoptimized_application_agree() {
    fn run(src: &str, optimize: bool) -> String {
      let ctx = EvalCtx::default();
      let mut program = parse_program_src(&ctx, src).unwrap();
      crate::desugar::desugar_exits(&ctx, &mut program).unwrap();
      if optimize {
        optimize_ast(&ctx, &mut program).unwrap();
      } else {
        crate::resolve::resolve_program(&mut program).unwrap();
      }
      format!(
        "{:?}",
        crate::eval_program_into_scope(&ctx, &program, &ctx.globals).unwrap()
      )
    }
    for src in [
      r#"a = 10
f = |a: int, b: int=(a+1)| b
f(2)"#,
      r#"f = |x: num| abs(x)
f(9007199254740993)"#,
      r#"f = |x: num| x+1
f(2)"#,
      r#"f = |x: float| {
p = sub(a=v3(1))
p(x, a=4)
}
f(2.5)"#,
      r#"f = |x: int, y: int=4| x+y
p = f(y=2)
p(1, y=3)"#,
      r#"factory = |x: int| |y: int| x+y
3 | factory(2)"#,
    ] {
      assert_eq!(run(src, true), run(src, false), "{src}");
    }
  }

  use crate::{
    ast::{Expr, FunctionCallTarget, Statement, TopLevelStatement},
    optimizer::optimize_ast,
    parse_program_src,
  };

  #[test]
  fn inferred_returns_enable_safe_downstream_preselection() {
    let ctx = EvalCtx::default();
    let mut program = parse_program_src(
      &ctx,
      r#"f = |items: seq, t: float| {
      a = lerp_paths(items[0], items[1], t)
      b = lerp_paths(items[1], items[0], t)
      path_union(a, b)
    }"#,
    )
    .unwrap();
    optimize_ast(&ctx, &mut program).unwrap();
    let TopLevelStatement::Statement(Statement::Assignment {
      expr: Expr::Literal {
        value: Value::Callable(callable),
        ..
      },
      ..
    }) = &program.statements[0]
    else {
      panic!("expected optimized closure");
    };
    let Callable::Closure(closure) = &**callable else {
      panic!("expected closure");
    };
    let mut lerps = 0;
    let mut unions = 0;
    for statement in &closure.body.0 {
      statement.traverse_exprs(&mut |expr| {
        let Expr::Call { call, .. } = expr else {
          return;
        };
        let FunctionCallTarget::Literal(callable) = &call.target else {
          return;
        };
        let Callable::Builtin {
          fn_entry_ix,
          pre_resolved_signature,
          ..
        } = &**callable
        else {
          return;
        };
        match fn_sigs().entries[*fn_entry_ix].0 {
          "lerp_paths" => {
            lerps += 1;
            assert!(
              pre_resolved_signature.is_none(),
              "indexed inputs still need validation"
            );
          }
          "path_union" => {
            unions += 1;
            assert!(
              pre_resolved_signature.is_some(),
              "known path intermediates establish the overload"
            );
          }
          _ => {}
        }
      });
    }
    assert_eq!((lerps, unions), (2, 1));
  }
}
