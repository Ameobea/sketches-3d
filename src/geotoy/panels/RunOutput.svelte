<script module lang="ts">
  export interface VectorizeRow {
    vectorized: boolean;
    /** Cache-hit run that handed off to the scalar path (vs. a compile-time bail). */
    aborted: boolean;
    /** Node name, or the bare module name when it isn't a node of the active tab. */
    where: string;
    nodeId: string | null;
    line: number;
    col: number;
    reason: string | null;
    /** Plan listing (+ per-step ms) when profiling was on for the run. */
    plan: string | null;
  }

  type PlanTok = { cls: string; text: string };
  const TOK =
    /(;.*$)|(\br\d+\b)|(\b(?:in\d*|uv)\.[xyzw]\b|\bu\d+(?:\.[xyzw])?\b|\bg\d+\b)|(-?\b\d+(?:\.\d+)?(?:e-?\d+)?\b)/g;
  const plainToks = (text: string): PlanTok[] => {
    const out: PlanTok[] = [];
    let last = 0;
    for (const m of text.matchAll(TOK)) {
      if (m.index > last) out.push({ cls: '', text: text.slice(last, m.index) });
      out.push({ cls: m[1] ? 'comment' : m[2] ? 'reg' : m[3] ? 'src' : 'num', text: m[0] });
      last = m.index + m[0].length;
    }
    if (last < text.length) out.push({ cls: '', text: text.slice(last) });
    return out;
  };
  /** Registers, plane sources, numbers, comments, section labels, and the kernel name. */
  export const tokenizePlanLine = (line: string): PlanTok[] => {
    const head = line.match(/^(\s*r\d+(?:,r\d+)*\s*=\s*)(\w+)/);
    if (head)
      return [...plainToks(head[1]), { cls: 'op', text: head[2] }, ...plainToks(line.slice(head[0].length))];
    const label = line.match(/^(\w+:)(.*)$/);
    if (label) return [{ cls: 'label', text: label[1] }, ...plainToks(label[2])];
    return plainToks(line);
  };

  export interface VectorizeSummary {
    rows: VectorizeRow[];
    /** The kill switch is on: nothing vectorizes, so nothing reports. */
    disabled: boolean;
    /** Drops the const-eval cache first so replayed modules re-run and report. */
    rerunUncached: () => void;
    reveal: (row: VectorizeRow) => void;
  }
</script>

<script lang="ts">
  import type { StatusMetric } from 'src/geotoy/modes/mode';

  let {
    err,
    metrics,
    vectorize = null,
  }: { err: string | null; metrics: StatusMetric[] | null; vectorize?: VectorizeSummary | null } = $props();

  const scalarRows = $derived(vectorize?.rows.filter(r => !r.vectorized) ?? []);
  /** Fallbacks first, then vectorized bodies (which carry the plan listings). */
  const shownRows = $derived([...scalarRows, ...(vectorize?.rows.filter(r => r.vectorized) ?? [])]);
  const rowKey = (row: VectorizeRow) => `${row.where}:${row.line}:${row.col}`;
  let openPlans = $state(new Set<string>());
  const togglePlan = (row: VectorizeRow) => {
    const next = new Set(openPlans);
    if (!next.delete(rowKey(row))) next.add(rowKey(row));
    openPlans = next;
  };
</script>

