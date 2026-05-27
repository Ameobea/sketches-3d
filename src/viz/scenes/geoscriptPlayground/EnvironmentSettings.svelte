<script lang="ts">
  import type { EnvironmentConfig, TextureID, User } from 'src/geoscript/geotoyAPIClient';
  import { Textures } from './materialEditor/state.svelte';
  import TexturePicker from './materialEditor/TexturePicker.svelte';
  import TexturePreview from './materialEditor/TexturePreview.svelte';

  let {
    isOpen = $bindable(),
    environment = $bindable(),
    me,
  }: {
    isOpen: boolean;
    environment: EnvironmentConfig | undefined;
    me: User | null | undefined;
  } = $props();

  let view = $state<{ type: 'settings' } | { type: 'texture_picker' }>({ type: 'settings' });

  const numToHex = (n: number): string => '#' + n.toString(16).padStart(6, '0');
  const hexToNum = (s: string): number => parseInt(s.slice(1), 16);

  type EnvKind = 'none' | 'gradient' | 'equirect';
  const currentKind = (): EnvKind => environment?.kind ?? 'none';

  const DEFAULT_GRADIENT = {
    kind: 'gradient' as const,
    skyColor: 0xbfd4e6,
    horizonColor: 0x9aa3a8,
    groundColor: 0x4a4540,
    intensity: 1,
    setBackground: true,
  };

  const onKindChange = (kind: EnvKind) => {
    if (kind === 'none') {
      environment = undefined;
    } else if (kind === 'gradient') {
      environment = { ...DEFAULT_GRADIENT };
    } else {
      const prevId = environment?.kind === 'equirect' ? environment.textureId : undefined;
      environment = {
        kind: 'equirect',
        textureId: prevId ?? (-1 as TextureID),
        intensity: 1,
        setBackground: true,
      };
    }
  };

  // Narrowed, mutable views onto `environment` for binding.
  const gradient = $derived(environment?.kind === 'gradient' ? environment : null);
  const equirect = $derived(environment?.kind === 'equirect' ? environment : null);

  const update = (patch: Partial<EnvironmentConfig>) => {
    if (!environment) return;
    environment = { ...environment, ...patch } as EnvironmentConfig;
  };

  const intensityOf = (): number => environment?.intensity ?? 1;
  const backgroundOn = (): boolean => environment?.setBackground !== false;
  const pickedTexture = $derived(
    equirect && equirect.textureId >= 0 ? Textures.textures[equirect.textureId] : undefined
  );
</script>

{#if isOpen}
  <div class="env-dialog">
    <div class="drag-handle">
      <span>scene environment</span>
      <button class="close-button" onclick={() => (isOpen = false)}>×</button>
    </div>
    {#if view.type === 'texture_picker'}
      <div class="picker-host">
        <TexturePicker
          selectedTextureId={equirect?.textureId}
          onselect={id => {
            if (environment?.kind === 'equirect' && id !== null) {
              environment = { ...environment, textureId: id };
            }
          }}
          onclose={() => (view = { type: 'settings' })}
          onupload={() => {}}
          {me}
        />
      </div>
    {:else}
      <div class="content">
        <div class="row">
          <span class="label">source</span>
          <select
            class="select"
            value={currentKind()}
            onchange={e => onKindChange((e.target as HTMLSelectElement).value as EnvKind)}
          >
            <option value="none">none</option>
            <option value="gradient">gradient</option>
            <option value="equirect">equirect image</option>
          </select>
        </div>

        {#if gradient}
          <div class="row">
            <span class="label">sky</span>
            <input
              type="color"
              value={numToHex(gradient.skyColor)}
              oninput={e => update({ skyColor: hexToNum((e.target as HTMLInputElement).value) })}
            />
          </div>
          <div class="row">
            <span class="label">horizon</span>
            <input
              type="color"
              value={numToHex(gradient.horizonColor)}
              oninput={e => update({ horizonColor: hexToNum((e.target as HTMLInputElement).value) })}
            />
          </div>
          <div class="row">
            <span class="label">ground</span>
            <input
              type="color"
              value={numToHex(gradient.groundColor)}
              oninput={e => update({ groundColor: hexToNum((e.target as HTMLInputElement).value) })}
            />
          </div>
        {/if}

        {#if equirect}
          <div class="row">
            <span class="label">image</span>
            <TexturePreview texture={pickedTexture} onclick={() => (view = { type: 'texture_picker' })} />
            <button class="btn" onclick={() => (view = { type: 'texture_picker' })}>choose…</button>
          </div>
        {/if}

        {#if environment}
          <div class="row">
            <span class="label">intensity</span>
            <input
              type="range"
              min="0"
              max="3"
              step="0.01"
              value={intensityOf()}
              oninput={e => update({ intensity: (e.target as HTMLInputElement).valueAsNumber })}
            />
            <span class="value">{intensityOf().toFixed(2)}</span>
          </div>
          <div class="row">
            <span class="label">background</span>
            <input
              type="checkbox"
              checked={backgroundOn()}
              onchange={e => update({ setBackground: (e.target as HTMLInputElement).checked })}
            />
            <span class="hint">use environment as the scene background</span>
          </div>
        {/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .env-dialog {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    background: #222;
    color: #f0f0f0;
    border: 1px solid #888;
    width: 800px;
    max-width: calc(min(800px, 90vw));
    max-height: min(80vh, 600px);
    z-index: 100;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .drag-handle {
    display: flex;
    padding: 6px 8px;
    background: #333;
    user-select: none;
    font-size: 13px;
  }

  .close-button {
    margin-left: auto;
    background: none;
    border: none;
    color: #f0f0f0;
    font-size: 22px;
    cursor: pointer;
    padding: 0;
    height: 20px;
    line-height: 0;
    margin-top: -2px;
  }

  .close-button:hover {
    color: #f22;
  }

  .content {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px;
    min-height: 120px;
  }

  .picker-host {
    display: flex;
    flex-grow: 1;
    max-height: calc(min(80vh, 600px) - 50px);
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .label {
    width: 72px;
    font-size: 12px;
    color: #aaa;
    flex-shrink: 0;
  }

  .value {
    font-size: 12px;
    color: #888;
    font-family: monospace;
  }

  .hint {
    font-size: 11px;
    color: #777;
  }

  .select,
  .btn {
    background: #1a1a1a;
    color: #e8e8e8;
    border: 1px solid #555;
    padding: 3px 6px;
    font: 12px monospace;
    cursor: pointer;
  }

  input[type='color'] {
    width: 36px;
    height: 22px;
    padding: 0;
    border: 1px solid #555;
    background: none;
    cursor: pointer;
  }
</style>
