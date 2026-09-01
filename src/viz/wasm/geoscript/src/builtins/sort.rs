use std::{cell::Cell, cmp::Ordering, rc::Rc};

use fxhash::FxHashMap;

use crate::{
  seq::EagerSeq, ArgRef, Callable, ErrorStack, EvalCtx, Sequence, Sym, Value, EMPTY_KWARGS,
};

enum SortKey {
  Int(i64),
  Float(f64),
  Bool(bool),
  Str(String),
  List(Vec<SortKey>),
}

type Mismatch = Cell<Option<(&'static str, &'static str)>>;

impl SortKey {
  fn from_value(ctx: &EvalCtx, val: &Value) -> Result<Self, ErrorStack> {
    Ok(match val {
      Value::Int(i) => SortKey::Int(*i),
      Value::Float(f) => SortKey::Float(*f as f64),
      Value::Bool(b) => SortKey::Bool(*b),
      Value::String(s) => SortKey::Str(s.clone()),
      Value::Sequence(seq) => SortKey::List(
        seq
          .consume(ctx)
          .map(|res| Self::from_value(ctx, &res?))
          .collect::<Result<_, _>>()?,
      ),
      other => {
        return Err(ErrorStack::new(format!(
          "Sort keys must be int, float, string, bool, or a list of those; got {:?}",
          other.get_type()
        )))
      }
    })
  }

  fn kind(&self) -> (&'static str, u8) {
    match self {
      SortKey::Int(_) | SortKey::Float(_) => ("number", 0),
      SortKey::Bool(_) => ("bool", 1),
      SortKey::Str(_) => ("string", 2),
      SortKey::List(_) => ("list", 3),
    }
  }

  // Rust's sorts require a total order and can't propagate errors, so a kind mismatch
  // orders by kind rank and is recorded to be raised once the sort finishes.
  fn cmp(&self, other: &Self, mismatch: &Mismatch) -> Ordering {
    use SortKey::*;
    match (self, other) {
      (Int(a), Int(b)) => a.cmp(b),
      (Int(a), Float(b)) => (*a as f64).total_cmp(b),
      (Float(a), Int(b)) => a.total_cmp(&(*b as f64)),
      (Float(a), Float(b)) => a.total_cmp(b),
      (Bool(a), Bool(b)) => a.cmp(b),
      (Str(a), Str(b)) => a.cmp(b),
      (List(a), List(b)) => a
        .iter()
        .zip(b)
        .map(|(x, y)| x.cmp(y, mismatch))
        .find(|ord| ord.is_ne())
        .unwrap_or_else(|| a.len().cmp(&b.len())),
      _ => {
        let (ka, ra) = self.kind();
        let (kb, rb) = other.kind();
        if mismatch.get().is_none() {
          mismatch.set(Some((ka, kb)));
        }
        ra.cmp(&rb)
      }
    }
  }
}

fn check_mismatch(mismatch: &Mismatch, fn_name: &str) -> Result<(), ErrorStack> {
  match mismatch.get() {
    Some((a, b)) => Err(ErrorStack::new(format!(
      "`{fn_name}` keys must all be the same kind; found both {a} and {b}"
    ))),
    None => Ok(()),
  }
}

fn keyed<'a>(
  ctx: &'a EvalCtx,
  by: Option<&'a Rc<Callable>>,
  seq: Rc<dyn Sequence>,
  fn_name: &'a str,
) -> impl Iterator<Item = Result<(SortKey, Value), ErrorStack>> + 'a {
  seq.consume(ctx).enumerate().map(move |(i, res)| {
    let val =
      res.map_err(|err| err.wrap(format!("Error evaluating sequence passed to `{fn_name}`")))?;
    let key_val = match by {
      Some(cb) => Some(
        ctx
          .invoke_callable(cb, &[val.clone(), Value::Int(i as i64)], EMPTY_KWARGS)
          .map_err(|err| err.wrap(format!("Error in `by` callback passed to `{fn_name}`")))?,
      ),
      None => None,
    };
    let key = SortKey::from_value(ctx, key_val.as_ref().unwrap_or(&val))
      .map_err(|err| err.wrap(format!("Invalid `{fn_name}` key at index {i}")))?;
    Ok((key, val))
  })
}

pub(crate) fn sort_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let seq = arg_refs[0].resolve(args, kwargs).as_sequence().unwrap();
  let by = arg_refs[1].resolve(args, kwargs).as_callable();
  let stable = arg_refs[2].resolve(args, kwargs).as_bool().unwrap();
  let desc = arg_refs[3].resolve(args, kwargs).as_bool().unwrap();

  let mut keyed: Vec<(SortKey, Value)> = keyed(ctx, by, seq, "sort").collect::<Result<_, _>>()?;
  let mismatch = Mismatch::default();
  let cmp = |a: &(SortKey, Value), b: &(SortKey, Value)| {
    let ord = a.0.cmp(&b.0, &mismatch);
    if desc {
      ord.reverse()
    } else {
      ord
    }
  };
  if stable {
    keyed.sort_by(cmp);
  } else {
    keyed.sort_unstable_by(cmp);
  }
  check_mismatch(&mismatch, "sort")?;

  Ok(Value::Sequence(Rc::new(EagerSeq {
    inner: Rc::new(keyed.into_iter().map(|(_, val)| val).collect()),
  })))
}

pub(crate) fn min_max_seq_impl<const MAX: bool>(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let fn_name = if MAX { "max" } else { "min" };
  let seq = arg_refs[0].resolve(args, kwargs).as_sequence().unwrap();
  let by = arg_refs[1].resolve(args, kwargs).as_callable();
  let wanted = if MAX {
    Ordering::Greater
  } else {
    Ordering::Less
  };

  let mismatch = Mismatch::default();
  let mut best: Option<(SortKey, Value)> = None;
  for res in keyed(ctx, by, seq, fn_name) {
    let (key, val) = res?;
    let better = match &best {
      None => true,
      Some((best_key, _)) => key.cmp(best_key, &mismatch) == wanted,
    };
    if better {
      best = Some((key, val));
    }
  }
  check_mismatch(&mismatch, fn_name)?;
  Ok(best.map_or(Value::Nil, |(_, val)| val))
}
