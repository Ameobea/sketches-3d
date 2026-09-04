use geoscript::{
  builtins::fn_defs::{fn_sigs, ArgDef, DefaultValue, FnDef, FnSignature},
  ty::PartialApplication,
  ArgType, EvalCtx, Sym, Value,
};
use nanoserde::SerJson;

/// Presentation form of a builtin's signatures, shared by hover and signature help.
#[derive(Clone, Debug, SerJson)]
pub struct BuiltinDocs {
  pub name: String,
  pub module: String,
  pub signatures: Vec<SignatureDocs>,
}

#[derive(Clone, Debug, SerJson)]
pub struct SignatureDocs {
  pub params: Vec<ParamDocs>,
  pub description: String,
  pub return_type: String,
}

#[derive(Clone, Debug, SerJson)]
pub struct ParamDocs {
  pub name: String,
  pub ty: String,
  pub default: Option<String>,
  pub description: String,
}

pub(crate) fn format_arg_type(arg: &ArgDef) -> String {
  let types = ArgType::list_from_bitflags(arg.valid_types);
  types
    .iter()
    .map(|t| t.as_str())
    .collect::<Vec<_>>()
    .join(" | ")
}

fn format_return_type(rt: &[ArgType]) -> String {
  rt.iter()
    .map(|t| t.as_str())
    .collect::<Vec<_>>()
    .join(" | ")
}

fn format_float(f: f32) -> String {
  if f.is_finite() && f.fract() == 0. {
    format!("{f:.1}")
  } else {
    f.to_string()
  }
}

fn format_default_value(v: &Value) -> String {
  match v {
    Value::Nil => "nil".to_owned(),
    Value::Int(i) => i.to_string(),
    Value::Float(f) => format_float(*f),
    Value::Bool(b) => b.to_string(),
    Value::String(s) => format!("{s:?}"),
    Value::Vec2(v) => format!("vec2({}, {})", format_float(v.x), format_float(v.y)),
    Value::Vec3(v) => format!(
      "vec3({}, {}, {})",
      format_float(v.x),
      format_float(v.y),
      format_float(v.z)
    ),
    Value::Map(map) => {
      let entries: Vec<String> = map
        .iter()
        .map(|(k, v)| format!("{k}: {}", format_default_value(v)))
        .collect();
      format!("{{{}}}", entries.join(", "))
    }
    other => format!("{other:?}"),
  }
}

fn format_default(arg: &ArgDef) -> Option<String> {
  match &arg.default_value {
    DefaultValue::Required => None,
    DefaultValue::Optional(get_default) => Some(format_default_value(&get_default())),
  }
}

pub fn format_signature_oneliner(name: &str, sig: &FnSignature) -> String {
  let args: Vec<String> = sig.arg_defs.iter().map(format_arg_oneliner).collect();
  format!("{}({})", name, args.join(", "))
}

pub fn builtin_docs(name: &str, fn_def: &FnDef) -> BuiltinDocs {
  BuiltinDocs {
    name: name.to_owned(),
    module: fn_def.module.to_owned(),
    signatures: fn_def
      .signatures
      .iter()
      .map(|sig| SignatureDocs {
        params: sig
          .arg_defs
          .iter()
          .map(|arg| ParamDocs {
            name: arg.name.to_owned(),
            ty: format_arg_type(arg),
            default: format_default(arg),
            description: arg.description.to_owned(),
          })
          .collect(),
        description: sig.description.to_owned(),
        return_type: format_return_type(sig.return_type),
      })
      .collect(),
  }
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
  match format_default(arg) {
    None => format!("{}: {type_str}", arg.name),
    Some(default) => format!("{}: {type_str} = {default}", arg.name),
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
      format!(
        "\nOverload {} of {} — remaining:",
        ix + 1,
        def.signatures.len()
      )
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
