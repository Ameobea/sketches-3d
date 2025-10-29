use std::{collections::VecDeque, fmt::Debug, iter::Enumerate, rc::Rc};

use itertools::Itertools;
use mesh::{linked_mesh::Vec3, LinkedMesh};
use nalgebra::Matrix4;
use point_distribute::MeshSurfaceSampler;

use crate::{Callable, ErrorStack, EvalCtx, MeshHandle, Sequence, Value, EMPTY_KWARGS};

#[derive(Clone, Debug)]
pub(crate) struct IntRange {
  pub start: i64,
  pub end: Option<i64>,
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
      end: self.end.unwrap_or(i64::MAX),
    }
  }
}

impl Sequence for IntRange {
  fn consume(
    self: Rc<Self>,
    _ctx: &EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>>> {
    Box::new((*self).clone().into_iter())
  }
}

#[derive(Debug)]
pub(crate) struct MapSeq {
  pub inner: Rc<dyn Sequence>,
  pub cb: Rc<Callable>,
}

impl Sequence for MapSeq {
  fn consume<'a>(
    self: Rc<Self>,
    ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    let inner = Rc::clone(&self.inner).consume(ctx).enumerate();
    let cb = Rc::clone(&self.cb);
    Box::new(inner.map(move |(i, res)| {
      match res {
        Ok(v) => ctx
          .invoke_callable(&cb, &[v, Value::Int(i as i64)], &EMPTY_KWARGS)
          .map_err(|err| err.wrap("cb passed to map produced an error")),
        Err(err) => Err(err),
      }
    }))
  }
}

#[derive(Debug)]
pub(crate) struct FilterSeq {
  pub inner: Rc<dyn Sequence>,
  pub cb: Rc<Callable>,
}

impl Sequence for FilterSeq {
  fn consume<'a>(
    self: Rc<Self>,
    ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    let inner = Rc::clone(&self.inner).consume(ctx);
    let cb = Rc::clone(&self.cb);

    Box::new(
      inner
        .enumerate()
        .map(move |(i, res)| match res {
          Ok(v) => {
            let flag = ctx
              .invoke_callable(&cb, &[v.clone(), Value::Int(i as i64)], &EMPTY_KWARGS)
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

#[derive(Debug)]
pub(crate) struct ScanSeq {
  pub acc: Value,
  pub inner: Rc<dyn Sequence>,
  pub cb: Rc<Callable>,
}

pub(crate) struct ScanIter<'a> {
  acc: Value,
  inner: Enumerate<Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a>>,
  cb: Rc<Callable>,
  ctx: &'a EvalCtx,
}

impl<'a> Iterator for ScanIter<'a> {
  type Item = Result<Value, ErrorStack>;

  fn next(&mut self) -> Option<Self::Item> {
    if let Some(res) = self.inner.next() {
      match res {
        (ix, Ok(v)) => {
          let args = &[self.acc.clone(), v, Value::Int(ix as i64)];
          match self.ctx.invoke_callable(&self.cb, args, &EMPTY_KWARGS) {
            Ok(new_acc) => {
              self.acc = new_acc;
              Some(Ok(self.acc.clone()))
            }
            Err(err) => Some(Err(err.wrap("cb passed to scan produced an error"))),
          }
        }
        (_ix, Err(e)) => Some(Err(e)),
      }
    } else {
      None
    }
  }
}

impl Sequence for ScanSeq {
  fn consume<'a>(
    self: Rc<Self>,
    ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    Box::new(ScanIter {
      acc: self.acc.clone(),
      inner: Rc::clone(&self.inner).consume(ctx).enumerate(),
      cb: Rc::clone(&self.cb),
      ctx,
    })
  }
}

#[derive(Debug)]
pub(crate) struct FlattenSeq {
  pub inner: Rc<dyn Sequence>,
}

pub(crate) struct FlattenIter<'a> {
  inner: Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a>,
  cur_sub_iter: Option<Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a>>,
  ctx: &'a EvalCtx,
}

impl<'a> Iterator for FlattenIter<'a> {
  type Item = Result<Value, ErrorStack>;

  fn next(&mut self) -> Option<Self::Item> {
    if let Some(sub_iter) = &mut self.cur_sub_iter {
      if let Some(res) = sub_iter.next() {
        return Some(res);
      } else {
        self.cur_sub_iter = None;
      }
    }

    match self.inner.next() {
      Some(Ok(Value::Sequence(seq))) => {
        self.cur_sub_iter = Some(seq.consume(self.ctx));
        self.next()
      }
      Some(Ok(other)) => Some(Ok(other)),
      Some(Err(err)) => Some(Err(err.wrap("error produced by inner sequence in flatten"))),
      None => None,
    }
  }
}

impl Sequence for FlattenSeq {
  fn consume<'a>(
    self: Rc<Self>,
    ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    Box::new(FlattenIter {
      inner: Rc::clone(&self.inner).consume(ctx),
      cur_sub_iter: None,
      ctx,
    })
  }
}

#[derive(Clone, Debug)]
pub(crate) struct EagerSeq {
  // TODO: should it be wrapped with `Rc`?
  pub inner: Vec<Value>,
}

impl Sequence for EagerSeq {
  fn consume<'a>(
    self: Rc<Self>,
    _ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    Box::new(self.inner.clone().into_iter().map(Ok))
  }
}

