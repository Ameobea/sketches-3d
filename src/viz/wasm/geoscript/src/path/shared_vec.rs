use std::{
  cell::{Cell, UnsafeCell},
  fmt,
  ops::Deref,
  rc::Rc,
};

struct Buf<T> {
  data: UnsafeCell<Vec<T>>,
  frozen: Cell<bool>,
}

/// Prefix view onto a shared append-only buffer. Pushing onto a view that ends where the buffer
/// ends appends in place, so a pen-op chain stays O(1) per op while every earlier view keeps
/// seeing only its prefix. Handing out a slice freezes the buffer for good; a later push then
/// copies the prefix into a fresh buffer, which is what keeps those slices valid.
pub(crate) struct SharedVec<T> {
  buf: Rc<Buf<T>>,
  len: usize,
}

impl<T> SharedVec<T> {
  fn wrap(data: Vec<T>) -> Self {
    Self {
      len: data.len(),
      buf: Rc::new(Buf {
        data: UnsafeCell::new(data),
        frozen: Cell::new(false),
      }),
    }
  }

  pub(crate) fn new() -> Self {
    Self::wrap(Vec::new())
  }

  pub(crate) fn len(&self) -> usize {
    self.len
  }

  pub(crate) fn is_empty(&self) -> bool {
    self.len == 0
  }

  fn data(&self) -> &Vec<T> {
    unsafe { &*self.buf.data.get() }
  }

  pub(crate) fn as_slice(&self) -> &[T] {
    self.buf.frozen.set(true);
    &self.data()[..self.len]
  }

  /// Value read that leaves the buffer extendable in place.
  pub(crate) fn last_copied(&self) -> Option<T>
  where
    T: Copy,
  {
    self.len.checked_sub(1).map(|ix| self.data()[ix])
  }

  pub(crate) fn push(&mut self, value: T)
  where
    T: Clone,
  {
    let in_place = !self.buf.frozen.get() && self.data().len() == self.len;
    if !in_place {
      *self = Self::wrap(self.data()[..self.len].to_vec());
    }
    unsafe { (*self.buf.data.get()).push(value) }
    self.len += 1;
  }

  /// Allocation identity + size of the backing buffer, for retained-size accounting.
  pub(crate) fn alloc(&self) -> (usize, usize) {
    (self.data().as_ptr() as usize, self.data().capacity())
  }
}

impl<T> Deref for SharedVec<T> {
  type Target = [T];

  fn deref(&self) -> &[T] {
    self.as_slice()
  }
}

impl<'a, T> IntoIterator for &'a SharedVec<T> {
  type Item = &'a T;
  type IntoIter = std::slice::Iter<'a, T>;

  fn into_iter(self) -> Self::IntoIter {
    self.as_slice().iter()
  }
}

impl<T> Clone for SharedVec<T> {
  fn clone(&self) -> Self {
    Self {
      buf: Rc::clone(&self.buf),
      len: self.len,
    }
  }
}

impl<T> From<Vec<T>> for SharedVec<T> {
  fn from(data: Vec<T>) -> Self {
    Self::wrap(data)
  }
}

impl<T> FromIterator<T> for SharedVec<T> {
  fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
    Self::wrap(iter.into_iter().collect())
  }
}

impl<T: fmt::Debug> fmt::Debug for SharedVec<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    self.as_slice().fmt(f)
  }
}

#[cfg(test)]
mod tests {
  use super::SharedVec;

  #[test]
  fn chain_shares_and_branch_copies() {
    let mut a = SharedVec::new();
    a.push(1);
    let b = a.clone();
    a.push(2);
    assert_eq!(&*b, &[1]);
    assert_eq!(&*a, &[1, 2]);
    let (pa, _) = a.alloc();
    let mut c = b.clone();
    c.push(3);
    assert_eq!(&*c, &[1, 3]);
    assert_ne!(c.alloc().0, pa);
    assert_eq!(&*a, &[1, 2]);
    let mut d = a.clone();
    d.push(4);
    assert_ne!(d.alloc().0, pa);
    assert_eq!(&*a, &[1, 2]);
    assert_eq!(&*d, &[1, 2, 4]);
  }

  #[test]
  fn unread_tail_extends_in_place() {
    let mut a = SharedVec::from(vec![1, 2]);
    let (p0, _) = a.alloc();
    a.push(3);
    assert_eq!(a.last_copied(), Some(3));
    a.push(4);
    assert_eq!(a.alloc().0, p0);
    assert_eq!(a.len(), 4);
  }
}
