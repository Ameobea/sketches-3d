use itertools::Itertools;
use mesh::{linked_mesh::Vec3, LinkedMesh};
use point_distribute::MeshSurfaceSampler;

use crate::{Callable, EvalCtx, Sequence, Value};

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
  type Item = Result<Value, String>;

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
  type Item = Result<Value, String>;
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

  fn consume(self: Box<Self>, _ctx: &EvalCtx) -> Box<dyn Iterator<Item = Result<Value, String>>> {
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
  ) -> Box<dyn Iterator<Item = Result<Value, String>> + 'a> {
    let inner = self.inner.consume(ctx);
    // TODO: more clones here
    let cb = self.cb;
    Box::new(inner.map(move |res| {
      match res {
        Ok(v) => ctx
          .invoke_callable(&cb, vec![v], Default::default(), &ctx.globals)
          .map_err(|err| format!("cb passed to map produced an error: {err}",)),
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
  ) -> Box<dyn Iterator<Item = Result<Value, String>> + 'a> {
    let inner = self.inner.consume(ctx);
    let cb = self.cb;

    Box::new(
      inner
        .map(move |res| match res {
          Ok(v) => {
            let flag = ctx
              .invoke_callable(&cb, vec![v.clone()], Default::default(), &ctx.globals)
              .map_err(|err| format!("cb passed to filter produced an error: {err}"))?;
            let Some(flag) = flag.as_bool() else {
              return Err(format!(
                "cb passed to filter produced value which could not be interpreted as a bool: \
                 {flag:?}"
              ));
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
  ) -> Box<dyn Iterator<Item = Result<Value, String>> + 'a> {
    Box::new(self.inner.into_iter().map(Ok))
  }
}

#[derive(Clone, Debug)]
pub(crate) struct PointDistributeSeq {
  pub mesh: LinkedMesh<()>,
  pub point_count: usize,
}

impl Sequence for PointDistributeSeq {
  fn clone_box(&self) -> Box<dyn Sequence> {
    Box::new(self.clone())
  }

  fn consume<'a>(
    self: Box<Self>,
    _ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, String>> + 'a> {
    let mesh = self.mesh;
    let sampler = MeshSurfaceSampler::new(&mesh);

    // TODO: not doing this the proper lazy way to avoid self-referential struct headaches
    Box::new(
      (0..self.point_count)
        .map(move |_| {
          // TODO: If we ever need normals, we'll have to use a composite data type for the sequence
          let (pos, _normal) = sampler.sample();
          let value = Value::Vec3(Vec3::new(pos.x, pos.y, pos.z));
          Ok(value)
        })
        .collect::<Vec<_>>()
        .into_iter(),
    )
  }
}
