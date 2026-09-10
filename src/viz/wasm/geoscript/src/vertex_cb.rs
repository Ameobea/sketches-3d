//! Bag-param idiom for per-vertex callbacks. After a builtin's fixed positional params a closure
//! may declare one map-destructured bag naming the per-vertex data it reads, e.g.
//! `|pos, normal, {color, uv, ix}|`. The bag is resolved to frame slots once per builtin call so
//! each per-vertex invoke writes values straight into the closure frame: no map is built, and
//! closures without a bag pay nothing extra.

use std::rc::Rc;

use fxhash::FxHashMap;

use mesh::{
  linked_mesh::{Channel, ChannelStore, Vec3, VertexKey},
  LinkedMesh,
};
use nalgebra::{Vector2, Vector4};
use smallvec::SmallVec;

use crate::{ast::DestructurePattern, Callable, ErrorStack, EvalCtx, Value, EMPTY_KWARGS};

pub(crate) const IX_KEY: &str = "ix";

enum Source {
  Fixed(usize),
  Ix,
  Attr(String),
}

struct Plan {
  n_fixed: usize,
  bag: Vec<(usize, Source)>,
  ix_of: Option<FxHashMap<VertexKey, u32>>,
}

pub(crate) struct VertexCb<'a> {
  callable: &'a Rc<Callable>,
  /// `None` for non-closure callables, which get the plain positional invoke and no bag.
  plan: Option<Plan>,
}

impl<'a> VertexCb<'a> {
  /// `fixed` names the builtin's positional params in order; they double as bag keys.
  pub fn new(
    ctx: &EvalCtx,
    builtin: &str,
    callable: &'a Rc<Callable>,
    fixed: &[&str],
    mesh: &LinkedMesh<()>,
  ) -> Result<Self, ErrorStack> {
    let Callable::Closure(closure) = &**callable else {
      return Ok(Self {
        callable,
        plan: None,
      });
    };
    let params = &closure.params;
    let hint = || {
      let mut attrs: Vec<&str> = mesh.vertex_channels.keys().map(String::as_str).collect();
      attrs.sort_unstable();
      let example: Vec<&str> = attrs.iter().take(1).chain([&IX_KEY]).copied().collect();
      let fixed = fixed.join(", ");
      format!(
        "`{builtin}` callbacks take `|{fixed}|` plus an optional destructured attribute bag, e.g. \
         `|{fixed}, {{{}}}|`.  Available bag keys: {fixed}, {IX_KEY}{}{}",
        example.join(", "),
        if attrs.is_empty() { "" } else { ", " },
        attrs.join(", ")
      )
    };
    if params.len() > fixed.len() + 1 {
      return Err(ErrorStack::new(format!(
        "Too many callback parameters.  {}",
        hint()
      )));
    }
    let mut plan = Plan {
      n_fixed: params.len().min(fixed.len()),
      bag: Vec::new(),
      ix_of: None,
    };
    if params.len() == fixed.len() + 1 {
      let DestructurePattern::Map(pat) = &params[fixed.len()].ident else {
        return Err(ErrorStack::new(format!(
          "Callback parameter {} is the attribute bag and must be destructured.  {}",
          fixed.len() + 1,
          hint()
        )));
      };
      let slot0 = closure.resolved.param_slots[fixed.len()] as usize;
      // Slot order follows the resolver's `visit_idents` walk over this same map instance.
      for (off, (key, leaf)) in pat.iter().enumerate() {
        let name = ctx.with_resolved_sym(*key, |s| s.to_owned());
        if !matches!(leaf, DestructurePattern::Ident(..)) {
          return Err(ErrorStack::new(format!(
            "Attribute bag key `{name}` must bind to a plain name, not a nested pattern"
          )));
        }
        let src = if let Some(i) = fixed.iter().position(|f| *f == name) {
          Source::Fixed(i)
        } else if name == IX_KEY {
          plan
            .ix_of
            .get_or_insert_with(|| mesh.vertices.keys().zip(0u32..).collect());
          Source::Ix
        } else if mesh.vertex_channels.contains_key(&name) {
          Source::Attr(name)
        } else {
          return Err(ErrorStack::new(format!(
            "Unknown attribute bag key `{name}`.  {}",
            hint()
          )));
        };
        plan.bag.push((slot0 + off, src));
      }
    }
    Ok(Self {
      callable,
      plan: Some(plan),
    })
  }

  /// Whether the callback can observe fixed param `i`, so callers can skip computing unused inputs.
  pub fn reads_fixed(&self, i: usize) -> bool {
    match &self.plan {
      None => true,
      Some(p) => {
        p.n_fixed > i
          || p
            .bag
            .iter()
            .any(|(_, s)| matches!(s, Source::Fixed(j) if *j == i))
      }
    }
  }

  /// `fixed` must hold every fixed param's value; bag attrs are read from `attrs_from` at `key`.
  pub fn invoke(
    &self,
    ctx: &EvalCtx,
    fixed: &[Value],
    attrs_from: &LinkedMesh<()>,
    key: VertexKey,
  ) -> Result<Value, ErrorStack> {
    let Some(plan) = &self.plan else {
      return ctx.invoke_callable(self.callable, fixed, EMPTY_KWARGS);
    };
    let Callable::Closure(closure) = &**self.callable else {
      unreachable!()
    };
    let mut prefilled: SmallVec<[(usize, Value); 4]> = SmallVec::new();
    for (slot, src) in &plan.bag {
      let v = match src {
        Source::Fixed(i) => fixed[*i].clone(),
        Source::Ix => Value::Int(plan.ix_of.as_ref().unwrap()[&key] as i64),
        Source::Attr(name) => attr_value(&attrs_from.vertex_channels[name], key),
      };
      prefilled.push((*slot, v));
    }
    ctx.invoke_closure_prefilled(self.callable, closure, &fixed[..plan.n_fixed], prefilled)
  }
}

/// A channel slot as a script value; `Nil` where the channel has no entry for `key`.
pub(crate) fn attr_value(ch: &Channel<VertexKey>, key: VertexKey) -> Value {
  match &ch.store {
    ChannelStore::Scalar(m) => m.get(key).map_or(Value::Nil, |&v| Value::Float(v)),
    ChannelStore::Vec2(m) => m
      .get(key)
      .map_or(Value::Nil, |&[a, b]| Value::Vec2(Vector2::new(a, b))),
    ChannelStore::Vec3(m) => m
      .get(key)
      .map_or(Value::Nil, |&[a, b, c]| Value::Vec3(Vec3::new(a, b, c))),
    ChannelStore::Vec4(m) => m
      .get(key)
      .map_or(Value::Nil, |&v| Value::Vec4(Rc::new(Vector4::from(v)))),
  }
}