#[derive(Debug)]
pub(crate) struct PointDistributeSeq {
  pub mesh: MeshHandle,
  pub seed: u64,
  pub point_count: Option<usize>,
  pub cb: Option<Rc<Callable>>,
  pub world_space: bool,
}

impl Clone for PointDistributeSeq {
  fn clone(&self) -> Self {
    Self {
      mesh: self.mesh.clone(false, false, false),
      seed: self.seed,
      point_count: self.point_count,
      cb: self.cb.clone(),
      world_space: self.world_space,
    }
  }
}

pub(crate) struct PointDistributeIter<'a> {
  ctx: &'a EvalCtx,
  #[allow(dead_code)]
  mesh: MeshHandle,
  inverse_transposed_transform: Matrix4<f32>,
  sampler: MeshSurfaceSampler<'static>,
  cb: Option<Rc<Callable>>,
  world_space: bool,
}

impl<'a> PointDistributeIter<'a> {
  pub fn new(
    ctx: &'a EvalCtx,
    mesh: MeshHandle,
    seed: u64,
    cb: Option<Rc<Callable>>,
    world_space: bool,
  ) -> Result<Self, ErrorStack> {
    // safe because this reference will only live as long as the iterator, and by holding the `Arc`
    // internally, we can ensure that it's not dropped while the iterator is still in use.
    let static_mesh: &'static LinkedMesh<()> =
      unsafe { std::mem::transmute::<&LinkedMesh<()>, &'static LinkedMesh<()>>(&mesh.mesh) };

    let sampler = MeshSurfaceSampler::new(static_mesh, Some(seed)).map_err(|err| {
      ErrorStack::wrap(
        ErrorStack::new(err),
        "Error creating point distribute sampler",
      )
    })?;

    Ok(Self {
      ctx,
      inverse_transposed_transform: mesh.transform.try_inverse().unwrap().transpose(),
      mesh,
      sampler,
      cb,
      world_space,
    })
  }
}

impl<'a> Iterator for PointDistributeIter<'a> {
  type Item = Result<Value, ErrorStack>;

  fn next(&mut self) -> Option<Self::Item> {
    match self.sampler.sample() {
      (pos, mut normal) => {
        let mut pos = Vec3::new(pos.x, pos.y, pos.z);
        if self.world_space {
          pos = (self.mesh.transform * pos.push(1.)).xyz();
          normal = (self.inverse_transposed_transform * normal.push(0.))
            .xyz()
            .normalize();
        }
        let value = Value::Vec3(Vec3::new(pos.x, pos.y, pos.z));
        if let Some(cb) = &self.cb {
          let mapped = self
            .ctx
            .invoke_callable(cb, &[value, Value::Vec3(normal)], &EMPTY_KWARGS);
          Some(mapped)
        } else {
          Some(Ok(value))
        }
      }
    }
  }
}

impl Sequence for PointDistributeSeq {
  fn consume<'a>(
    self: Rc<Self>,
    ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    let mesh = self.mesh.clone(false, false, false);
    let iter =
      match PointDistributeIter::new(ctx, mesh, self.seed, self.cb.clone(), self.world_space) {
        Ok(iter) => iter,
        Err(err) => {
          return Box::new(std::iter::once(Err(
            err.wrap("Error creating point distribute iterator"),
          )))
        }
      };

