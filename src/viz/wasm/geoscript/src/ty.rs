use crate::{ArgType, Sym};

/// A callable with captured arguments. Reapplication appends positional arguments and replaces
/// captured keyword arguments by name, matching runtime partial application.
#[derive(Clone, Debug)]
pub struct PartialApplication {
  pub target: Box<AbstractType>,
  pub bound_args: Vec<AbstractType>,
  pub bound_kwargs: Vec<(Sym, AbstractType)>,
}

/// One parameter of a user-defined callable (typically a closure).  Captures the declared
/// (or inferred) type of the param plus an optional display name for hover rendering.
#[derive(Clone, Debug)]
pub struct CallableParam {
  pub name: Option<String>,
  pub symbol: Option<Sym>,
  pub has_default: bool,
  pub ty: AbstractType,
}

/// Parameter shape and available return information for a user-defined closure.
#[derive(Clone, Debug)]
pub struct CallableType {
  pub params: Vec<CallableParam>,
  pub return_type: Box<AbstractType>,
}

/// A type-level representation of a geoscript value, used during abstract interpretation.
#[derive(Clone, Debug)]
pub enum AbstractType {
  /// A single concrete type (Mesh, Vec3, Float, Int, etc.)
  Concrete(ArgType),
  /// A union of possible types (e.g. Float | Int for a function that returns either)
  Union(Vec<ArgType>),
  /// A builtin reference retaining its canonical name and overloads.
  Builtin(String),
  /// A partial application — calling/piping into it appends new args
  /// to the bound set and re-resolves against the underlying signatures.
  PartiallyApplied(PartialApplication),
  /// A user-defined callable (closure) with known param and return types.
  Callable(CallableType),
  /// A host callable with a return contract but no statically described parameter list.
  OpaqueCallable(Box<AbstractType>),
  /// Type cannot be determined
  Unknown,
}

impl AbstractType {
  /// Runtime type possibilities, including aggregate tags such as `num`. This is a set of
  /// candidates, not proof that any particular overload applies.
  pub fn possible_type_flags(&self) -> u32 {
    match self {
      Self::Concrete(ty) => ty.as_bitflags(),
      Self::Union(types) => types.iter().fold(0, |flags, ty| flags | ty.as_bitflags()),
      Self::Callable(_)
      | Self::PartiallyApplied(_)
      | Self::Builtin(_)
      | Self::OpaqueCallable(_) => ArgType::Callable.as_bitflags(),
      Self::Unknown => ArgType::Any.as_bitflags(),
    }
  }

  /// Build an AbstractType from a `FnSignature.return_type` slice.
  pub fn from_return_type(rt: &[ArgType]) -> Self {
    if rt.iter().any(|ty| matches!(ty, ArgType::Any)) {
      return Self::Unknown;
    }
    match rt.len() {
      0 => AbstractType::Unknown,
      1 => AbstractType::Concrete(rt[0]),
      _ => AbstractType::Union(rt.to_vec()),
    }
  }

  /// Extract a single runtime tag, or None for Union/Unknown.
  /// Callable values report as `ArgType::Callable` so they satisfy callable-typed param slots.
  pub fn as_single_arg_type(&self) -> Option<ArgType> {
    match self {
      AbstractType::Concrete(t) => Some(*t),
      AbstractType::Callable(_)
      | AbstractType::PartiallyApplied(_)
      | AbstractType::Builtin(_)
      | AbstractType::OpaqueCallable(_) => Some(ArgType::Callable),
      _ => None,
    }
  }

