use std::rc::Rc;

use fxhash::FxHashMap;

use crate::{
  map_key_from_value,
  seq::{EagerSeq, MapIterMode, MapIterSeq},
  seq_as_eager, ArgRef, ErrorStack, EvalCtx, Sym, Value, EMPTY_KWARGS,
};

fn as_map_rc(val: &Value) -> Rc<FxHashMap<String, Value>> {
  match val {
    Value::Map(map) => Rc::clone(map),
    _ => unreachable!(),
  }
}

pub(crate) fn map_iter_impl<const MODE: MapIterMode>(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let map = as_map_rc(arg_refs[0].resolve(args, kwargs));
  Ok(Value::Sequence(Rc::new(MapIterSeq::<MODE> { map })))
}

pub(crate) fn from_entries_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let seq = arg_refs[0].resolve(args, kwargs).as_sequence().unwrap();
  let mut map = FxHashMap::default();
  for (i, res) in seq.consume(ctx).enumerate() {
    let entry =
      res.map_err(|err| err.wrap("Error evaluating sequence passed to `from_entries`"))?;
    let Some(entry_seq) = entry.as_sequence() else {
      return Err(ErrorStack::new(format!(
        "Element {i} passed to `from_entries` has type {:?}; expected [key, value] pairs",
        entry.get_type()
      )));
    };
    let pair: Vec<Value> = if let Some(eager) = seq_as_eager(&*entry_seq) {
      (*eager.inner).clone()
    } else {
      entry_seq
        .consume(ctx)
        .collect::<Result<_, _>>()
        .map_err(|err| {
          err.wrap(format!(
            "Error evaluating entry {i} passed to `from_entries`"
          ))
        })?
    };
    if pair.len() != 2 {
      return Err(ErrorStack::new(format!(
        "Element {i} passed to `from_entries` has {} elements; expected [key, value] pairs",
        pair.len()
      )));
    }
    let key = map_key_from_value(&pair[0]).map_err(|err| {
      err.wrap(format!(
        "Invalid key in element {i} passed to `from_entries`"
      ))
    })?;
    map.insert(key, pair[1].clone());
  }
  Ok(Value::Map(Rc::new(map)))
}

pub(crate) fn has_impl(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let key = map_key_from_value(arg_refs[0].resolve(args, kwargs))?;
  let map = as_map_rc(arg_refs[1].resolve(args, kwargs));
  Ok(Value::Bool(map.contains_key(&key)))
}

pub(crate) fn group_by_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let cb = arg_refs[0].resolve(args, kwargs).as_callable().unwrap();
  let seq = arg_refs[1].resolve(args, kwargs).as_sequence().unwrap();
  let mut groups: FxHashMap<String, Vec<Value>> = FxHashMap::default();
  for (i, res) in seq.consume(ctx).enumerate() {
    let val = res.map_err(|err| err.wrap("Error evaluating sequence passed to `group_by`"))?;
    let key_val = ctx
      .invoke_callable(cb, &[val.clone(), Value::Int(i as i64)], EMPTY_KWARGS)
      .map_err(|err| err.wrap("Error in user-provided callback to `group_by`"))?;
    let key = map_key_from_value(&key_val)
      .map_err(|err| err.wrap("Invalid key produced by `group_by` callback"))?;
    groups.entry(key).or_default().push(val);
  }
  Ok(Value::Map(Rc::new(
    groups
      .into_iter()
      .map(|(key, vals)| {
        (
          key,
          Value::Sequence(Rc::new(EagerSeq {
            inner: Rc::new(vals),
          })),
        )
      })
      .collect(),
  )))
}

fn collect_path(ctx: &EvalCtx, path: &Value) -> Result<Vec<String>, ErrorStack> {
  let seq = path.as_sequence().unwrap();
  let mut out = Vec::new();
  for (i, res) in seq.consume(ctx).enumerate() {
    let val = res.map_err(|err| err.wrap("Error evaluating path sequence"))?;
    out.push(
      map_key_from_value(&val)
        .map_err(|err| err.wrap(format!("Invalid path element at index {i}")))?,
    );
  }
  Ok(out)
}

fn update_path(
  cur: Option<&Value>,
  path: &[String],
  leaf: &mut dyn FnMut(Option<&Value>) -> Result<Value, ErrorStack>,
) -> Result<Value, ErrorStack> {
  let Some((key, rest)) = path.split_first() else {
    return leaf(cur);
  };
  let mut map = match cur {
    None | Some(Value::Nil) => FxHashMap::default(),
    Some(Value::Map(map)) => (**map).clone(),
    Some(other) => {
      return Err(ErrorStack::new(format!(
        "Cannot descend into path segment `{key}`; parent has type {:?}, expected map",
        other.get_type()
      )))
    }
  };
  let new_child = update_path(map.get(key.as_str()), rest, leaf)?;
  map.insert(key.clone(), new_child);
  Ok(Value::Map(Rc::new(map)))
}

pub(crate) fn get_in_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let path = collect_path(ctx, arg_refs[0].resolve(args, kwargs))?;
  let mut cur = arg_refs[1].resolve(args, kwargs).clone();
  let default = arg_refs[2].resolve(args, kwargs);
  for key in &path {
    let next = match &cur {
      Value::Map(map) => map.get(key).cloned(),
      _ => None,
    };
    match next {
      Some(val) => cur = val,
      None => return Ok(default.clone()),
    }
  }
  if cur.is_nil() {
    return Ok(default.clone());
  }
  Ok(cur)
}

pub(crate) fn set_in_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let path = collect_path(ctx, arg_refs[0].resolve(args, kwargs))?;
  let val = arg_refs[1].resolve(args, kwargs).clone();
  let map = arg_refs[2].resolve(args, kwargs);
  update_path(Some(map), &path, &mut |_| Ok(val.clone()))
}

pub(crate) fn update_in_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let path = collect_path(ctx, arg_refs[0].resolve(args, kwargs))?;
  let cb = arg_refs[1].resolve(args, kwargs).as_callable().unwrap();
  let map = arg_refs[2].resolve(args, kwargs);
  let default = arg_refs[3].resolve(args, kwargs);
  update_path(Some(map), &path, &mut |cur| {
    let cur = match cur {
      None | Some(Value::Nil) => default.clone(),
      Some(val) => val.clone(),
    };
    ctx
      .invoke_callable(cb, &[cur], EMPTY_KWARGS)
      .map_err(|err| err.wrap("Error in user-provided callback to `update_in`"))
  })
}
