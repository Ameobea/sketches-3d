import type { KeymapEntry } from 'src/geotoy/modules/keymapTable';

/**
 * DOM-level keyboard dispatch for the geotoy app, reproducing Viz's handling exactly:
 * input-like targets bail first, keys normalize to `${ctrl+}${shift+}${key}` (no
 * alt/meta). Listens in the capture phase so entries dispatch before Viz's bubble
 * handler and can preventDefault() to claim a key ahead of its Escape/pause logic.
 */
export class GeotoyKeymap {
  private table = new Map<string, KeymapEntry['action']>();

  setTable(entries: KeymapEntry[]) {
    this.table.clear();
    for (const entry of entries) {
      this.table.set(entry.key, entry.action);
    }
  }

  private handleKeyDown = (evt: KeyboardEvent) => {
    const target = evt.target;
    const isInputLikeTarget =
      target instanceof HTMLInputElement ||
      target instanceof HTMLTextAreaElement ||
      (target instanceof HTMLElement &&
        (target.isContentEditable || target.getAttribute('role') === 'textbox'));
    // Modal dialogs trap focus, so a target inside one means the user is in the dialog.
    if (isInputLikeTarget || (target instanceof Element && target.closest('dialog[open]'))) {
      return;
    }

    const key = `${evt.ctrlKey ? 'ctrl+' : ''}${evt.shiftKey ? 'shift+' : ''}${evt.key.toLowerCase()}`;
    this.table.get(key)?.(evt);
  };

  install() {
    window.addEventListener('keydown', this.handleKeyDown, { capture: true });
  }

  dispose() {
    window.removeEventListener('keydown', this.handleKeyDown, { capture: true });
  }
}
