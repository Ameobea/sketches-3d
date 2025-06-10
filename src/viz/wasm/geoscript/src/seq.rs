use std::{fmt::Debug, sync::Arc};

use itertools::Itertools;
use mesh::{linked_mesh::Vec3, LinkedMesh};
use point_distribute::MeshSurfaceSampler;

use crate::{Callable, ErrorStack, EvalCtx, Sequence, Value};

#[derive(Clone, Debug)]
pub(crate) struct IntRange {
  pub start: i64,
  pub end: i64,
}

pub(crate) struct IntRangeIter {
  current: i64,
  end: i64,
}

impl Iterator for IntRangeIter {
  type Item = Result<Value, ErrorStack>;

  fn next(&mut self) -> Option<Self::Item> {
    if self.current < self.end {
      let value = Value::Int(self.current);
      self.current += 1;
      Some(Ok(value))
    } else {
      None
    }
  }
}

impl IntoIterator for IntRange {
  type Item = Result<Value, ErrorStack>;
  type IntoIter = IntRangeIter;

  fn into_iter(self) -> Self::IntoIter {
    IntRangeIter {
      current: self.start,
      end: self.end,
    }
  }
}

impl Sequence for IntRange {
  fn clone_box(&self) -> Box<dyn Sequence> {
    Box::new(self.clone())
  }

  fn consume(
    self: Box<Self>,
    _ctx: &EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>>> {
    Box::new(self.into_iter())
  }
}

#[derive(Debug)]
pub(crate) struct MapSeq {
  pub inner: Box<dyn Sequence>,
  pub cb: Callable,
}

impl Sequence for MapSeq {
  fn clone_box(&self) -> Box<dyn Sequence> {
    Box::new(Self {
      inner: self.inner.clone_box(),
      cb: self.cb.clone(),
    })
  }

  fn consume<'a>(
    self: Box<Self>,
    ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    let inner = self.inner.consume(ctx);
    // TODO: more clones here
    let cb = self.cb;
    Box::new(inner.map(move |res| {
      match res {
        Ok(v) => ctx
          .invoke_callable(&cb, &[v], &Default::default(), &ctx.globals)
          .map_err(|err| err.wrap("cb passed to map produced an error")),
        Err(e) => Err(e),
      }
    }))
  }
}

#[derive(Debug)]
pub(crate) struct FilterSeq {
  pub inner: Box<dyn Sequence>,
  pub cb: Callable,
}

impl Sequence for FilterSeq {
  fn clone_box(&self) -> Box<dyn Sequence> {
    Box::new(Self {
      inner: self.inner.clone_box(),
      cb: self.cb.clone(),
    })
  }

  fn consume<'a>(
    self: Box<Self>,
    ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    let inner = self.inner.consume(ctx);
    let cb = self.cb;

    Box::new(
      inner
        .enumerate()
        .map(move |(i, res)| match res {
          Ok(v) => {
            let flag = ctx
              .invoke_callable(
                &cb,
                &[v.clone(), Value::Int(i as i64)],
                &Default::default(),
                &ctx.globals,
              )
              .map_err(|err| err.wrap("cb passed to filter produced an error"))?;
            let Some(flag) = flag.as_bool() else {
              return Err(ErrorStack::new(format!(
                "cb passed to filter produced value which could not be interpreted as a bool: \
                 {flag:?}"
              )));
            };
            Ok(if flag { Some(v) } else { None })
          }
          Err(e) => Err(e),
        })
        .filter_map_ok(|opt| opt),
    )
  }
}

#[derive(Clone, Debug)]
pub(crate) struct EagerSeq {
  pub inner: Vec<Value>,
}

impl Sequence for EagerSeq {
  fn clone_box(&self) -> Box<dyn Sequence> {
    Box::new(self.clone())
  }

  fn consume<'a>(
    self: Box<Self>,
    _ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    Box::new(self.inner.into_iter().map(Ok))
  }
}

#[derive(Clone, Debug)]
pub(crate) struct PointDistributeSeq {
  pub mesh: Arc<LinkedMesh<()>>,
  pub point_count: usize,
}

pub(crate) struct PointDistributeIter {
  #[allow(dead_code)]
  mesh: Arc<LinkedMesh<()>>,
  sampler: MeshSurfaceSampler<'static>,
}

impl PointDistributeIter {
  pub fn new(mesh: Arc<LinkedMesh<()>>) -> Self {
    // safe because this reference will only live as long as the iterator, and by holding the `Arc`
    // internally, we can ensure that it's not dropped while the iterator is still in use.
    let static_mesh: &'static LinkedMesh<()> =
      unsafe { std::mem::transmute::<&LinkedMesh<()>, &'static LinkedMesh<()>>(&*mesh) };

    let sampler = MeshSurfaceSampler::new(static_mesh);

    Self { mesh, sampler }
  }
}

impl Iterator for PointDistributeIter {
  type Item = Result<Value, ErrorStack>;

  fn next(&mut self) -> Option<Self::Item> {
    match self.sampler.sample() {
      (pos, _normal) => {
        let value = Value::Vec3(Vec3::new(pos.x, pos.y, pos.z));
        Some(Ok(value))
      }
    }
  }
}

impl Sequence for PointDistributeSeq {
  fn clone_box(&self) -> Box<dyn Sequence> {
    Box::new(self.clone())
  }

  fn consume<'a>(
    self: Box<Self>,
    _ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    let mesh = self.mesh;

