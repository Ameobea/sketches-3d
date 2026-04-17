use geoscript::{
  builtins::fn_defs::{fn_sigs, ArgDef, DefaultValue, FnDef, FnSignature},
  ty::PartialApplication,
  ArgType, EvalCtx, Sym,
};

fn format_arg_type(arg: &ArgDef) -> String {
  let types = ArgType::list_from_bitflags(arg.valid_types);
  types
    .iter()
    .map(|t| t.as_str())
    .collect::<Vec<_>>()
    .join(" | ")
}

fn format_return_type(rt: &[ArgType]) -> String {
  rt.iter().map(|t| t.as_str()).collect::<Vec<_>>().join(" | ")
}

pub fn format_signature_oneliner(name: &str, sig: &FnSignature) -> String {
  let args: Vec<String> = sig
    .arg_defs
    .iter()
    .map(|arg| {
      let type_str = format_arg_type(arg);
      match &arg.default_value {
        DefaultValue::Required => {
          format!("{}: {type_str}", arg.name)
        }
        DefaultValue::Optional(get_default) => {
          format!("{}: {type_str} = {:?}", arg.name, get_default())
        }
      }
    })
    .collect();
  format!("{}({})", name, args.join(", "))
}

pub fn format_builtin_hover(name: &str, fn_def: &FnDef) -> String {
  let mut parts = Vec::new();
  parts.push(format!("(builtin) **{}**", name));
  if !fn_def.module.is_empty() {
    parts.push(format!("Module: {}", fn_def.module));
  }

  for (i, sig) in fn_def.signatures.iter().enumerate() {
    if fn_def.signatures.len() > 1 {
      parts.push(format!("\nOverload {}:", i + 1));
    }
    parts.push(format!("`{}`", format_signature_oneliner(name, sig)));
    if !sig.description.is_empty() {
      parts.push(sig.description.to_string());
    }
  }

  parts.join("\n")
}

/// Format hover content for a builtin call when the active overload is known.  Shows just the
/// matched signature with detailed per-argument documentation, plus an overload-N-of-M hint
/// when the function has more than one overload.
pub fn format_builtin_hover_with_sig(name: &str, fn_def: &FnDef, sig_ix: usize) -> String {
  let Some(sig) = fn_def.signatures.get(sig_ix) else {
    return format_builtin_hover(name, fn_def);
  };

  let mut parts = Vec::new();
  parts.push(format!("(builtin) **{}**", name));
  if !fn_def.module.is_empty() {
    parts.push(format!("Module: {}", fn_def.module));
  }
  let total = fn_def.signatures.len();
  if total > 1 {
    parts.push(format!("Overload {} of {}:", sig_ix + 1, total));
  }

  let return_str = format_return_type(sig.return_type);
  if return_str.is_empty() {
    parts.push(format!("`{}`", format_signature_oneliner(name, sig)));
  } else {
    parts.push(format!(
      "`{} → {return_str}`",
      format_signature_oneliner(name, sig)
    ));
  }

  if !sig.description.is_empty() {
    parts.push(sig.description.to_string());
  }

  if !sig.arg_defs.is_empty() {
    parts.push("\n**Arguments:**".to_string());
    for arg in sig.arg_defs {
      let type_str = format_arg_type(arg);
      let default_str = match &arg.default_value {
        DefaultValue::Required => String::new(),
        DefaultValue::Optional(get_default) => format!(" = `{:?}`", get_default()),
      };
      let mut line = format!("- **{}**: `{type_str}`{default_str}", arg.name);
      if !arg.description.is_empty() {
        line.push_str(" — ");
        line.push_str(arg.description);
      }
      parts.push(line);
    }
  }

  parts.join("\n")
}

