use crate::{ArgType, Sym};

/// A partial application of a builtin function: bound positional and keyword args, plus the
/// canonical builtin name needed to look up the underlying signature list.
#[derive(Clone, Debug)]
pub struct PartialApplication {
  /// Canonical builtin name (aliases already resolved).
  pub name: String,
  /// Positional args already bound, in order.
  pub bound_args: Vec<ArgType>,
  /// Keyword args already bound (interned name → type).
  pub bound_kwargs: Vec<(Sym, ArgType)>,
}

/// One parameter of a user-defined callable (typically a closure).  Captures the declared
/// (or inferred) type of the param plus an optional display name for hover rendering.
#[derive(Clone, Debug)]
pub struct CallableParam {
  pub name: Option<String>,
  pub ty: AbstractType,
}

/// The type of a user-defined callable (e.g. a closure).  Separate from `PartialApplication`
/// because a closure has fully-known param & return types, not a bound-prefix-of-builtin shape.
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
  /// A partial application of a builtin function — calling/piping into it appends new args
  /// to the bound set and re-resolves against the underlying signatures.
  PartiallyApplied(PartialApplication),
  /// A user-defined callable (closure) with known param and return types.
  Callable(CallableType),
  /// Type cannot be determined
  Unknown,
}

impl AbstractType {
  /// Build an AbstractType from a `FnSignature.return_type` slice.
  pub fn from_return_type(rt: &[ArgType]) -> Self {
    // Filter out `Any` since it carries no useful type information
    let filtered: Vec<ArgType> = rt
      .iter()
      .copied()
      .filter(|t| !matches!(t, ArgType::Any))
      .collect();
    match filtered.len() {
      0 => AbstractType::Unknown,
      1 => AbstractType::Concrete(filtered[0]),
      _ => AbstractType::Union(filtered),
    }
  }

  /// Extract a single concrete ArgType, or None for Union/Unknown/PartiallyApplied.
  /// Callable values report as `ArgType::Callable` so they satisfy callable-typed param slots.
  pub fn as_single_arg_type(&self) -> Option<ArgType> {
    match self {
      AbstractType::Concrete(t) => Some(*t),
      AbstractType::Callable(_) => Some(ArgType::Callable),
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
      AbstractType::PartiallyApplied(paf) => {
        let bound = paf
          .bound_args
          .iter()
          .map(|t| t.as_str())
          .collect::<Vec<_>>()
          .join(", ");
        Some(format!("partial {}({bound})", paf.name))
      }
      AbstractType::Callable(ct) => {
        let params: Vec<String> = ct
          .params
          .iter()
          .map(|p| {
            let ty = p.ty.display_str().unwrap_or_else(|| "?".to_string());
            match &p.name {
              Some(n) => format!("{n}: {ty}"),
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
/// - Callable/PartiallyApplied merged with anything else collapses to Concrete(Callable)
///   or Unknown (we don't support rich unions over structured types).
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
    // For structured types, fall back: if both callable, keep callable flavor; else Unknown.
    (AbstractType::Callable(_), AbstractType::Callable(_)) => {
      AbstractType::Concrete(ArgType::Callable)
    }
    _ => AbstractType::Unknown,
  }
}
