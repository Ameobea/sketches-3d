import { type Extension } from '@codemirror/state';
import { EditorView, hoverTooltip, keymap } from '@codemirror/view';
import { linter, type Diagnostic } from '@codemirror/lint';
import { autocompletion, type CompletionContext, type CompletionResult } from '@codemirror/autocomplete';
import { EditorSelection } from '@codemirror/state';

import type { AnalysisClient } from './analysisClient';
import type { Text } from '@codemirror/state';

let clientPromise: Promise<AnalysisClient> | null = null;

const getClient = (): Promise<AnalysisClient> => {
  if (!clientPromise) {
    clientPromise = import('./analysisClient').then(mod => mod.getAnalysisClient());
  }
  return clientPromise;
};

/** Convert 1-based line/col from analysis to a CM6 character offset. */
const lcToPos = (doc: Text, line: number, col: number): number => {
  if (line < 1 || line > doc.lines) return 0;
  const lineObj = doc.line(line);
  return Math.min(lineObj.from + col - 1, lineObj.to);
};

/** Convert a CM6 character offset to 1-based line/col for the analysis API. */
const posToLc = (doc: Text, pos: number): [line: number, col: number] => {
  const lineObj = doc.lineAt(pos);
  return [lineObj.number, pos - lineObj.from + 1];
};

type GetIncludePrelude = () => boolean;

// ---------------------------------------------------------------------------
// Semantic linter — reports undefined variables, wrong arg counts, etc.
// ---------------------------------------------------------------------------

const buildSemanticLinter = (getIncludePrelude: GetIncludePrelude): Extension =>
  linter(
    async view => {
      let client: AnalysisClient;
      try {
        client = await getClient();
      } catch {
        return [];
      }

      const src = view.state.doc.toString();
      const result = await client.analyze(src, getIncludePrelude());

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
// Hover tooltips — builtin signatures, variable info
// ---------------------------------------------------------------------------

const renderHoverContent = (content: string): string =>
  content
    .split('\n')
    .map(line => {
      line = line.replace(/`([^`]+)`/g, '<code>$1</code>');
      line = line.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
      return line;
    })
    .join('<br>');

const buildHoverExtension = (getIncludePrelude: GetIncludePrelude): Extension =>
  hoverTooltip(async (view, pos) => {
    let client: AnalysisClient;
    try {
      client = await getClient();
    } catch {
      return null;
    }

    const src = view.state.doc.toString();
    const [line, col] = posToLc(view.state.doc, pos);
    const info = await client.hover(src, line, col, getIncludePrelude());
    if (!info) return null;

    const from = lcToPos(view.state.doc, info.start_line, info.start_col);
    const to = lcToPos(view.state.doc, info.end_line, info.end_col);

    return {
      pos: from,
      end: to,
      above: true,
      create: () => {
        const dom = document.createElement('div');
        dom.className = 'cm-analysis-hover';
        dom.innerHTML = renderHoverContent(info.content);
        return { dom };
      },
    };
  });

// ---------------------------------------------------------------------------
// Autocomplete
// ---------------------------------------------------------------------------

const buildCompletionSource =
  (getIncludePrelude: GetIncludePrelude) =>
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
    const items = await client.completions(src, line, col, getIncludePrelude());

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

const buildCompletionExtension = (getIncludePrelude: GetIncludePrelude): Extension =>
  autocompletion({
    override: [buildCompletionSource(getIncludePrelude)],
    activateOnTyping: true,
  });

// ---------------------------------------------------------------------------
// Go-to-definition (F12 / Ctrl-click style)
// ---------------------------------------------------------------------------

const buildGoToDefinitionKeymap = (getIncludePrelude: GetIncludePrelude): Extension =>
  keymap.of([
    {
      key: 'F12',
      run: view => {
        goToDefinition(view, getIncludePrelude);
        return true;
      },
    },
  ]);

const goToDefinition = async (view: EditorView, getIncludePrelude: GetIncludePrelude) => {
  let client: AnalysisClient;
  try {
    client = await getClient();
  } catch {
    return;
  }

  const pos = view.state.selection.main.head;
  const src = view.state.doc.toString();
  const [line, col] = posToLc(view.state.doc, pos);

  const def = await client.gotoDefinition(src, line, col, getIncludePrelude());
  if (!def) return;

  const targetPos = lcToPos(view.state.doc, def.start_line, def.start_col);
  view.dispatch({
    selection: EditorSelection.cursor(targetPos),
    effects: EditorView.scrollIntoView(targetPos, { y: 'center' }),
  });
  view.focus();
};

// ---------------------------------------------------------------------------
// Theme for hover tooltips
// ---------------------------------------------------------------------------

const hoverTheme = EditorView.baseTheme({
  '.cm-tooltip': {
    border: '1px solid #555 !important',
    borderRadius: '0 !important',
  },
  '.cm-analysis-hover': {
    padding: '4px 8px',
    fontSize: '13px',
    fontFamily: "'IBM Plex Mono', 'Hack', 'Roboto Mono', monospace",
    lineHeight: '1.5',
    maxWidth: '500px',
    color: '#ccc',
    background: '#1a1a1a',
  },
  '.cm-analysis-hover code': {
    background: 'rgba(255,255,255,0.08)',
    padding: '1px 3px',
    fontSize: '12px',
  },
  '.cm-analysis-hover strong': {
    color: '#e0e0e0',
    fontWeight: '600',
  },
  '.cm-tooltip-autocomplete': {
    borderRadius: '0 !important',
  },
  '.cm-tooltip-autocomplete > ul': {
    fontFamily: "'IBM Plex Mono', 'Hack', 'Roboto Mono', monospace",
    fontSize: '13px',
  },
  '.cm-tooltip-autocomplete > ul > li': {
    borderRadius: '0 !important',
  },
  '.cm-completionInfo': {
    borderRadius: '0 !important',
    borderLeft: '1px solid #555 !important',
    padding: '4px 8px',
    fontFamily: "'IBM Plex Mono', 'Hack', 'Roboto Mono', monospace",
    fontSize: '12px',
  },
  '.cm-diagnostic': {
    borderRadius: '0 !important',
  },
});

// ---------------------------------------------------------------------------
// Public API — builds all analysis extensions as a single array
// ---------------------------------------------------------------------------

export const buildAnalysisExtensions = (getIncludePrelude: GetIncludePrelude): Extension[] => [
  buildSemanticLinter(getIncludePrelude),
  buildHoverExtension(getIncludePrelude),
  buildCompletionExtension(getIncludePrelude),
  buildGoToDefinitionKeymap(getIncludePrelude),
  hoverTheme,
];
