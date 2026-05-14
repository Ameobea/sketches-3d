// Snapshot-based undo for the geotoy tree. Each entry stores the full tree
// plus selection/solo before/after — cheap enough at this scale to skip
// patch-style diffs.
//
// Coalescing: same-keyed pushes whose gap to the previous push is < COALESCE_WINDOW_MS
// merge into one entry. The window is measured against the LAST push, not the burst
// start — a continuous stream (drag at frame-rate, fast typing) keeps extending one
// entry indefinitely; any pause > COALESCE_WINDOW_MS ends the burst. This is the
// natural shape for both drags and keystroke bursts: a single visual gesture
// collapses to a single undo step.

import type { TreeDef } from 'src/geoscript/geotoyAPIClient';

const COALESCE_WINDOW_MS = 800;
const MAX_UNDO = 200;

export interface TreeSnapshot {
  tree: TreeDef;
  selectedId: string | null;
  soloId: string | null;
}

interface UndoEntry {
  coalesceKey: string | null;
  before: TreeSnapshot;
  after: TreeSnapshot;
  timestamp: number;
}

const snapshotsEqual = (a: TreeSnapshot, b: TreeSnapshot): boolean =>
  a.selectedId === b.selectedId && a.soloId === b.soloId && JSON.stringify(a.tree) === JSON.stringify(b.tree);

export class TreeUndoSystem {
  private undoStack: UndoEntry[] = [];
  private redoStack: UndoEntry[] = [];

  /** Would the next `push(coalesceKey, ...)` merge into the previous entry rather
   *  than add a new one? Lets the caller skip capturing a `before` snapshot it
   *  knows will be discarded. */
  wouldCoalesce(coalesceKey: string | null, now: number = Date.now()): boolean {
    if (coalesceKey === null) return false;
    const last = this.undoStack[this.undoStack.length - 1];
    return !!(last && last.coalesceKey === coalesceKey && now - last.timestamp < COALESCE_WINDOW_MS);
  }

  /** `before === null` is a signal "I predicted coalescing and skipped the
   *  before-snapshot." If coalescing no longer applies (e.g., the previous entry
   *  aged out between the prediction and the call) we no-op rather than push a
   *  half-built entry; the caller is expected to retry with a real `before` if
   *  it cares. */
  push(
    coalesceKey: string | null,
    before: TreeSnapshot | null,
    after: TreeSnapshot,
    now: number = Date.now()
  ): void {
    const last = this.undoStack[this.undoStack.length - 1];
    if (
      last &&
      coalesceKey !== null &&
      last.coalesceKey === coalesceKey &&
      now - last.timestamp < COALESCE_WINDOW_MS
    ) {
      last.after = after;
      last.timestamp = now;
      this.redoStack.length = 0;
      return;
    }

    if (before === null) return;
    if (snapshotsEqual(before, after)) return;

    this.undoStack.push({ coalesceKey, before, after, timestamp: now });
    if (this.undoStack.length > MAX_UNDO) this.undoStack.shift();
    this.redoStack.length = 0;
  }

  undo(): TreeSnapshot | null {
    const entry = this.undoStack.pop();
    if (!entry) return null;
    this.redoStack.push(entry);
    return entry.before;
  }

  redo(): TreeSnapshot | null {
    const entry = this.redoStack.pop();
    if (!entry) return null;
    this.undoStack.push(entry);
    return entry.after;
  }

  canUndo(): boolean {
    return this.undoStack.length > 0;
  }

  canRedo(): boolean {
    return this.redoStack.length > 0;
  }

  clear(): void {
    this.undoStack.length = 0;
    this.redoStack.length = 0;
  }
}
