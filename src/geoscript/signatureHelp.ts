import { Prec, StateEffect, StateField, type Extension } from '@codemirror/state';
import {
  EditorView,
  keymap,
  showTooltip,
  ViewPlugin,
  type TooltipView,
  type ViewUpdate,
} from '@codemirror/view';

import type { SignatureHelp } from './analysisClient';
import { getClient, posToLc, type GetAmbientSource, type GetIncludePrelude } from './analysisShared';
import { renderDocsInto } from './builtinDocs';

interface SigHelpState {
  help: SignatureHelp | null;
  /** User-picked overload for the current call; `null` follows the analysis' choice. */
  override: number | null;
  /** Call dismissed with Escape; stays hidden until the cursor leaves it. */
  dismissedKey: string | null;
  /** Start of the cursor's line; the tooltip sits above it. */
  anchor: number;
}

const callKey = (help: SignatureHelp | null) => (help ? `${help.call_line}:${help.call_col}` : null);
const isShown = (s: SigHelpState) => s.help !== null && callKey(s.help) !== s.dismissedKey;
const activeSignature = (s: SigHelpState) => s.override ?? s.help!.active_signature;

const setHelp = StateEffect.define<{ help: SignatureHelp | null; explicit: boolean }>();
const selectSignature = StateEffect.define<number>();
const dismissHelp = StateEffect.define();
const requestHelp = StateEffect.define();

/** Everything the tooltip's DOM depends on; responses are fresh objects, so identity won't do. */
const renderKey = (s: SigHelpState) =>
  s.help
    ? JSON.stringify([
        callKey(s.help),
        s.help.docs.name,
        activeSignature(s),
        s.help.active_params,
        s.help.compatible,
      ])
    : '';

const createTooltip = (view: EditorView): TooltipView => {
  const dom = document.createElement('div');
  dom.className = 'cm-tooltip-sighelp';
  // clicks inside (text, scrollbar) must not blur the editor, which would close the help
  dom.addEventListener('mousedown', e => e.preventDefault());
  const body = document.createElement('div');
  body.className = 'cm-docs';
  dom.append(body);

  let rendered = '';
  const render = () => {
    const s = view.state.field(sigHelpField);
    const key = renderKey(s);
    if (!s.help || key === rendered) {
      return;
    }
    rendered = key;
    renderDocsInto(body, s.help.docs, {
      activeSignature: activeSignature(s),
      activeParams: s.help.active_params,
      compatible: s.help.compatible,
      compact: true,
      onSelectSignature: ix => view.dispatch({ effects: selectSignature.of(ix) }),
    });
  };
  render();
  return { dom, update: render };
};

const sigHelpField = StateField.define<SigHelpState>({
  create: () => ({ help: null, override: null, dismissedKey: null, anchor: 0 }),
  update(s, tr) {
    let next = s;
    for (const e of tr.effects) {
      if (e.is(setHelp)) {
        const { help, explicit } = e.value;
        const sameCall = help !== null && callKey(help) === callKey(next.help);
        next = {
          ...next,
          help,
          override: sameCall ? next.override : null,
          dismissedKey: explicit || help === null ? null : next.dismissedKey,
        };
      } else if (e.is(selectSignature)) {
        next = { ...next, override: e.value };
      } else if (e.is(dismissHelp)) {
        next = { ...next, dismissedKey: callKey(next.help) };
      }
    }
    if (next.help) {
      const anchor = tr.state.doc.lineAt(tr.state.selection.main.head).from;
      if (anchor !== next.anchor) {
        next = { ...next, anchor };
      }
    }
    return next;
  },
  provide: f =>
    showTooltip.from(f, s => (isShown(s) ? { pos: s.anchor, above: true, create: createTooltip } : null)),
});

const buildRequestPlugin = (getIncludePrelude: GetIncludePrelude, getAmbientSource: GetAmbientSource) =>
  ViewPlugin.fromClass(
    class {
      private timer: ReturnType<typeof setTimeout> | null = null;
      private seq = 0;
      private explicit = false;

      constructor(private view: EditorView) {}

      update(u: ViewUpdate) {
        const explicit = u.transactions.some(tr => tr.effects.some(e => e.is(requestHelp)));
        // Typing opens help; plain cursor movement only updates it while it's already open.
        const relevant = explicit || u.docChanged || (u.selectionSet && isShown(u.state.field(sigHelpField)));
        if (relevant) {
          this.explicit ||= explicit;
          this.schedule();
        }
      }

      private schedule() {
        if (this.timer !== null) {
          clearTimeout(this.timer);
        }
        this.timer = setTimeout(() => void this.request(), 60);
      }

      private async request() {
        this.timer = null;
        const explicit = this.explicit;
        this.explicit = false;
        const seq = ++this.seq;
        const { state } = this.view;
        const head = state.selection.main.head;
        const [line, col] = posToLc(state.doc, head);
        let help: SignatureHelp | null;
        try {
          const client = await getClient();
          help = await client.signatureHelp(
            state.doc.toString(),
            line,
            col,
            getIncludePrelude(),
            getAmbientSource()
          );
        } catch {
          return;
        }
        if (seq !== this.seq) {
          return;
        }
        if (this.view.state.doc !== state.doc || this.view.state.selection.main.head !== head) {
          this.explicit ||= explicit;
          this.schedule();
          return;
        }
        if (help !== null || this.view.state.field(sigHelpField).help !== null) {
          this.view.dispatch({ effects: setHelp.of({ help, explicit }) });
        }
      }

      destroy() {
        if (this.timer !== null) {
          clearTimeout(this.timer);
        }
        this.seq++;
      }
    }
  );

const cycleSignature = (view: EditorView, delta: number): boolean => {
  const s = view.state.field(sigHelpField);
  const count = s.help?.docs.signatures.length ?? 0;
  if (!isShown(s) || count < 2) {
    return false;
  }
  view.dispatch({ effects: selectSignature.of((activeSignature(s) + delta + count) % count) });
  return true;
};

const sigHelpKeymap = Prec.high(
  keymap.of([
    {
      // the autocomplete keymap sits at Prec.highest, so an open completion takes Escape first
      key: 'Escape',
      run: view => {
        if (!isShown(view.state.field(sigHelpField))) {
          return false;
        }
        view.dispatch({ effects: dismissHelp.of(null) });
        return true;
      },
    },
    { key: 'Alt-ArrowDown', run: view => cycleSignature(view, 1) },
    { key: 'Alt-ArrowUp', run: view => cycleSignature(view, -1) },
    {
      key: 'Ctrl-Shift-Space',
      run: view => {
        view.dispatch({ effects: requestHelp.of(null) });
        return true;
      },
    },
  ])
);

const closeOnBlur = EditorView.domEventHandlers({
  blur: (_e, view) => {
    if (view.state.field(sigHelpField).help) {
      view.dispatch({ effects: setHelp.of({ help: null, explicit: false }) });
    }
    return false;
  },
});

export const buildSignatureHelpExtension = (
  getIncludePrelude: GetIncludePrelude,
  getAmbientSource: GetAmbientSource
): Extension[] => [
  sigHelpField,
  buildRequestPlugin(getIncludePrelude, getAmbientSource),
  sigHelpKeymap,
  closeOnBlur,
];
