/**
 * Editor markers for texel bodies the auto-vectorizer fell back on: a dotted underline from
 * the reported location to the end of its line, with the bail reason as the hover title.
 * Pushed per run like the gizmo readouts; the run-output drawer carries the full list.
 */

import { StateEffect, StateField, type Extension, type Text } from '@codemirror/state';
import { Decoration, EditorView, type DecorationSet } from '@codemirror/view';

export interface VectorizeMarker {
  /** 1-based, already adjusted into the editor's own line space. */
  line: number;
  col: number;
  reason: string;
}

const setMarkersEffect = StateEffect.define<VectorizeMarker[]>();

const build = (doc: Text, markers: VectorizeMarker[]): DecorationSet => {
  const ranges = markers
    .filter(m => m.line >= 1 && m.line <= doc.lines)
    .map(m => {
      const l = doc.line(m.line);
      const from = Math.min(l.from + Math.max(m.col - 1, 0), l.to);
      const to = Math.max(from + 1, l.to);
      return Decoration.mark({
        class: 'cm-vectorize-bail',
        attributes: { title: `not vectorized: ${m.reason}` },
      }).range(from, Math.min(to, doc.length));
    })
    .sort((a, b) => a.from - b.from);
  return Decoration.set(ranges, true);
};

const markersField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update: (deco, tr) => {
    for (const e of tr.effects) {
      if (e.is(setMarkersEffect)) return build(tr.newDoc, e.value);
    }
    // Locations are stale once the text moves; drop rather than mislead.
    return tr.docChanged ? Decoration.none : deco;
  },
  provide: f => EditorView.decorations.from(f),
});

const theme = EditorView.baseTheme({
  '.cm-vectorize-bail': {
    textDecoration: 'underline dotted #e0a030',
    textDecorationSkipInk: 'none',
    textUnderlineOffset: '3px',
  },
});

export const buildVectorizeMarkerExtensions = (): Extension[] => [markersField, theme];

export const pushVectorizeMarkers = (view: EditorView, markers: VectorizeMarker[]): void =>
  view.dispatch({ effects: setMarkersEffect.of(markers) });
