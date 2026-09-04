import { autocompletion, type CompletionContext, type CompletionResult } from '@codemirror/autocomplete';
import { linter, type Diagnostic } from '@codemirror/lint';
import { EditorSelection, StateEffect, StateField, type Extension } from '@codemirror/state';
import {
  Decoration,
  EditorView,
  hoverTooltip,
  keymap,
  ViewPlugin,
  type DecorationSet,
  type ViewUpdate,
} from '@codemirror/view';

import type { AnalysisClient } from './analysisClient';
import { getClient, lcToPos, posToLc, type GetAmbientSource, type GetIncludePrelude } from './analysisShared';
import { renderDocsInto, renderRichText } from './builtinDocs';
import { buildSignatureHelpExtension } from './signatureHelp';

// ---------------------------------------------------------------------------
// Semantic linter — reports undefined variables, wrong arg counts, etc.
// ---------------------------------------------------------------------------

const buildSemanticLinter = (
  getIncludePrelude: GetIncludePrelude,
  getAmbientSource: GetAmbientSource
): Extension =>
  linter(
    async view => {
      let client: AnalysisClient;
      try {
        client = await getClient();
      } catch {
        return [];
      }

      const src = view.state.doc.toString();
      const result = await client.analyze(src, getIncludePrelude(), getAmbientSource());

      const diagnostics: Diagnostic[] = [];
      for (const d of result.diagnostics) {
        const from = lcToPos(view.state.doc, d.start_line, d.start_col);
        let to = lcToPos(view.state.doc, d.end_line, d.end_col);
        if (from === 0 && to === 0) continue;
        // Ensure the span is at least 1 char wide so the squiggle is visible
        if (to <= from) to = Math.min(from + 1, view.state.doc.length);

        diagnostics.push({
          from,
          to,
          severity: d.severity === 'Error' ? 'error' : d.severity === 'Warning' ? 'warning' : 'info',
          message: d.message,
        });
      }
      return diagnostics;
    },
    { delay: 400 }
  );

// ---------------------------------------------------------------------------
// Hover tooltips — builtin docs (with overload paging), variable info
// ---------------------------------------------------------------------------

const buildHoverExtension = (
  getIncludePrelude: GetIncludePrelude,
  getAmbientSource: GetAmbientSource
): Extension =>
  hoverTooltip(async (view, pos, side) => {
    let client: AnalysisClient;
    try {
      client = await getClient();
    } catch {
      return null;
    }

    const src = view.state.doc.toString();
    // `side < 0`: the pointer is over the char before `pos`
    const [line, col] = posToLc(view.state.doc, side < 0 && pos > 0 ? pos - 1 : pos);
    const info = await client.hover(src, line, col, getIncludePrelude(), getAmbientSource());
    if (!info) return null;

    const from = lcToPos(view.state.doc, info.start_line, info.start_col);
    const to = lcToPos(view.state.doc, info.end_line, info.end_col);

    return {
      pos: from,
      end: to,
      above: true,
      create: () => {
        const dom = document.createElement('div');
        dom.className = 'cm-docs cm-analysis-hover';
        const docs = info.builtin;
        if (docs) {
          const render = (ix: number) =>
            renderDocsInto(dom, docs, { activeSignature: ix, onSelectSignature: render });
          render(info.active_signature ?? 0);
        } else {
          dom.append(renderRichText(info.content));
        }
        return { dom };
      },
    };
  });

// ---------------------------------------------------------------------------
// Autocomplete
// ---------------------------------------------------------------------------

const buildCompletionSource =
  (getIncludePrelude: GetIncludePrelude, getAmbientSource: GetAmbientSource) =>
  async (context: CompletionContext): Promise<CompletionResult | null> => {
    const word = context.matchBefore(/\w*/);
    if (!context.explicit && (!word || word.text.length < 1)) return null;

    let client: AnalysisClient;
    try {
      client = await getClient();
    } catch {
      return null;
    }

    const src = context.state.doc.toString();
    const [line, col] = posToLc(context.state.doc, context.pos);
    const items = await client.completions(src, line, col, getIncludePrelude(), getAmbientSource());

    return {
      from: word ? word.from : context.pos,
      options: items.map(item => ({
        label: item.label,
        type: item.kind, // "function" | "variable" | "keyword" — CM6 uses these for icons
        detail: item.detail || undefined,
        info: item.info || undefined,
      })),
      validFor: /^\w*$/,
    };
  };

const buildCompletionExtension = (
  getIncludePrelude: GetIncludePrelude,
  getAmbientSource: GetAmbientSource
): Extension =>
  autocompletion({
    override: [buildCompletionSource(getIncludePrelude, getAmbientSource)],
    activateOnTyping: true,
  });

// ---------------------------------------------------------------------------
// Go-to-definition: F12, ctrl-click (option-click on mac); modifier-hover underlines the target
// ---------------------------------------------------------------------------

