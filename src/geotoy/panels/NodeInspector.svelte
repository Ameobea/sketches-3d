<script lang="ts">
  import type { Instance, NodeDef, Transform3, TreeDef } from 'src/geoscript/geotoyAPIClient';
  import { cloneTransform3 } from 'src/geoscript/geotoyAPIClient';
  import type { GizmoTargetRef } from 'src/viz/gizmos/gizmoTypes';
  import { evalMathExpr } from 'src/viz/util/mathExpr';

  interface Props {
    tree: TreeDef;
    parentId: string;
    meshCounts: ReadonlyMap<string, number>;
    armedRef: GizmoTargetRef | null;
    onselect: (id: string) => void;
    onInstanceTransformChange: (nodeId: string, instanceId: string, transform: Transform3) => void;
    onArmInstance: (nodeId: string, instanceId: string) => void;
    onAddInstance: (nodeId: string) => void;
    onRemoveInstance: (nodeId: string, instanceId: string) => void;
    onDisableToggle: (id: string, disabled: boolean) => void;
  }

  let {
    tree,
    parentId,
    meshCounts,
    armedRef,
    onselect,
    onInstanceTransformChange,
    onArmInstance,
    onAddInstance,
    onRemoveInstance,
    onDisableToggle,
  }: Props = $props();

  const RAD_TO_DEG = 180 / Math.PI;
  const DEG_TO_RAD = Math.PI / 180;
  const LS_KEY = 'geotoy-inspector-open';
  const ROW_LS_KEY = 'geotoy-inspector-rows-open';

  const loadMap = (key: string): Record<string, boolean> => {
    try {
      const parsed = JSON.parse(localStorage.getItem(key) ?? '');
      return parsed && typeof parsed === 'object' ? parsed : {};
    } catch {
      return {};
    }
  };
  const saveMap = (key: string, map: Record<string, boolean>) => {
    try {
      localStorage.setItem(key, JSON.stringify(map));
    } catch {}
  };

  let openMap = $state<Record<string, boolean>>(loadMap(LS_KEY));
  const isOpen = $derived(openMap[parentId] === true);
  const toggleOpen = () => {
    openMap = { ...openMap, [parentId]: !isOpen };
    saveMap(LS_KEY, openMap);
  };

  // Per-child instance-list expansion; multi-instance rows default open.
  let rowOpenMap = $state<Record<string, boolean>>(loadMap(ROW_LS_KEY));
  const isRowOpen = (cid: string): boolean => rowOpenMap[cid] ?? true;
  const toggleRow = (cid: string) => {
    rowOpenMap = { ...rowOpenMap, [cid]: !isRowOpen(cid) };
    saveMap(ROW_LS_KEY, rowOpenMap);
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

  let focused: { id: string; instanceId: string; field: Field; axis: Axis } | null = $state(null);
  let draft = $state('');

  // Selection swap can unmount an input before its blur fires; clear focus state.
  $effect(() => {
    void parentId;
    focused = null;
    draft = '';
  });

  const isArmed = (nodeId: string, instanceId: string): boolean =>
    armedRef?.kind === 'instance' && armedRef.nodeId === nodeId && armedRef.instanceId === instanceId;

  const fmt = (n: number): string => {
    if (!Number.isFinite(n)) return '0';
    const v = Math.abs(n) < 1e-9 ? 0 : n;
    return Number(v.toFixed(4)).toString();
  };

  const underlying = (inst: Instance, field: Field, axis: Axis): number => {
    const raw = inst[field][axis];
    return field === 'rot' ? raw * RAD_TO_DEG : raw;
  };

  const displayVal = (nodeId: string, inst: Instance, field: Field, axis: Axis): string => {
    if (
      focused &&
      focused.id === nodeId &&
      focused.instanceId === inst.id &&
      focused.field === field &&
      focused.axis === axis
    ) {
      return draft;
    }
    return fmt(underlying(inst, field, axis));
  };

  const onFocus = (nodeId: string, inst: Instance, field: Field, axis: Axis) => {
    draft = fmt(underlying(inst, field, axis));
    focused = { id: nodeId, instanceId: inst.id, field, axis };
  };

  const commit = () => {
    if (!focused) return;
    const inst = tree.nodes[focused.id]?.instances.find(i => i.id === focused!.instanceId);
    if (!inst) return;
    const parsed = evalMathExpr(draft);
    if (parsed === null) return;
    const value = focused.field === 'rot' ? parsed * DEG_TO_RAD : parsed;
    if (inst[focused.field][focused.axis] === value) return;
    const next = cloneTransform3(inst);
    next[focused.field][focused.axis] = value;
    onInstanceTransformChange(focused.id, inst.id, next);
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

{#snippet tforms(node: NodeDef, inst: Instance)}
  {#each ['pos', 'rot', 'scale'] as const as field}
    <span class="tf-group">
      <span class="tf-label" title={FIELD_META[field].title}>{FIELD_META[field].glyph}</span>
      {#each [0, 1, 2] as const as axis}
        <input
          class="tf-input"
          type="text"
          value={displayVal(node.id, inst, field, axis)}
          oninput={e => {
            draft = (e.target as HTMLInputElement).value;
          }}
          onfocus={() => onFocus(node.id, inst, field, axis)}
          onblur={onBlur}
          onkeydown={onKeydown}
        />
      {/each}
    </span>
  {/each}
{/snippet}

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
            {@const nInst = child.instances.length}
            {@const expanded = nInst > 1 && isRowOpen(child.id)}
            <div class="row" class:disabled-row={disabled}>
              <div class="row-head">
                <input
                  class="toggle"
                  type="checkbox"
                  checked={!disabled}
                  title={disabled ? 'enable' : 'disable'}
                  onchange={e => onDisableToggle(child.id, !(e.currentTarget as HTMLInputElement).checked)}
                />
                <button class="name" type="button" title="select" onclick={() => onselect(child.id)}>
                  {child.name}
                </button>
                {#if nInst > 1}
                  <button
                    class="inst-chip"
                    type="button"
                    title="{nInst} instances — click to collapse/expand"
                    onclick={() => toggleRow(child.id)}
                  >
                    ×{nInst}
                    <span class="chev-sm">{expanded ? '▾' : '▸'}</span>
                  </button>
                {:else}
                  <span
                    class="mesh-count"
                    title={disabled
                      ? 'disabled — does not evaluate'
                      : `${count} mesh${count === 1 ? '' : 'es'} from this subtree`}
                  >
                    {disabled ? '—' : count}
                  </span>
                {/if}
              </div>

              {#if nInst === 1}
                {@const inst = child.instances[0]}
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div
                  class="row-tforms inst-arm"
                  class:armed={isArmed(child.id, inst.id)}
                  title="click to arm the gizmo to this instance"
                  onclick={() => onArmInstance(child.id, inst.id)}
                >
                  <span class="inst-tforms">{@render tforms(child, inst)}</span>
                  <button
                    class="icon-btn add"
                    type="button"
                    title="add instance"
                    onclick={e => {
                      e.stopPropagation();
                      onAddInstance(child.id);
                    }}
                  >
                    ＋
                  </button>
                </div>
              {:else if expanded}
                <div class="inst-list">
                  {#each child.instances as inst (inst.id)}
                    <!-- svelte-ignore a11y_click_events_have_key_events -->
                    <!-- svelte-ignore a11y_no_static_element_interactions -->
                    <div
                      class="inst-row inst-arm"
                      class:armed={isArmed(child.id, inst.id)}
                      title="click to arm the gizmo to this instance"
                      onclick={() => onArmInstance(child.id, inst.id)}
                    >
                      <span class="inst-tforms">{@render tforms(child, inst)}</span>
                      <button
                        class="icon-btn rm"
                        type="button"
                        title="remove instance"
                        onclick={e => {
                          e.stopPropagation();
                          onRemoveInstance(child.id, inst.id);
                        }}
                      >
                        ✕
                      </button>
                    </div>
                  {/each}
                  <button class="add-inst-row" type="button" onclick={() => onAddInstance(child.id)}>
                    ＋ add instance
                  </button>
                </div>
              {/if}
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
    cursor: default;
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
    overflow: auto;
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
    flex: 0 1 auto;
    min-width: 0;
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

  .inst-chip {
    display: inline-flex;
    align-items: center;
    gap: 1px;
    margin-left: auto;
    background: #2a2a2a;
    border: 1px solid #383838;
    color: #bbb;
    cursor: pointer;
    font: inherit;
    font-size: 10px;
    padding: 0 3px;
    flex-shrink: 0;
  }

  .inst-chip:hover {
    background: #333;
    color: #ddd;
  }

  .chev-sm {
    color: #888;
    font-size: 8px;
  }

  .mesh-count {
    color: #888;
    font-size: 10px;
    margin-left: auto;
    border: 1px solid #333;
    padding: 0 4px;
    flex-shrink: 0;
    min-width: 18px;
    text-align: center;
  }

  .row-tforms {
    display: flex;
    flex-wrap: nowrap;
    align-items: center;
    gap: 6px;
    padding-left: 22px;
  }

  .inst-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  /* Left padding matches `.row-tforms` so multi-instance transforms line up with
   * the single-instance inline layout. */
  .inst-row {
    display: flex;
    flex-wrap: nowrap;
    align-items: center;
    gap: 6px;
    padding: 1px 0 1px 22px;
  }

  /* Clicking anywhere on an instance row arms the gizmo to it. */
  .inst-arm {
    cursor: pointer;
  }

  .inst-arm:hover:not(.armed) {
    background: rgba(44, 107, 69, 0.16);
    outline: 1px solid rgba(44, 107, 69, 0.4);
  }

  .inst-row.armed,
  .row-tforms.armed {
    background: #15301f;
    outline: 1px solid #2c6b45;
  }

  .inst-tforms {
    display: inline-flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    flex: 1 1 auto;
    min-width: 0;
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
    font: 11px monospace;
    padding: 1px 3px;
    width: 60px;
  }

  .tf-input:focus {
    outline: none;
    border-color: #7a7;
    background: #181818;
  }

  .icon-btn {
    background: none;
    border: 1px solid #333;
    color: #999;
    cursor: pointer;
    font: inherit;
    font-size: 11px;
    line-height: 1;
    padding: 1px 4px;
    flex-shrink: 0;
  }

  .icon-btn:hover {
    color: #fff;
    border-color: #555;
  }

  .rm:hover {
    color: #e88;
    border-color: #844;
  }

  .add-inst-row {
    align-self: flex-start;
    background: none;
    border: 1px dashed #3a3a3a;
    color: #888;
    cursor: pointer;
    font: inherit;
    font-size: 10px;
    margin-top: 1px;
    margin-left: 22px;
    padding: 1px 6px;
  }

  .add-inst-row:hover {
    color: #ddd;
    border-color: #555;
  }
</style>
