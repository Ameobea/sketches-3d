import type { BuiltinDocs, ParamDocs, SignatureDocs } from './analysisClient';

export interface DocsRenderOpts {
  activeSignature: number;
  /** Per-signature index of the param to highlight. */
  activeParams?: (number | null)[];
  /** Per-signature: whether the args typed so far could still fit it. */
  compatible?: boolean[];
  /** Signature help mode: signature line + the active param's docs only. */
  compact?: boolean;
  onSelectSignature?: (ix: number) => void;
}

const el = <K extends keyof HTMLElementTagNameMap>(tag: K, cls: string, text?: string) => {
  const e = document.createElement(tag);
  e.className = cls;
  if (text !== undefined) {
    e.textContent = text;
  }
  return e;
};

/** Inline `code` spans and **bold** runs; newlines become line breaks. */
export const renderRichText = (text: string): DocumentFragment => {
  const frag = document.createDocumentFragment();
  let last = 0;
  for (const m of text.matchAll(/`([^`]+)`|\*\*([^*]+)\*\*|\n/g)) {
    if (m.index > last) {
      frag.append(text.slice(last, m.index));
    }
    if (m[1] !== undefined) {
      frag.append(el('code', '', m[1]));
    } else if (m[2] !== undefined) {
      frag.append(el('strong', '', m[2]));
    } else {
      frag.append(document.createElement('br'));
    }
    last = m.index + m[0].length;
  }
  if (last < text.length) {
    frag.append(text.slice(last));
  }
  return frag;
};

const renderParamHead = (p: ParamDocs, withDefault: boolean): Node[] => {
  const nodes: Node[] = [
    el('span', 'cm-docs-param-name', p.name || '…'),
    el('span', 'cm-docs-type', `: ${p.ty}`),
  ];
  if (withDefault && p.default != null) {
    nodes.push(el('span', 'cm-docs-default', ` = ${p.default}`));
  }
  return nodes;
};

const renderSignatureLine = (
  name: string,
  sig: SignatureDocs,
  activeParam: number | null,
  withDefaults: boolean
) => {
  const line = el('div', 'cm-docs-sig');
  line.append(el('span', 'cm-docs-fn', name), '(');
  sig.params.forEach((p, i) => {
    if (i > 0) {
      line.append(', ');
    }
    const span = el('span', i === activeParam ? 'cm-docs-param cm-docs-param-active' : 'cm-docs-param');
    span.append(...renderParamHead(p, withDefaults));
    line.append(span);
  });
  line.append(')');
  if (sig.return_type) {
    line.append(el('span', 'cm-docs-ret', ` → ${sig.return_type}`));
  }
  return line;
};

const renderNav = (ix: number, count: number, keyHint: boolean, onSelect: (ix: number) => void) => {
  const nav = el('span', 'cm-docs-nav');
  nav.title = keyHint ? 'Switch overload (alt+↑ / alt+↓)' : 'Switch overload';
  const btn = (label: string, delta: number) => {
    const b = el('button', 'cm-docs-nav-btn', label);
    b.type = 'button';
    b.tabIndex = -1;
    b.addEventListener('mousedown', e => e.preventDefault());
    b.addEventListener('click', () => onSelect((ix + delta + count) % count));
    return b;
  };
  nav.append(btn('‹', -1), el('span', 'cm-docs-nav-count', `${ix + 1}/${count}`), btn('›', 1));
  return nav;
};

const renderDescription = (text: string, cls: string) => {
  const d = el('div', cls);
  d.append(renderRichText(text));
  return d;
};

export const renderDocsInto = (root: HTMLElement, docs: BuiltinDocs, opts: DocsRenderOpts) => {
  root.replaceChildren();
  const count = docs.signatures.length;
  if (count === 0) {
    root.append(el('div', 'cm-docs-sig', docs.name));
    return;
  }
  const ix = Math.min(opts.activeSignature, count - 1);
  const sig = docs.signatures[ix];
  const activeParam = opts.activeParams?.[ix] ?? null;

  const header = el('div', 'cm-docs-header');
  if (count > 1) {
    header.append(renderNav(ix, count, !!opts.compact, i => opts.onSelectSignature?.(i)));
  }
  // the full view lists defaults per param below; keep its header compact
  const sigLine = renderSignatureLine(docs.name, sig, activeParam, !!opts.compact);
  if (opts.compatible?.[ix] === false) {
    sigLine.classList.add('cm-docs-sig-incompatible');
    sigLine.title = "The arguments given so far don't fit this overload";
  }
  header.append(sigLine);
  root.append(header);

  if (opts.compact) {
    const p = activeParam === null ? null : sig.params[activeParam];
    if (p) {
      const line = el('div', 'cm-docs-active-param');
      line.append(...renderParamHead(p, false));
      if (p.description) {
        line.append(' — ', renderRichText(p.description));
      }
      root.append(line);
    } else if (sig.description) {
      root.append(renderDescription(sig.description, 'cm-docs-desc cm-docs-desc-dim'));
    }
    return;
  }

  root.append(el('div', 'cm-docs-meta', docs.module ? `builtin · ${docs.module}` : 'builtin'));
  if (sig.description) {
    root.append(renderDescription(sig.description, 'cm-docs-desc'));
  }
  if (sig.params.length > 0 && sig.params[0].name) {
    const list = el('ul', 'cm-docs-params');
    for (const p of sig.params) {
      const li = el('li', '');
      li.append(...renderParamHead(p, true));
      if (p.description) {
        li.append(' — ', renderRichText(p.description));
      }
      list.append(li);
    }
    root.append(list);
  }
};
