//! Retained-size accounting for values held by the cross-run const-eval cache.

use std::rc::Rc;

use fxhash::FxHashSet;
use mesh::{
  linked_mesh::{Edge, Face, Vec3, Vertex},
  LinkedMesh,
};

use crate::{Callable, TexKind, Value};

/// The distinct heap allocations a value keeps alive, as `(address, bytes)` pairs — a set
/// rather than a total because texture ops freely share plane buffers and cached values wrap
/// each other, so callers dedupe by address. Only allocations owned through an `Rc` reachable
/// from the value are reported; lazily-populated memo fields (mip chains, cached trimeshes)
/// can be replaced behind a shared reference, which would leave a stale address charged.
pub(crate) fn retained_allocs(value: &Value) -> Vec<(usize, usize)> {
  let mut walk = Walk::default();
  walk.value(value);
  walk.allocs
}

#[derive(Default)]
struct Walk {
  seen: FxHashSet<usize>,
  allocs: Vec<(usize, usize)>,
}

/// Slotmap arenas are sized by capacity, not live count — a mesh decimated from 1M verts still
/// holds the 1M-slot arena. Inline `SmallVec` storage only, so adjacency that spilled to the
/// heap is missed; average valence sits under the 9-edge inline capacity.
fn linked_mesh_bytes(m: &LinkedMesh<()>) -> usize {
  m.vertices.capacity() * size_of::<Vertex>()
    + m.faces.capacity() * size_of::<Face<()>>()
    + m.edges.capacity() * size_of::<Edge>()
    + (m.shading_normals.capacity()
      + m.displacement_normals.capacity()
      + m.edge_displacement_normals.capacity())
      * size_of::<Vec3>()
}

impl Walk {
  /// False when this allocation was already charged on this walk.
  fn charge(&mut self, addr: usize, bytes: usize) -> bool {
    if !self.seen.insert(addr) {
      return false;
    }
    self.allocs.push((addr, bytes));
    true
  }

  fn str_buf(&mut self, s: &str) {
    if !s.is_empty() {
      self.charge(s.as_ptr() as usize, s.len());
    }
  }

  fn value(&mut self, value: &Value) {
    match value {
      Value::Texture(tex) => {
        let planes = match &tex.storage.kind {
          TexKind::Planes(planes) => planes,
          TexKind::View(view) => &view.planes,
        };
        let spine = size_of::<crate::TextureHandle>() + planes.len() * size_of::<Rc<Vec<f32>>>();
        if !self.charge(Rc::as_ptr(tex) as usize, spine) {
          return;
        }
        for plane in planes {
          self.charge(
            Rc::as_ptr(plane) as usize,
            size_of::<Vec<f32>>() + plane.len() * size_of::<f32>(),
          );
        }
      }
      Value::Mesh(handle) => {
        if !self.charge(Rc::as_ptr(handle) as usize, size_of::<crate::MeshHandle>()) {
          return;
        }
        self.charge(
          Rc::as_ptr(&handle.mesh) as usize,
          linked_mesh_bytes(&handle.mesh),
        );
        if let Some(material) = &handle.material {
          self.charge(Rc::as_ptr(material) as usize, size_of::<crate::Material>());
        }
      }
      Value::Sequence(seq) => {
        let addr = Rc::as_ptr(seq) as *const u8 as usize;
        if !self.charge(addr, size_of_val(&**seq)) {
          return;
        }
        if let Some(eager) = crate::seq_as_eager(&**seq) {
          if self.charge(
            Rc::as_ptr(&eager.inner) as usize,
            size_of::<Vec<Value>>() + eager.inner.len() * size_of::<Value>(),
          ) {
            for el in eager.inner.iter() {
              self.value(el);
            }
          }
          return;
        }
        // Lazy sequences hold their source seq and callables here. Two variants
        // (`point_distribute`, mesh vertex iteration) also hold a mesh they don't report,
        // so those undercount by one handle.
        for dep in seq.consumption_deps().into_iter().flatten() {
          self.value(&dep);
        }
      }
      Value::Map(map) => {
        let bytes = size_of::<crate::FxHashMap<String, Value>>()
          + map.len() * (size_of::<(String, Value)>() + 1);
        if !self.charge(Rc::as_ptr(map) as usize, bytes) {
          return;
        }
        for (key, val) in map.iter() {
          self.str_buf(key);
          self.value(val);
        }
      }
      Value::Callable(callable) => {
        if self.charge(Rc::as_ptr(callable) as usize, size_of::<Callable>()) {
          self.callable(callable);
        }
      }
      Value::String(s) => self.str_buf(s),
      Value::Material(m) => {
        self.charge(Rc::as_ptr(m) as usize, size_of::<crate::Material>());
      }
      Value::Mat4(m) => {
        self.charge(Rc::as_ptr(m) as usize, size_of::<crate::Mat4>());
      }
      Value::Vec4(v) => {
        self.charge(Rc::as_ptr(v) as usize, size_of::<crate::Vec4>());
      }
      Value::Light(light) => {
        self.charge(&**light as *const _ as usize, size_of::<crate::Light>());
      }
      Value::Nil
      | Value::Int(_)
      | Value::Float(_)
      | Value::Vec2(_)
      | Value::Vec3(_)
      | Value::Bool(_) => (),
    }
  }

  fn callable(&mut self, callable: &Callable) {
    match callable {
      Callable::Closure(closure) => {
        let addr = Rc::as_ptr(&closure.captures) as *const u8 as usize;
        if self.charge(addr, closure.captures.len() * size_of::<Value>()) {
          for captured in closure.captures.iter() {
            self.value(captured);
          }
        }
      }
      Callable::PartiallyAppliedFn(paf) => {
        for arg in paf.args.iter().chain(paf.kwargs.values()) {
          self.value(arg);
        }
        if self.charge(Rc::as_ptr(&paf.inner) as usize, size_of::<Callable>()) {
          self.callable(&paf.inner);
        }
      }
      Callable::ComposedFn(composed) => {
        for inner in &composed.inner {
          if self.charge(Rc::as_ptr(inner) as usize, size_of::<Callable>()) {
            self.callable(inner);
          }
        }
      }
      Callable::Builtin { .. } | Callable::Dynamic { .. } => (),
    }
  }
}