const isMac = typeof navigator !== 'undefined' && /Mac|iP(hone|ad)/.test(navigator.platform);
const hasGotoModifier = (e: MouseEvent | KeyboardEvent) => (isMac ? e.altKey : e.ctrlKey);

const findDefinition = async (
  view: EditorView,
  pos: number,
  getIncludePrelude: GetIncludePrelude,
  getAmbientSource: GetAmbientSource
): Promise<number | null> => {
  let client: AnalysisClient;
  try {
    client = await getClient();
  } catch {
    return null;
  }
  const { doc } = view.state;
  const [line, col] = posToLc(doc, pos);
  const def = await client.gotoDefinition(doc.toString(), line, col, getIncludePrelude(), getAmbientSource());
  return def ? lcToPos(doc, def.start_line, def.start_col) : null;
};

const linkMark = Decoration.mark({ class: 'cm-goto-link' });
const setLinkRange = StateEffect.define<{ from: number; to: number } | null>();

const linkField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(deco, tr) {
    for (const e of tr.effects) {
      if (e.is(setLinkRange)) {
        return e.value ? Decoration.set([linkMark.range(e.value.from, e.value.to)]) : Decoration.none;
      }
    }
    return tr.docChanged ? Decoration.none : deco;
  },
  provide: f => EditorView.decorations.from(f),
});

/** The word last looked up under the modifier-held pointer; `target` is `undefined` while pending. */
interface Probe {
  from: number;
  to: number;
  target: number | null | undefined;
}

const buildGoToDefinition = (
  getIncludePrelude: GetIncludePrelude,
  getAmbientSource: GetAmbientSource
): Extension => {
  const plugin = ViewPlugin.fromClass(
    class {
      probed: Probe | null = null;

      constructor(readonly view: EditorView) {}

      update(u: ViewUpdate) {
        if (u.docChanged) {
          this.probed = null;
        }
      }

      probeFor(word: { from: number; to: number }): Probe | null {
        const p = this.probed;
        return p && p.from === word.from && p.to === word.to ? p : null;
      }

      wordAtCoords(e: MouseEvent) {
        const pos = this.view.posAtCoords({ x: e.clientX, y: e.clientY });
        return pos === null ? null : this.view.state.wordAt(pos);
      }

      clear() {
        const hadLink = typeof this.probed?.target === 'number';
        this.probed = null;
        if (hadLink) {
          this.view.dispatch({ effects: setLinkRange.of(null) });
        }
      }

      async probe(word: { from: number; to: number }) {
        if (this.probeFor(word)) {
          return;
        }
        this.clear();
        const probed: Probe = { from: word.from, to: word.to, target: undefined };
        this.probed = probed;
        const { doc } = this.view.state;
        const target = await findDefinition(this.view, word.from, getIncludePrelude, getAmbientSource);
        if (this.probed !== probed || this.view.state.doc !== doc) {
          return;
        }
        probed.target = target;
        if (target !== null) {
          this.view.dispatch({ effects: setLinkRange.of({ from: word.from, to: word.to }) });
        }
      }

      /** Jump to the definition of the word at `pos`, or just place the cursor there if none. */
      async goto(pos: number) {
        const { state } = this.view;
        const word = state.wordAt(pos);
        if (!word) {
          return;
        }
        const known = this.probeFor(word)?.target;
        const target =
          known !== undefined
            ? known
            : await findDefinition(this.view, word.from, getIncludePrelude, getAmbientSource);
        if (this.view.state.doc !== state.doc) {
          return;
        }
        const dest = target ?? pos;
        this.view.dispatch({
          selection: EditorSelection.cursor(dest),
          effects: target === null ? [] : EditorView.scrollIntoView(dest, { y: 'center' }),
        });
        this.view.focus();
      }
    },
    {
      eventHandlers: {
        mousedown(e) {
          if (e.button !== 0 || !hasGotoModifier(e)) {
            return false;
          }
          const pos = this.view.posAtCoords({ x: e.clientX, y: e.clientY });
          const word = pos === null ? null : this.view.state.wordAt(pos);
          // a probed word without a definition isn't a link: leave CM's own modifier gestures alone
          if (!word || this.probeFor(word)?.target === null) {
            return false;
          }
          void this.goto(pos!);
          return true;
        },
        mousemove(e) {
          if (!hasGotoModifier(e)) {
            this.clear();
            return false;
          }
          const word = this.wordAtCoords(e);
          if (word) {
            void this.probe(word);
          } else {
            this.clear();
          }
          return false;
        },
        keyup(e) {
          if (!hasGotoModifier(e)) {
            this.clear();
          }
          return false;
        },
        mouseleave() {
          this.clear();
          return false;
        },
      },
    }
  );

  return [
    linkField,
    plugin,
    keymap.of([
      {
        key: 'F12',
        run: view => {
          void view.plugin(plugin)?.goto(view.state.selection.main.head);
          return true;
        },
      },
    ]),
  ];
};