<div class="run-output">
  {#if err}
    <div class="error">{err}</div>
  {/if}
  {#if metrics}
    <ul>
      {#each metrics as m (m.label)}
        <li>
          <span class="label">{m.label}</span>
          <span class="value">{m.value}</span>
        </li>
      {/each}
    </ul>
  {/if}
  {#if vectorize}
    <div class="vectorize">
      <div class="vec-header">
        <span class="label">Vectorization</span>
        <span class="value">
          {#if vectorize.disabled}
            <span class="warn">disabled (view › vectorization)</span>
          {:else if vectorize.rows.length === 0}
            no texel bodies ran
          {:else}
            {vectorize.rows.length} texel {vectorize.rows.length === 1 ? 'body' : 'bodies'} ·
            {vectorize.rows.length - scalarRows.length} vectorized ·
            <span class:warn={scalarRows.length > 0}>{scalarRows.length} scalar</span>
          {/if}
        </span>
      </div>
      {#each shownRows as row (rowKey(row))}
        <div class="vec-row">
          <button
            class="vec-main"
            class:clickable={!!row.nodeId}
            disabled={!row.nodeId}
            onclick={() => vectorize.reveal(row)}
          >
            <span class={['glyph', row.vectorized ? 'ok' : row.aborted ? 'aborted' : 'bail']}>
              {row.vectorized ? '✓' : row.aborted ? '⚠' : '✗'}
            </span>
            <span class="where">{row.where}:{row.line}:{row.col}</span>
            {#if row.reason}<span class="reason">{row.reason}</span>{/if}
          </button>
          {#if row.plan}
            <button class="link" onclick={() => togglePlan(row)}>
              {openPlans.has(rowKey(row)) ? 'hide plan' : 'plan'}
            </button>
          {/if}
        </div>
        {#if row.plan && openPlans.has(rowKey(row))}
          <pre class="plan">{#each row.plan
              .trimEnd()
              .split('\n') as line, i (i)}{#each tokenizePlanLine(line) as tok, j (j)}<span
                  class={tok.cls}>{tok.text}</span>{/each}{'\n'}{/each}</pre>
        {/if}
      {/each}
      <div class="vec-footer">
        <button
          class="link"
          onclick={vectorize.rerunUncached}
          title="cached modules are replayed, not run, so they report nothing"
        >
          re-run uncached
        </button>
      </div>
    </div>
  {/if}
</div>

<style>
  .run-output {
    display: flex;
    flex-direction: column;
    max-height: 260px;
    overflow-y: auto;
    overflow-x: hidden;
    background: #141414;
    border: 1px solid #444;
    border-bottom: none;
  }

  /* Plan listings are tall; give them room instead of a 260px scroll box. */
  .run-output:has(.plan) {
    max-height: 65vh;
  }

  .error {
    color: #f44;
    font-size: 12px;
    padding: 8px;
    white-space: pre-wrap;
    overflow-wrap: break-word;
    font-family: inherit;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 6px 8px;
  }

  li {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    font-size: 11px;
    line-height: 1.6;
  }

  .label {
    color: #888;
  }

  .value {
    color: #ddd;
  }

  .vectorize {
    border-top: 1px solid #333;
    padding: 6px 8px;
    font-size: 11px;
  }

  .vec-header {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    line-height: 1.6;
  }

  .warn {
    color: #e0a030;
  }

  .vec-row {
    display: flex;
    align-items: baseline;
    gap: 10px;
  }

  .vec-main {
    display: flex;
    gap: 8px;
    flex: 1;
    min-width: 0;
    padding: 2px 0;
    background: none;
    border: none;
    color: #bbb;
    font: inherit;
    text-align: left;
    cursor: default;
  }

  .vec-main.clickable {
    cursor: pointer;
  }

  .vec-main.clickable:hover {
    color: #fff;
  }

  .plan {
    margin: 2px 0 6px 16px;
    padding: 6px 8px;
    background: #0e0e0e;
    border: 1px solid #2a2a2a;
    color: #aaa;
    font-size: 10.5px;
    line-height: 1.45;
    overflow-x: auto;
  }

  .plan .op {
    color: #e8c46a;
  }

  .plan .reg {
    color: #8fd3ff;
  }

  .plan .src {
    color: #9fe09f;
  }

  .plan .num {
    color: #d8a0e0;
  }

  .plan .comment,
  .plan .label {
    color: #777;
  }

  .glyph.ok {
    color: #6a6;
  }

  .glyph.bail {
    color: #e55;
  }

  .glyph.aborted {
    color: #e0a030;
  }

  .where {
    color: #ddd;
    white-space: nowrap;
  }

  .reason {
    color: #999;
    overflow-wrap: anywhere;
  }

  .vec-footer {
    display: flex;
    gap: 12px;
    padding-top: 4px;
  }

  .link {
    background: none;
    border: none;
    padding: 0;
    color: #7aa;
    font: inherit;
    cursor: pointer;
  }

  .link:hover {
    color: #acc;
  }
</style>
