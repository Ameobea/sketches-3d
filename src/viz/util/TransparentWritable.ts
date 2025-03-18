/**
 * Same as `Writable` from `svelte/store` but with the current value
 * available as a property.  This means you don't have to use `get()` to
 * access the current value.
 */

import { writable, type Writable } from 'svelte/store';

export interface TransparentWritable<T> extends Writable<T> {
  current: T;
}

export const rwritable = <T>(value: T): TransparentWritable<T> => {
  const inner = writable(value);

  const inst: TransparentWritable<T> = {
    subscribe: inner.subscribe,
    set: () => {
      throw new Error('Unreachable');
    },
    update: () => {
      throw new Error('Unreachable');
    },
    current: value,
  };

  inst.set = (newValue: T) => {
    inst.current = newValue;
    inner.set(newValue);
  };

  inst.update = (updater: (value: T) => T) =>
    inner.update(oldVal => {
      const newVal = updater(oldVal);
      inst.current = newVal;
      return newVal;
    });

  return inst;
};