// ---------------------------------------------------------------------------
// Theme for tooltips and docs
// ---------------------------------------------------------------------------

const mono = "'IBM Plex Mono', 'Hack', 'Roboto Mono', monospace";

const analysisTheme = EditorView.baseTheme({
  '.cm-tooltip': {
    border: '1px solid #555 !important',
    borderRadius: '0 !important',
  },
  // CM shrinks a tooltip that doesn't fit; the docs body scrolls inside it
  '.cm-tooltip.cm-tooltip-hover': { maxHeight: '50vh', overflowY: 'auto' },
  '.cm-tooltip.cm-tooltip-sighelp': { maxHeight: '40vh', overflowY: 'auto' },
  '.cm-docs': {
    padding: '5px 9px',
    fontSize: '13px',
    fontFamily: mono,
    lineHeight: '1.45',
    maxWidth: '560px',
    color: '#ccc',
    background: '#1a1a1a',
  },
  '.cm-docs code': {
    background: 'rgba(255,255,255,0.08)',
    padding: '1px 3px',
    fontSize: '12px',
  },
  '.cm-docs strong': {
    color: '#e0e0e0',
    fontWeight: '600',
  },
  '.cm-docs-header': { display: 'flex', alignItems: 'baseline', gap: '8px' },
  '.cm-docs-sig': { whiteSpace: 'pre-wrap', wordBreak: 'break-word', color: '#ddd' },
  '.cm-docs-fn': { color: '#f0f0f0', fontWeight: '600' },
  '.cm-docs-type, .cm-docs-default, .cm-docs-ret': { color: '#8a8a8a' },
  '.cm-docs-param-name': { color: '#e8e8e8', fontWeight: '600' },
  '.cm-docs-param-active': {
    background: 'rgba(255,255,255,0.16)',
    boxShadow: '0 0 0 1px rgba(255,255,255,0.25)',
  },
  '.cm-docs-param-active .cm-docs-type, .cm-docs-param-active .cm-docs-default': { color: '#bbb' },
  '.cm-docs-sig-incompatible': { opacity: '0.55' },
  '.cm-docs-nav': {
    display: 'inline-flex',
    alignItems: 'center',
    gap: '3px',
    flexShrink: '0',
    color: '#999',
    fontSize: '11px',
  },
  '.cm-docs-nav-btn': {
    background: 'transparent',
    border: '1px solid #555',
    borderRadius: '0',
    color: '#ccc',
    cursor: 'pointer',
    fontFamily: mono,
    fontSize: '12px',
    lineHeight: '1',
    padding: '1px 5px',
  },
  '.cm-docs-nav-btn:hover': { background: 'rgba(255,255,255,0.1)' },
  '.cm-docs-meta': { color: '#8a8a8a', fontSize: '11px', marginTop: '2px' },
  '.cm-docs-desc': { marginTop: '6px', color: '#bbb' },
  '.cm-docs-desc-dim': { color: '#8a8a8a', fontSize: '12px' },
  '.cm-docs-active-param': { marginTop: '5px', paddingTop: '5px', borderTop: '1px solid #333' },
  '.cm-docs-params': { margin: '6px 0 0', paddingLeft: '18px' },
  '.cm-docs-params li': { marginTop: '3px' },
  '.cm-goto-link': { textDecoration: 'underline', cursor: 'pointer' },
  '.cm-tooltip-autocomplete': {
    borderRadius: '0 !important',
  },
  '.cm-tooltip-autocomplete > ul': {
    fontFamily: mono,
    fontSize: '13px',
  },
  '.cm-tooltip-autocomplete > ul > li': {
    borderRadius: '0 !important',
  },
  '.cm-completionInfo': {
    borderRadius: '0 !important',
    borderLeft: '1px solid #555 !important',
    padding: '4px 8px',
    fontFamily: mono,
    fontSize: '12px',
  },
  '.cm-diagnostic': {
    borderRadius: '0 !important',
  },
});

// ---------------------------------------------------------------------------
// Public API — builds all analysis extensions as a single array
// ---------------------------------------------------------------------------

export const buildAnalysisExtensions = (
  getIncludePrelude: GetIncludePrelude,
  getAmbientSource: GetAmbientSource = () => ''
): Extension[] => [
  buildSemanticLinter(getIncludePrelude, getAmbientSource),
  buildHoverExtension(getIncludePrelude, getAmbientSource),
  buildCompletionExtension(getIncludePrelude, getAmbientSource),
  buildSignatureHelpExtension(getIncludePrelude, getAmbientSource),
  buildGoToDefinition(getIncludePrelude, getAmbientSource),
  analysisTheme,
];
