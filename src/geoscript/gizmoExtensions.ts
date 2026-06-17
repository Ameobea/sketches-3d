import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  type ViewUpdate,
  WidgetType,
} from '@codemirror/view';
import { type Extension, StateEffect, StateField, Transaction } from '@codemirror/state';

import { evalMathExpr } from 'src/viz/util/mathExpr';
import { scanGizmoSites, type GizmoSite } from './gizmoScan';

export interface GizmoReadout {
  kind: 'vec3' | 'transform';
  vec3?: [number, number, number];
  transform?: {
    pos: [number, number, number];
    rot: [number, number, number];
    scale: [number, number, number];
  };
}

export interface GizmoEditorHooks {
  /** Toggle: arm the handle's viewport gizmo (or disarm if it's already the armed one). */
  arm: (handleId: string, kind: 'vec3' | 'transform') => void;
  disarm: () => void;
  /** Clear the stored value, returning the handle to its default. */
  resetHandle: (handleId: string) => void;
  /** Commit a directly-typed vec3 value from the inline readout editor. */
  setHandleVec3: (handleId: string, value: [number, number, number]) => void;
  getArmedHandleId: () => string | null;
}

const setValuesEffect = StateEffect.define<Map<string, GizmoReadout>>();
const patchValueEffect = StateEffect.define<{ id: string; readout: GizmoReadout }>();
const setArmedEffect = StateEffect.define<string | null>();

/** The host pushes readouts/arm state straight into the editor (no subscription indirection). */
export const pushGizmoArmed = (view: EditorView, handleId: string | null): void =>
  void view.dispatch({ effects: setArmedEffect.of(handleId) });
export const pushGizmoValues = (view: EditorView, values: Map<string, GizmoReadout>): void =>
  void view.dispatch({
    effects: setValuesEffect.of(values),
    annotations: Transaction.addToHistory.of(false),
  });
/** Single-handle update for the drag hot path — avoids rebuilding the whole readout map. */
export const pushGizmoValue = (view: EditorView, id: string, readout: GizmoReadout): void =>
  void view.dispatch({
    effects: patchValueEffect.of({ id, readout }),
    annotations: Transaction.addToHistory.of(false),
  });

const num = (n: number): string => (Number.isInteger(n) ? `${n}` : n.toFixed(2));

/** Full-ish precision for the editable inputs, capped at 8 decimals (drops float noise). */
const editNum = (n: number): string => `${parseFloat(n.toFixed(8))}`;

const readoutText = (r: GizmoReadout | undefined): string => {
  if (!r) return '⟨–⟩';
  const v = r.kind === 'transform' ? (r.transform?.pos ?? [0, 0, 0]) : (r.vec3 ?? [0, 0, 0]);
  return `⟨${num(v[0])}, ${num(v[1])}, ${num(v[2])}⟩`;
};

