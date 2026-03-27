const MAX_UNDO = 50;

/**
 * Generic undo/redo stack. Each entry is an opaque value — the caller provides
 * an `apply` callback that knows how to execute an entry in either direction.
 *
 * Designed to be extended with new entry types (e.g. CSG tree edits) without
 * changing the undo system itself.
 */
export class UndoSystem<T> {
  private undoStack: T[] = [];
  private redoStack: T[] = [];

  push(entry: T) {
    this.undoStack.push(entry);
    if (this.undoStack.length > MAX_UNDO) this.undoStack.shift();
    this.redoStack.length = 0;
  }

  undo(apply: (entry: T, direction: 'undo') => void): boolean {
    const entry = this.undoStack.pop();
    if (!entry) return false;
    this.redoStack.push(entry);
    apply(entry, 'undo');
    return true;
  }

  redo(apply: (entry: T, direction: 'redo') => void): boolean {
    const entry = this.redoStack.pop();
    if (!entry) return false;
    this.undoStack.push(entry);
    apply(entry, 'redo');
    return true;
  }

  /** Remove entries matching a predicate from both stacks. */
  purge(predicate: (entry: T) => boolean) {
    this.undoStack = this.undoStack.filter(e => !predicate(e));
    this.redoStack = this.redoStack.filter(e => !predicate(e));
  }

  clear() {
    this.undoStack.length = 0;
    this.redoStack.length = 0;
  }
}
