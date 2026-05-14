<script lang="ts">
  import type { NodeDef, Transform3, TreeDef } from 'src/geoscript/geotoyAPIClient';
  import { evalMathExpr } from 'src/viz/util/mathExpr';

  interface Props {
    tree: TreeDef;
    parentId: string;
    meshCounts: ReadonlyMap<string, number>;
    onselect: (id: string) => void;
    onTransformChange: (id: string, transform: Transform3) => void;
    onDisableToggle: (id: string, disabled: boolean) => void;
  }

  let { tree, parentId, meshCounts, onselect, onTransformChange, onDisableToggle }: Props = $props();

  const RAD_TO_DEG = 180 / Math.PI;
  const DEG_TO_RAD = Math.PI / 180;
  const LS_KEY = 'geotoy-inspector-open';

  const loadOpenMap = (): Record<string, boolean> => {
    try {
      const raw = localStorage.getItem(LS_KEY);
      if (!raw) return {};
      const parsed = JSON.parse(raw);
      return parsed && typeof parsed === 'object' ? parsed : {};
    } catch {
      return {};
    }
  };
  let openMap = $state<Record<string, boolean>>(loadOpenMap());
  const isOpen = $derived(openMap[parentId] === true);
  const toggleOpen = () => {
    openMap = { ...openMap, [parentId]: !isOpen };
    try {
      localStorage.setItem(LS_KEY, JSON.stringify(openMap));
    } catch {}
  };

  const parent = $derived(tree.nodes[parentId]);
  const children = $derived(parent?.children ?? []);

  type Field = 'pos' | 'rot' | 'scale';
  type Axis = 0 | 1 | 2;

  const FIELD_META: Record<Field, { glyph: string; title: string }> = {
    pos: { glyph: 't', title: 'translation' },
    rot: { glyph: 'r', title: 'rotation (degrees, YXZ)' },
    scale: { glyph: 's', title: 'scale' },
  };

  let focused: { id: string; field: Field; axis: Axis } | null = $state(null);
  let draft = $state('');

  // Selection swap can unmount an input before its blur fires; clear focus
  // state so the next focus elsewhere starts clean.
  $effect(() => {
    void parentId;
    focused = null;
    draft = '';
  });

  const fmt = (n: number): string => {
    if (!Number.isFinite(n)) return '0';
    const v = Math.abs(n) < 1e-9 ? 0 : n;
    return Number(v.toFixed(4)).toString();
  };

  const underlying = (node: NodeDef, field: Field, axis: Axis): number => {
    const raw = node.transform[field][axis];
    return field === 'rot' ? raw * RAD_TO_DEG : raw;
  };

  const displayVal = (node: NodeDef, field: Field, axis: Axis): string => {
    if (focused && focused.id === node.id && focused.field === field && focused.axis === axis) {
      return draft;
    }
    return fmt(underlying(node, field, axis));
  };

  const onFocus = (node: NodeDef, field: Field, axis: Axis) => {
    draft = fmt(underlying(node, field, axis));
    focused = { id: node.id, field, axis };
  };

  const commit = () => {
    if (!focused) return;
    const node = tree.nodes[focused.id];
    if (!node) return;
    const parsed = evalMathExpr(draft);
    if (parsed === null) return;
    const value = focused.field === 'rot' ? parsed * DEG_TO_RAD : parsed;
    const t = node.transform;
    if (t[focused.field][focused.axis] === value) return;
    const next: Transform3 = {
      pos: [t.pos[0], t.pos[1], t.pos[2]],
      rot: [t.rot[0], t.rot[1], t.rot[2]],
      scale: [t.scale[0], t.scale[1], t.scale[2]],
    };
    next[focused.field][focused.axis] = value;
    onTransformChange(node.id, next);
  };

  const onBlur = () => {
    commit();
    focused = null;
  };

  const onKeydown = (e: KeyboardEvent) => {
    if (e.key === 'Enter') {
      commit();
      (e.target as HTMLInputElement).blur();
    } else if (e.key === 'Escape') {
      focused = null;
      (e.target as HTMLInputElement).blur();
    }
  };
</script>