    Box::new(PointDistributeIter::new(mesh).take(self.point_count))
  }
}

/// Wrapper over an inner `Iterator` that produces `Value` items
#[derive(Clone)]
pub(crate) struct IteratorSeq<T: Iterator<Item = Result<Value, ErrorStack>> + Clone + 'static> {
  pub inner: T,
}

impl<T: Iterator<Item = Result<Value, ErrorStack>> + Clone> Debug for IteratorSeq<T> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "IteratorSeq {{ ... }}")
  }
}

impl<T: Iterator<Item = Result<Value, ErrorStack>> + Clone + 'static> Sequence for IteratorSeq<T> {
  fn clone_box(&self) -> Box<dyn Sequence> {
    Box::new(self.clone())
  }

  fn consume<'a>(
    self: Box<Self>,
    _ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    Box::new(self.inner)
  }
}

#[derive(Clone, Debug)]
pub(crate) struct MeshVertsSeq {
  pub mesh: Arc<LinkedMesh<()>>,
}

pub(crate) struct MeshVertsIter {
  #[allow(dead_code)]
  mesh: Arc<LinkedMesh<()>>,
  iter: Box<dyn Iterator<Item = Result<Value, ErrorStack>>>,
}

impl MeshVertsIter {
  pub fn new(mesh: Arc<LinkedMesh<()>>) -> Self {
    // safe because this reference will only live as long as the iterator, and by holding the `Arc`
    // internally, we can ensure that it's not dropped while the iterator is still in use.
    let static_mesh: &'static LinkedMesh<()> =
      unsafe { std::mem::transmute::<&LinkedMesh<()>, &'static LinkedMesh<()>>(&*mesh) };

    let iter: impl Iterator<Item = _> + 'static = static_mesh
      .vertices
      .values()
      .map(|vtx| Ok(Value::Vec3(vtx.position)));

    Self {
      mesh,
      iter: Box::new(iter),
    }
  }
}

impl Iterator for MeshVertsIter {
  type Item = Result<Value, ErrorStack>;

  fn next(&mut self) -> Option<Self::Item> {
    self.iter.next()
  }
}

impl Sequence for MeshVertsSeq {
  fn clone_box(&self) -> Box<dyn Sequence> {
    Box::new(self.clone())
  }

  fn consume<'a>(
    self: Box<Self>,
    _ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    Box::new(MeshVertsIter::new(Arc::clone(&self.mesh)))
  }
}

pub(crate) struct TakeSeq {
  pub inner: Box<dyn Sequence>,
  pub count: usize,
}

impl Sequence for TakeSeq {
  fn clone_box(&self) -> Box<dyn Sequence> {
    Box::new(Self {
      inner: self.inner.clone_box(),
      count: self.count,
    })
  }

  fn consume<'a>(
    self: Box<Self>,
    ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    let inner = self.inner.consume(ctx);
    Box::new(inner.take(self.count))
  }
}

impl Debug for TakeSeq {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "TakeSeq {{ count: {}, inner: {:?} }}",
      self.count, self.inner
    )
  }
}

pub(crate) struct SkipSeq {
  pub inner: Box<dyn Sequence>,
  pub count: usize,
}

impl Sequence for SkipSeq {
  fn clone_box(&self) -> Box<dyn Sequence> {
    Box::new(Self {
      inner: self.inner.clone_box(),
      count: self.count,
    })
  }

  fn consume<'a>(
    self: Box<Self>,
    ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    let inner = self.inner.consume(ctx);
    Box::new(inner.skip(self.count))
  }
}

impl Debug for SkipSeq {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "SkipSeq {{ count: {}, inner: {:?} }}",
      self.count, self.inner
    )
  }
}

pub(crate) struct TakeWhileSeq {
  pub inner: Box<dyn Sequence>,
  pub cb: Callable,
}

pub(crate) struct TakeWhileIter<'a> {
  inner: Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a>,
  cb: Callable,
  ctx: &'a EvalCtx,
}

impl Iterator for TakeWhileIter<'_> {
  type Item = Result<Value, ErrorStack>;

  fn next(&mut self) -> Option<Self::Item> {
    let Some(res) = self.inner.next() else {
      return None;
    };
    let Ok(val) = res else {
      return Some(res);
    };

    match self.ctx.invoke_callable(
      &self.cb,
      &[val.clone()],
      &Default::default(),
      &self.ctx.globals,
    ) {
      Ok(Value::Bool(flag)) => {
        if flag {
          Some(Ok(val))
        } else {
          None
        }
      }
      Ok(other) => Some(Err(ErrorStack::new(format!(
        "cb passed to take_while produced value which could not be interpreted as a bool; found: \
         {other:?}"
      )))),
      Err(err) => Some(Err(err.wrap("cb passed to `take_while` produced an error"))),
    }
  }
}

impl Sequence for TakeWhileSeq {
  fn clone_box(&self) -> Box<dyn Sequence> {
    Box::new(Self {
      inner: self.inner.clone_box(),
      cb: self.cb.clone(),
    })
  }

  fn consume<'a>(
    self: Box<Self>,
    ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    Box::new(TakeWhileIter {
      inner: self.inner.consume(ctx),
      cb: self.cb,
      ctx,
    })
  }
}

impl Debug for TakeWhileSeq {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "TakeWhileSeq {{ inner: {:?}, cb: {:?} }}",
      self.inner, self.cb
    )
  }
}