/// Walk a signature's arg defs and split them into (bound, remaining) by replaying the PAF's
/// positional args + bound kwargs against the sig.  Returns None if the bound args don't fit
/// this signature even as a prefix.  When some kwargs name later positional params, those are
/// reported as bound by name.
fn classify_sig_for_paf<'a>(
  sig: &'a FnSignature,
  paf: &PartialApplication,
) -> Option<(Vec<&'a ArgDef>, Vec<&'a ArgDef>)> {
  if paf.bound_args.len() > sig.arg_defs.len() {
    return None;
  }
  for (i, ty) in paf.bound_args.iter().enumerate() {
    if sig.arg_defs[i].valid_types & ty.as_bitflags() == 0 {
      return None;
    }
  }
  for (k, kty) in &paf.bound_kwargs {
    let arg_def = sig.arg_defs.iter().find(|d| d.interned_name == *k);
    match arg_def {
      Some(d) if d.valid_types & kty.as_bitflags() != 0 => {}
      _ => return None,
    }
  }

  let mut bound: Vec<&ArgDef> = Vec::new();
  let mut remaining: Vec<&ArgDef> = Vec::new();
  for (i, def) in sig.arg_defs.iter().enumerate() {
    let bound_by_pos = i < paf.bound_args.len();
    let bound_by_kw = paf
      .bound_kwargs
      .iter()
      .any(|(k, _)| *k == def.interned_name);
    if bound_by_pos || bound_by_kw {
      bound.push(def);
    } else {
      remaining.push(def);
    }
  }
  Some((bound, remaining))
}

fn format_arg_oneliner(arg: &ArgDef) -> String {
  let type_str = format_arg_type(arg);
  match &arg.default_value {
    DefaultValue::Required => format!("{}: {type_str}", arg.name),
    DefaultValue::Optional(get_default) => {
      format!("{}: {type_str} = {:?}", arg.name, get_default())
    }
  }
}

/// Format hover content for a value of `PartiallyApplied` type.  Shows the underlying builtin
/// name, which args have been bound, and which signatures the call could still complete into
/// (each with its remaining params).
pub fn format_partial_application(paf: &PartialApplication, ctx: &EvalCtx) -> String {
  let mut parts = Vec::new();
  parts.push(format!("(partial application of `{}`)", paf.name));

  if let Some(def) = fn_sigs().get(paf.name.as_str()) {
    if !def.module.is_empty() {
      parts.push(format!("Module: {}", def.module));
    }
  }

  let bound_pos_str = if paf.bound_args.is_empty() {
    None
  } else {
    Some(
      paf
        .bound_args
        .iter()
        .map(|t| format!("`{}`", t.as_str()))
        .collect::<Vec<_>>()
        .join(", "),
    )
  };
  let bound_kw_str = if paf.bound_kwargs.is_empty() {
    None
  } else {
    Some(
      paf
        .bound_kwargs
        .iter()
        .map(|(sym, t)| {
          let name = resolve_sym(ctx, *sym);
          format!("{name}=`{}`", t.as_str())
        })
        .collect::<Vec<_>>()
        .join(", "),
    )
  };
  let mut bound_parts = Vec::new();
  if let Some(s) = bound_pos_str {
    bound_parts.push(s);
  }
  if let Some(s) = bound_kw_str {
    bound_parts.push(s);
  }
  if !bound_parts.is_empty() {
    parts.push(format!("Bound: {}", bound_parts.join(", ")));
  }

  // Find which signatures still accept the bound args.  Show remaining params for each.
  let Some(def) = fn_sigs().get(paf.name.as_str()) else {
    return parts.join("\n");
  };

  let candidates: Vec<(usize, Vec<&ArgDef>)> = def
    .signatures
    .iter()
    .enumerate()
    .filter_map(|(ix, sig)| classify_sig_for_paf(sig, paf).map(|(_b, r)| (ix, r)))
    .collect();

  if candidates.is_empty() {
    return parts.join("\n");
  }

  let multi = def.signatures.len() > 1;
  for (ix, remaining) in &candidates {
    let header = if multi {
      format!("\nOverload {} of {} — remaining:", ix + 1, def.signatures.len())
    } else {
      "\nRemaining params:".to_string()
    };
    parts.push(header);
    if remaining.is_empty() {
      parts.push("- (all bound — call to invoke)".to_string());
    } else {
      for arg in remaining {
        let mut line = format!("- `{}`", format_arg_oneliner(arg));
        if !arg.description.is_empty() {
          line.push_str(" — ");
          line.push_str(arg.description);
        }
        parts.push(line);
      }
    }
    let return_str = format_return_type(def.signatures[*ix].return_type);
    if !return_str.is_empty() {
      parts.push(format!("Returns: `{return_str}`"));
    }
  }

  parts.join("\n")
}

fn resolve_sym(ctx: &EvalCtx, sym: Sym) -> String {
  ctx
    .interned_symbols
    .with_resolved(sym, |s| s.to_string())
    .unwrap_or_else(|| "?".to_string())
}