{#if parent && children.length > 0}
  <div class="inspector">
    <button class="header" type="button" onclick={toggleOpen}>
      <span class="chev">{isOpen ? '▾' : '▸'}</span>
      <span class="hdr-label">children of</span>
      <span class="hdr-name">{parent.name}</span>
      <span class="hdr-count">({children.length})</span>
    </button>
    {#if isOpen}
      <div class="rows">
        {#each children as cid (cid)}
          {@const child = tree.nodes[cid]}
          {#if child}
            {@const disabled = child.disabled === true}
            {@const count = meshCounts.get(child.id) ?? 0}
            <div class="row" class:disabled-row={disabled}>
              <div class="row-head">
                <input
                  class="toggle"
                  type="checkbox"
                  checked={!disabled}
                  title={disabled ? 'enable' : 'disable'}
                  onchange={(e) =>
                    onDisableToggle(child.id, !(e.currentTarget as HTMLInputElement).checked)}
                />
                <button
                  class="name"
                  type="button"
                  title="select"
                  onclick={() => onselect(child.id)}
                >{child.name}</button>
                <span
                  class="mesh-count"
                  title={disabled
                    ? 'disabled — does not evaluate'
                    : `${count} mesh${count === 1 ? '' : 'es'} from this subtree`}
                >{disabled ? '—' : count}</span>
              </div>
              <div class="row-tforms">
                {#each (['pos', 'rot', 'scale'] as const) as field}
                  <span class="tf-group">
                    <span class="tf-label" title={FIELD_META[field].title}
                      >{FIELD_META[field].glyph}</span>
                    {#each ([0, 1, 2] as const) as axis}
                      <input
                        class="tf-input"
                        type="text"
                        value={displayVal(child, field, axis)}
                        oninput={(e) => { draft = (e.target as HTMLInputElement).value; }}
                        onfocus={() => onFocus(child, field, axis)}
                        onblur={onBlur}
                        onkeydown={onKeydown}
                      />
                    {/each}
                  </span>
                {/each}
              </div>
            </div>
          {/if}
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .inspector {
    display: flex;
    flex-direction: column;
    background: #1c1c1c;
    border-bottom: 1px solid #333;
    flex-shrink: 0;
    font-size: 11px;
  }

  .header {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 2px 6px;
    background: #181818;
    border: none;
    border-bottom: 1px solid #2a2a2a;
    color: #aaa;
    cursor: pointer;
    font: inherit;
    text-align: left;
  }

  .header:hover {
    background: #222;
  }

  .chev {
    width: 10px;
    font-size: 9px;
    color: #888;
  }

  .hdr-label {
    color: #777;
  }

  .hdr-name {
    color: #ddd;
    font-weight: 600;
  }

  .hdr-count {
    color: #666;
    margin-left: auto;
    font-size: 10px;
  }

  .rows {
    display: flex;
    flex-direction: column;
    max-height: 220px;
    overflow-y: auto;
  }

  .row {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 3px 6px;
    border-bottom: 1px solid #232323;
  }

  .row:last-child {
    border-bottom: none;
  }

  .row.disabled-row .name {
    color: #777;
    text-decoration: line-through;
  }

  .row-head {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .toggle {
    margin: 0;
    cursor: pointer;
    flex-shrink: 0;
  }

  .name {
    flex: 1;
    background: none;
    border: none;
    color: #ddd;
    text-align: left;
    padding: 0;
    cursor: pointer;
    font: inherit;
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .name:hover {
    color: #fff;
    text-decoration: underline;
  }

  .mesh-count {
    color: #888;
    font-size: 10px;
    border: 1px solid #333;
    border-radius: 2px;
    padding: 0 4px;
    flex-shrink: 0;
    min-width: 18px;
    text-align: center;
  }

  .row-tforms {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    padding-left: 22px;
  }

  .tf-group {
    display: inline-flex;
    align-items: center;
    gap: 2px;
  }

  .tf-label {
    color: #777;
    font-size: 10px;
    width: 8px;
  }

  .tf-input {
    background: #111;
    color: #ddd;
    border: 1px solid #333;
    border-radius: 2px;
    font: 11px monospace;
    padding: 1px 3px;
    width: 60px;
  }

  .tf-input:focus {
    outline: none;
    border-color: #7a7;
    background: #181818;
  }
</style>