export const buildGizmoExtensions = (hooks: GizmoEditorHooks): Extension[] => {
  const valuesField = StateField.define<Map<string, GizmoReadout>>({
    create: () => new Map(),
    update: (value, tr) => {
      for (const e of tr.effects) {
        if (e.is(setValuesEffect)) return e.value;
        if (e.is(patchValueEffect)) return new Map(value).set(e.value.id, e.value.readout);
      }
      return value;
    },
  });

  class ChipWidget extends WidgetType {
    constructor(
      readonly handleId: string,
      readonly kind: 'vec3' | 'transform',
      readonly armed: boolean
    ) {
      super();
    }
    eq(o: ChipWidget) {
      return o.handleId === this.handleId && o.kind === this.kind && o.armed === this.armed;
    }
    toDOM() {
      const wrap = document.createElement('span');
      wrap.className = 'cm-gizmo-chip-wrap';
      wrap.setAttribute('contenteditable', 'false');

      const chip = document.createElement('span');
      chip.className = 'cm-gizmo-chip' + (this.armed ? ' cm-gizmo-chip-armed' : '');
      chip.textContent = this.kind === 'transform' ? '✥' : '⬩';
      chip.title = `${this.armed ? 'disarm' : 'arm'} gizmo "${this.handleId}"`;
      chip.addEventListener('mousedown', e => e.preventDefault());
      chip.addEventListener('click', e => {
        e.preventDefault();
        if (this.armed) hooks.disarm();
        else hooks.arm(this.handleId, this.kind);
      });
      wrap.appendChild(chip);

      if (this.armed) {
        const reset = document.createElement('span');
        reset.className = 'cm-gizmo-reset';
        reset.textContent = '⟲';
        reset.title = `reset gizmo "${this.handleId}" to default`;
        reset.addEventListener('mousedown', e => e.preventDefault());
        reset.addEventListener('click', e => {
          e.preventDefault();
          hooks.resetHandle(this.handleId);
        });
        wrap.appendChild(reset);
      }
      return wrap;
    }
    ignoreEvent() {
      return true;
    }
  }

  class ReadoutWidget extends WidgetType {
    constructor(
      readonly handleId: string,
      readonly kind: 'vec3' | 'transform'
    ) {
      super();
    }
    eq(o: ReadoutWidget) {
      return o.handleId === this.handleId && o.kind === this.kind;
    }
    toDOM(view: EditorView) {
      const el = document.createElement('span');
      el.className = 'cm-gizmo-readout';
      el.dataset.gizmoId = this.handleId;
      el.textContent = readoutText(view.state.field(valuesField).get(this.handleId));
      if (this.kind === 'vec3') {
        el.classList.add('cm-gizmo-readout-editable');
        el.title = 'click to edit';
        el.addEventListener('click', () => this.beginEdit(el, view));
      }
      return el;
    }
    private beginEdit(el: HTMLElement, view: EditorView) {
      if (el.classList.contains('cm-gizmo-readout-editing')) return;
      const cur = view.state.field(valuesField).get(this.handleId)?.vec3 ?? [0, 0, 0];
      el.classList.add('cm-gizmo-readout-editing');
      el.textContent = '';

      let finished = false;
      const finish = (apply: boolean) => {
        if (finished) return; // re-render removes the focused input → re-entrant focusout
        finished = true;
        el.classList.remove('cm-gizmo-readout-editing');
        if (apply) {
          const vals = inputs.map(inp => evalMathExpr(inp.value));
          if (vals.every(n => n !== null)) hooks.setHandleVec3(this.handleId, [vals[0]!, vals[1]!, vals[2]!]);
        }
        el.textContent = readoutText(view.state.field(valuesField).get(this.handleId));
      };

      const inputs = [0, 1, 2].map(i => {
        const inp = document.createElement('input');
        inp.className = 'cm-gizmo-input';
        inp.value = editNum(cur[i]);
        inp.addEventListener('keydown', e => {
          e.stopPropagation();
          if (e.key === 'Enter') {
            e.preventDefault();
            finish(true);
          } else if (e.key === 'Escape') {
            e.preventDefault();
            finish(false);
          }
        });
        return inp;
      });

      el.appendChild(document.createTextNode('⟨'));
      inputs.forEach((inp, i) => {
        el.appendChild(inp);
        el.appendChild(document.createTextNode(i < 2 ? ', ' : '⟩'));
      });
      el.addEventListener('focusout', e => {
        if (!el.contains(e.relatedTarget as Node | null)) finish(true);
      });
      inputs[0].focus();
      inputs[0].select();
    }
    ignoreEvent() {
      return true;
    }
  }

  const buildDeco = (sites: GizmoSite[], armed: string | null): DecorationSet => {
    const ranges = [];
    for (const s of sites) {
      if (s.dynamic) continue;
      ranges.push(
        Decoration.widget({
          widget: new ChipWidget(s.handleId, s.kind, armed === s.handleId),
          side: -1,
        }).range(s.callFrom)
      );
      if (armed === s.handleId) {
        ranges.push(Decoration.mark({ class: 'cm-gizmo-armed' }).range(s.callFrom, s.callTo));
      }
      ranges.push(
        Decoration.widget({ widget: new ReadoutWidget(s.handleId, s.kind), side: 1 }).range(s.callTo)
      );
    }
    return Decoration.set(ranges, true);
  };

  const sitesField = StateField.define<{ sites: GizmoSite[]; deco: DecorationSet }>({
    create: state => {
      const sites = scanGizmoSites(state);
      return { sites, deco: buildDeco(sites, hooks.getArmedHandleId()) };
    },
    update: (value, tr) => {
      const armedChanged = tr.effects.some(e => e.is(setArmedEffect));
      if (!tr.docChanged && !armedChanged) return value;
      const sites = tr.docChanged ? scanGizmoSites(tr.state) : value.sites;
      return { sites, deco: buildDeco(sites, hooks.getArmedHandleId()) };
    },
    provide: f => EditorView.decorations.from(f, v => v.deco),
  });

  // Patch readout text at drag-frame rate without rebuilding decorations. Skip readouts
  // mid-edit so a concurrent run's values don't blow away the input boxes.
  const liveRepaint = ViewPlugin.fromClass(
    class {
      update(u: ViewUpdate) {
        const hit = u.transactions.some(tr =>
          tr.effects.some(e => e.is(setValuesEffect) || e.is(patchValueEffect))
        );
        if (!hit) return;
        const values = u.view.state.field(valuesField);
        for (const el of u.view.contentDOM.querySelectorAll<HTMLElement>('.cm-gizmo-readout')) {
          if (el.classList.contains('cm-gizmo-readout-editing')) continue;
          el.textContent = readoutText(values.get(el.dataset.gizmoId ?? ''));
        }
      }
    }
  );

  return [valuesField, sitesField, liveRepaint, gizmoTheme];
};

const gizmoTheme = EditorView.baseTheme({
  '.cm-gizmo-chip-wrap': { whiteSpace: 'nowrap' },
  '.cm-gizmo-chip': {
    cursor: 'pointer',
    color: '#83a598',
    padding: '0 2px',
    userSelect: 'none',
  },
  '.cm-gizmo-chip:hover': { color: '#8ec07c' },
  '.cm-gizmo-chip-armed': {
    color: '#fabd2f',
    fontWeight: 'bold',
  },
  '.cm-gizmo-reset': {
    cursor: 'pointer',
    color: '#b45706ff',
    padding: '0 2px',
    marginLeft: '1px',
    userSelect: 'none',
  },
  '.cm-gizmo-reset:hover': { color: '#fe8019' },
  '.cm-gizmo-armed': {
    backgroundColor: 'rgba(250, 189, 47, 0.18)',
  },
  '.cm-gizmo-readout': {
    color: '#928374',
    fontSize: '0.85em',
    padding: '0 2px',
    userSelect: 'none',
  },
  '.cm-gizmo-readout-editable': { cursor: 'pointer' },
  '.cm-gizmo-readout-editable:hover': { color: '#d5c4a1' },
  '.cm-gizmo-input': {
    width: '64px',
    margin: '0 1px',
    fontSize: '10px',
    fontFamily: 'inherit',
    color: '#ebdbb2',
    background: '#3c3836',
    border: '1px solid #665c54',
    padding: '0 3px',
  },
});