  /// Format for display in hover info.  Returns a compact one-line string; rich rendering
  /// for partial applications (with remaining-param info) is done by the hover module.
  pub fn display_str(&self) -> Option<String> {
    match self {
      AbstractType::Concrete(t) => Some(t.as_str().to_owned()),
      AbstractType::Union(types) => {
        let parts: Vec<&str> = types.iter().map(ArgType::as_str).collect();
        Some(parts.join(" | "))
      }
      AbstractType::Builtin(name) => Some(format!("builtin {name}")),
      AbstractType::OpaqueCallable(return_ty) => Some(format!(
        "fn(…) → {}",
        return_ty.display_str().unwrap_or_else(|| "?".to_owned())
      )),
      AbstractType::PartiallyApplied(paf) => {
        let bound = paf
          .bound_args
          .iter()
          .map(|t| t.display_str().unwrap_or_else(|| "?".to_owned()))
          .collect::<Vec<_>>()
          .join(", ");
        Some(format!(
          "partial {}({bound})",
          paf
            .target
            .display_str()
            .unwrap_or_else(|| "callable".to_owned())
        ))
      }
      AbstractType::Callable(ct) => {
        let params: Vec<String> = ct
          .params
          .iter()
          .map(|p| {
            let ty = p.ty.display_str().unwrap_or_else(|| "?".to_string());
            match &p.name {
              Some(n) => format!("{n}{}: {ty}", if p.has_default { "?" } else { "" }),
              None => ty,
            }
          })
          .collect();
        let ret = ct
          .return_type
          .display_str()
          .unwrap_or_else(|| "?".to_string());
        Some(format!("fn({}) → {ret}", params.join(", ")))
      }
      AbstractType::Unknown => None,
    }
  }
}

/// Merge two types into a union-ish representation.  Used for combining branch types from
/// conditionals and collecting exit types from closure bodies.
///
/// Rules:
/// - Any op involving Unknown → Unknown (absorbing).
/// - Concrete + same Concrete → that Concrete.
/// - Concrete + different Concrete → Union of both.
/// - Union + Concrete → Union with the Concrete added (dedup).
/// - Union + Union → Union of both (dedup).
/// - Structured callable types lose signature details when joined, retaining the callable tag.
pub fn merge_types(a: &AbstractType, b: &AbstractType) -> AbstractType {
  /// ArgType doesn't impl PartialEq, but its bitflag is unique per variant.
  fn same(x: ArgType, y: ArgType) -> bool {
    x.as_bitflags() == y.as_bitflags()
  }
  fn push_unique(v: &mut Vec<ArgType>, t: ArgType) {
    if !v.iter().any(|u| same(*u, t)) {
      v.push(t);
    }
  }

  match (a, b) {
    (AbstractType::Unknown, _) | (_, AbstractType::Unknown) => AbstractType::Unknown,
    (AbstractType::Concrete(x), AbstractType::Concrete(y)) if same(*x, *y) => {
      AbstractType::Concrete(*x)
    }
    (AbstractType::Concrete(x), AbstractType::Concrete(y)) => AbstractType::Union(vec![*x, *y]),
    (AbstractType::Union(xs), AbstractType::Concrete(y))
    | (AbstractType::Concrete(y), AbstractType::Union(xs)) => {
      let mut combined = xs.clone();
      push_unique(&mut combined, *y);
      match combined.len() {
        1 => AbstractType::Concrete(combined[0]),
        _ => AbstractType::Union(combined),
      }
    }
    (AbstractType::Union(xs), AbstractType::Union(ys)) => {
      let mut combined = xs.clone();
      for y in ys {
        push_unique(&mut combined, *y);
      }
      match combined.len() {
        1 => AbstractType::Concrete(combined[0]),
        _ => AbstractType::Union(combined),
      }
    }
    // Structured callable unions retain the runtime callable tag; their signatures are unknown.
    (
      AbstractType::Builtin(_)
      | AbstractType::Callable(_)
      | AbstractType::PartiallyApplied(_)
      | AbstractType::OpaqueCallable(_),
      other,
    ) => merge_types(&AbstractType::Concrete(ArgType::Callable), other),
    (
      other,
      AbstractType::Builtin(_)
      | AbstractType::Callable(_)
      | AbstractType::PartiallyApplied(_)
      | AbstractType::OpaqueCallable(_),
    ) => merge_types(other, &AbstractType::Concrete(ArgType::Callable)),
  }
}