    if let Some(point_count) = self.point_count {
      Box::new(iter.take(point_count))
    } else {
      Box::new(iter)
    }
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
  fn consume<'a>(
    self: Rc<Self>,
    _ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    Box::new(self.inner.clone())
  }
}

#[derive(Debug)]
pub(crate) struct MeshVertsSeq<const WORLD_SPACE: bool> {
  pub mesh: MeshHandle,
}

impl<const WORLD_SPACE: bool> Clone for MeshVertsSeq<WORLD_SPACE> {
  fn clone(&self) -> Self {
    Self {
      mesh: self.mesh.clone(false, false, false),
    }
  }
}

pub(crate) struct MeshVertsIter<const WORLD_SPACE: bool> {
  #[allow(dead_code)]
  mesh: MeshHandle,
  iter: Box<dyn Iterator<Item = Result<Value, ErrorStack>>>,
}

impl<const WORLD_SPACE: bool> MeshVertsIter<WORLD_SPACE> {
  pub fn new(mesh: MeshHandle) -> Self {
    // safe because this reference will only live as long as the iterator, and by holding the `Arc`
    // internally, we can ensure that it's not dropped while the iterator is still in use.
    let static_mesh: &'static LinkedMesh<()> =
      unsafe { std::mem::transmute::<&LinkedMesh<()>, &'static LinkedMesh<()>>(&*mesh.mesh) };

    let iter: impl Iterator<Item = _> + 'static = static_mesh.vertices.values().map(move |vtx| {
      let mut pos = vtx.position;
      if WORLD_SPACE {
        pos = (mesh.transform * pos.push(1.)).xyz();
      }
      Ok(Value::Vec3(pos))
    });

    Self {
      mesh,
      iter: Box::new(iter),
    }
  }
}

impl<const WORLD_SPACE: bool> Iterator for MeshVertsIter<WORLD_SPACE> {
  type Item = Result<Value, ErrorStack>;

  fn next(&mut self) -> Option<Self::Item> {
    self.iter.next()
  }
}

impl<const WORLD_SPACE: bool> Sequence for MeshVertsSeq<WORLD_SPACE> {
  fn consume<'a>(
    self: Rc<Self>,
    _ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    Box::new(MeshVertsIter::<WORLD_SPACE>::new(
      self.mesh.clone(false, false, false),
    ))
  }
}

pub(crate) struct TakeSeq {
  pub inner: Rc<dyn Sequence>,
  pub count: usize,
}

impl Sequence for TakeSeq {
  fn consume<'a>(
    self: Rc<Self>,
    ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    let inner = Rc::clone(&self.inner).consume(ctx);
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
  pub inner: Rc<dyn Sequence>,
  pub count: usize,
}

impl Sequence for SkipSeq {
  fn consume<'a>(
    self: Rc<Self>,
    ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    let inner = Rc::clone(&self.inner).consume(ctx);
    Box::new(inner.skip(self.count))
  }
}

impl Debug for SkipSeq {
  #[cold]
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "SkipSeq {{ count: {}, inner: {:?} }}",
      self.count, self.inner
    )
  }
}

pub(crate) struct TakeWhileSeq {
  pub inner: Rc<dyn Sequence>,
  pub cb: Rc<Callable>,
}

pub(crate) struct TakeWhileIter<'a> {
  inner: Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a>,
  cb: Rc<Callable>,
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

    match self
      .ctx
      .invoke_callable(&self.cb, &[val.clone()], &EMPTY_KWARGS)
    {
      Ok(Value::Bool(flag)) => {
        if flag {
          Some(Ok(val))
        } else {
          None
        }
      }
      Ok(other) => Some(Err(ErrorStack::new(format!(
        "cb passed to `take_while` produced value which could not be interpreted as a bool; \
         found: {other:?}"
      )))),
      Err(err) => Some(Err(err.wrap("cb passed to `take_while` produced an error"))),
    }
  }
}

