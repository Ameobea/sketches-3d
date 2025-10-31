use std::{marker::PhantomData, mem::ManuallyDrop, num::NonZeroU32};

use slotmap::{Key, KeyData, SlotMap};

use crate::linked_mesh::{EdgeKey, VertexKey};

pub fn vkey(ix: u32, version: u32) -> VertexKey {
  debug_assert!(version > 0);
  unsafe {
    std::mem::transmute(LocalKeyData {
      idx: ix,
      version: NonZeroU32::new_unchecked(version),
    })
  }
}

#[cfg(test)]
pub fn fkey(ix: u32, version: u32) -> crate::linked_mesh::FaceKey {
  debug_assert!(version > 0);
  unsafe {
    std::mem::transmute(LocalKeyData {
      idx: ix,
      version: NonZeroU32::new_unchecked(version),
    })
  }
}

pub fn vkey_ix(key: &VertexKey) -> u32 {
  let key: &LocalKeyData = unsafe { std::mem::transmute(key) };
  key.idx
}

pub fn vkey_ix_mut<'a>(key: &'a mut VertexKey) -> &'a mut u32 {
  let key: &mut LocalKeyData = unsafe { std::mem::transmute(key) };
  &mut key.idx
}

pub fn vkey_version(key: &VertexKey) -> u32 {
  let key: &LocalKeyData = unsafe { std::mem::transmute(key) };
  key.version.get()
}

pub fn ekey_ix(key: &EdgeKey) -> u32 {
  let key: &LocalKeyData = unsafe { std::mem::transmute(key) };
  key.idx
}

#[allow(dead_code)]
pub struct LocalKeyData {
  pub idx: u32,
  pub version: NonZeroU32,
}

// Storage inside a slot or metadata for the freelist when vacant.
union LocalSlotUnion<T> {
  value: ManuallyDrop<T>,
  next_free: u32,
}

// A slot, which represents storage for a value and a current version.
// Can be occupied or vacant.
#[allow(dead_code)]
pub struct LocalSlot<T> {
  u: LocalSlotUnion<T>,
  version: u32, // Even = vacant, odd = occupied.
}

/// Same as `SlotMap`, done so that we can do fun low-level things that the library doesn't think
/// we're worthy of.
pub struct LocalSlotMap<K: Key, V> {
  pub slots: Vec<LocalSlot<V>>,
  pub free_head: u32,
  pub num_elems: u32,
  pub _k: PhantomData<fn(K) -> K>,
}

/// Optimized routine to build a slotmap from an iterator of values.  This avoids a lot of
/// conditional logic when inserting elements one at a time.
pub fn build_slotmap_from_iter_with_key<K: Key, V, T, I, F>(iter: I, mut f: F) -> SlotMap<K, V>
where
  I: ExactSizeIterator<Item = T>,
  F: FnMut(T, K) -> V,
{
  let count = iter.len();
  let mut slots = Vec::with_capacity(count + 1);
  slots.push(LocalSlot {
    u: LocalSlotUnion { next_free: 0 },
    version: 0,
  });

  let mut slotmap: LocalSlotMap<K, V> = LocalSlotMap {
    slots,
    free_head: 1,
    num_elems: 0,
    _k: PhantomData,
  };

  unsafe {
    slotmap.slots.set_len(count + 1);
    let base_ptr = slotmap.slots.as_mut_ptr().add(1);

    for (i, value) in iter.enumerate() {
      let ptr = base_ptr.add(i);
      let version = 1u32;
      let kd: KeyData = std::mem::transmute(LocalKeyData {
        idx: (i + 1) as u32,
        version: NonZeroU32::new_unchecked(version),
      });
      let key: K = kd.into();
      let value = f(value, key);
      std::ptr::write(
        ptr,
        LocalSlot {
          u: LocalSlotUnion {
            value: ManuallyDrop::new(value),
          },
          version,
        },
      );
    }

    slotmap.num_elems = count as u32;
    slotmap.free_head = count as u32 + 1;
  }

  unsafe { std::mem::transmute(slotmap) }
}

pub fn build_slotmap_from_iter<K, V, I>(iter: I) -> SlotMap<K, V>
where
  K: Key + From<KeyData>,
  I: ExactSizeIterator<Item = V>,
{
  build_slotmap_from_iter_with_key(iter, |v, _| v)
}

/// Fastpath that inserts a new value into a `SlotMap` that we know is dense, meaning nothing has
/// ever been removed from it and it has no vacant slots.
///
/// This allows us to skip logic relating to the freelist and versioning and just write directly to
/// the next slot.
pub fn slotmap_insert_dense_with_key<K: Key, V>(
  slotmap: &mut SlotMap<K, V>,
  f: impl FnOnce(K) -> V,
) -> K {
  let slotmap =
    unsafe { std::mem::transmute::<&mut SlotMap<K, V>, &mut LocalSlotMap<K, V>>(slotmap) };

  debug_assert!(slotmap.slots.get_mut(slotmap.free_head as usize).is_none());

  let new_num_elems = slotmap.num_elems + 1;
  let version = 1;
  let kd = LocalKeyData {
    idx: slotmap.slots.len() as u32,
    version: unsafe { NonZeroU32::new_unchecked(version) },
  };

  slotmap.free_head = kd.idx + 1;
  slotmap.num_elems = new_num_elems;

  // Create new slot before adjusting freelist in case f or the allocation panics or errors.
  let kd: KeyData = unsafe { std::mem::transmute(kd) };
  let key: K = kd.into();
  slotmap.slots.push(LocalSlot {
    u: LocalSlotUnion {
      value: ManuallyDrop::new(f(key)),
    },
    version,
  });

  key
}

pub fn slotmap_insert_dense<K, V>(slotmap: &mut SlotMap<K, V>, value: V) -> K
where
  K: Key,
{
  slotmap_insert_dense_with_key(slotmap, |_| value)
}