impl Sequence for TakeWhileSeq {
  fn consume<'a>(
    self: Rc<Self>,
    ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    Box::new(TakeWhileIter {
      inner: Rc::clone(&self.inner).consume(ctx),
      cb: Rc::clone(&self.cb),
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

#[derive(Debug)]
pub(crate) struct SkipWhileSeq {
  pub inner: Rc<dyn Sequence>,
  pub cb: Rc<Callable>,
}

pub(crate) struct SkipWhileIter<'a> {
  inner: Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a>,
  cb: Rc<Callable>,
  ctx: &'a EvalCtx,
}

impl Iterator for SkipWhileIter<'_> {
  type Item = Result<Value, ErrorStack>;

  fn next(&mut self) -> Option<Self::Item> {
    while let Some(res) = self.inner.next() {
      let Ok(val) = res else {
        return Some(res);
      };

      match self
        .ctx
        .invoke_callable(&self.cb, &[val.clone()], &EMPTY_KWARGS)
      {
        Ok(Value::Bool(flag)) => {
          if flag {
            continue;
          } else {
            return Some(Ok(val));
          }
        }
        Ok(other) => {
          return Some(Err(ErrorStack::new(format!(
            "cb passed to `skip_while` produced value which could not be interpreted as a bool; \
             found: {other:?}"
          ))));
        }
        Err(err) => return Some(Err(err.wrap("cb passed to `skip_while` produced an error"))),
      }
    }
    None
  }
}

impl Sequence for SkipWhileSeq {
  fn consume<'a>(
    self: Rc<Self>,
    ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    Box::new(SkipWhileIter {
      inner: Rc::clone(&self.inner).consume(ctx),
      cb: Rc::clone(&self.cb),
      ctx,
    })
  }
}

#[derive(Debug)]
pub(crate) struct ChainSeq {
  pub inner: VecDeque<Rc<dyn Sequence>>,
}

impl ChainSeq {
  pub(crate) fn new(ctx: &EvalCtx, seqs: Rc<dyn Sequence>) -> Result<Self, ErrorStack> {
    let seqs = seqs
      .consume(ctx)
      .map(|res| match res {
        Ok(Value::Sequence(seq)) => Ok(seq),
        Ok(other) => Err(ErrorStack::new(format!(
          "got non-seq value in seq passed to `chain`: {other:?}"
        ))),
        Err(err) => Err(err.wrap("error produced by seq of seqs passed to `chain`")),
      })
      .collect::<Result<VecDeque<_>, _>>()?;
    Ok(Self { inner: seqs })
  }
}

pub(crate) struct ChainIter<'a> {
  ctx: &'a EvalCtx,
  inner: VecDeque<Rc<dyn Sequence>>,
  cur: Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a>,
}

impl<'a> ChainIter<'a> {
  pub fn new(
    ctx: &'a EvalCtx,
    mut inner: VecDeque<Rc<dyn Sequence>>,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    match inner.pop_front() {
      Some(seq) => {
        let iter = seq.consume(ctx);
        Box::new(ChainIter {
          ctx,
          inner: inner,
          cur: iter,
        })
      }
      None => Box::new(std::iter::empty()),
    }
  }
}

impl<'a> Iterator for ChainIter<'a> {
  type Item = Result<Value, ErrorStack>;

  fn next(&mut self) -> Option<Self::Item> {
    loop {
      match self.cur.next() {
        Some(res) => return Some(res),
        None => {
          if let Some(next_seq) = self.inner.pop_front() {
            self.cur = next_seq.consume(self.ctx);
          } else {
            return None;
          }
        }
      }
    }
  }
}

impl Sequence for ChainSeq {
  fn consume<'a>(
    self: Rc<Self>,
    ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    Box::new(ChainIter::new(ctx, self.inner.clone()))
  }
}
