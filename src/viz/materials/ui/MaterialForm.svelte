<script lang="ts">
  import type { Snippet } from 'svelte';

  import type { CustomShaderMatDef, MaterialDef } from 'src/viz/materials/schema';
  import FormField from './FormField.svelte';
  import ColorPicker from './ColorPicker.svelte';
  import UvPropertiesEditor from './UvPropertiesEditor.svelte';
  import DerivedMapConfigurator from './DerivedMapConfigurator.svelte';
  import type { MaterialEditorHost, PhysicalMaterialTextureField } from './host';

  let {
    material = $bindable(),
    host,
    showAdvanced = $bindable(),
    textureSlot,
  }: {
    material: MaterialDef;
    host: MaterialEditorHost;
    showAdvanced: boolean;
    textureSlot: Snippet<
      [
        {
          field: PhysicalMaterialTextureField;
          handle: string | undefined;
          set: (h: string | undefined) => void;
        },
      ]
    >;
  } = $props();

  type ReqCustomShader = CustomShaderMatDef & {
    props: NonNullable<CustomShaderMatDef['props']>;
    options: NonNullable<CustomShaderMatDef['options']>;
  };
  // Valid only inside `{#if material.type === 'customShader'}`; props/options are ensured below.
  let cs = $derived(material as ReqCustomShader);

  $effect(() => {
    if (material.type === 'customShader') {
      material.props ??= {};
      material.options ??= {};
      material.props.uvScale ??= [1, 1];
    } else if (material.type === 'customBasicShader') {
      material.props ??= {};
    }
  });

  const mappingMode = (m: ReqCustomShader): 'triplanar' | 'mesh_uv' | 'uv' =>
    m.meshUvUnwrap ? 'uv' : m.options.useTriplanarMapping === false ? 'mesh_uv' : 'triplanar';

  let showRoughnessConfigurator = $state(false);
  let showMetalnessConfigurator = $state(false);
</script>

{#snippet texField(
  label: string,
  field: PhysicalMaterialTextureField,
  handle: string | undefined,
  set: (h: string | undefined) => void
)}
  <FormField {label}>
    {@render textureSlot({ field, handle, set })}
  </FormField>
{/snippet}

{#if showRoughnessConfigurator}
  <DerivedMapConfigurator
    onclose={() => (showRoughnessConfigurator = false)}
    onsave={params => {
      (cs.shaders ??= {}).roughnessReverseColorRamp = params;
      showRoughnessConfigurator = false;
    }}
  />
{/if}

{#if showMetalnessConfigurator}
  <DerivedMapConfigurator
    onclose={() => (showMetalnessConfigurator = false)}
    onsave={params => {
      (cs.shaders ??= {}).metalnessReverseColorRamp = params;
      showMetalnessConfigurator = false;
    }}
  />
{/if}

<div class="properties-editor">
  {#if material.type === 'generated'}
    <div class="placeholder">generated material — edit via the asset pipeline</div>
  {:else}
    {#if host.showSaveToLibrary}
      <button class="save-to-library" onclick={host.onsavetolibrary}>save/share</button>
    {/if}

    {#if host.showName}
      <FormField label="name">
        <input type="text" bind:value={material.name} />
      </FormField>
    {/if}

    <FormField label="type">
      <select
        value={material.type}
        onchange={e =>
          host.onconverttype((e.target as HTMLSelectElement).value as 'customShader' | 'customBasicShader')}
      >
        <option value="customBasicShader">basic</option>
        <option value="customShader">physical</option>
      </select>
    </FormField>

    <FormField label="color">
      <ColorPicker value={material.props?.color} onchange={n => ((material.props ??= {}).color = n)} />
    </FormField>
  {/if}

  {#if material.type === 'customShader'}
    <FormField label="roughness">
      <input type="range" min="0" max="1" step="0.01" bind:value={cs.props.roughness} />
      <span>{(cs.props.roughness ?? 0).toFixed(2)}</span>
    </FormField>
    <FormField label="metalness">
      <input type="range" min="0" max="1" step="0.01" bind:value={cs.props.metalness} />
      <span>{(cs.props.metalness ?? 0).toFixed(2)}</span>
    </FormField>
    <FormField
      label="oren-nayar diffuse"
      help="Rough-matte diffuse for direct lights (plaster, concrete, sand, clay): flatter terminator, slight grazing retroreflection. Driven by roughness; no-op at roughness 0. Indirect/ambient fill stays Lambert."
    >
      <input
        type="checkbox"
        checked={cs.options.useOrenNayarDiffuse ?? true}
        onchange={e => (cs.options.useOrenNayarDiffuse = (e.target as HTMLInputElement).checked)}
      />
    </FormField>
    <FormField label="env intensity">
      <input
        type="range"
        min="0"
        max="3"
        step="0.01"
        value={cs.props.envMapIntensity ?? 1}
        oninput={e => (cs.props.envMapIntensity = (e.target as HTMLInputElement).valueAsNumber)}
      />
      <span>{(cs.props.envMapIntensity ?? 1).toFixed(2)}</span>
    </FormField>

    {@render texField('map', 'map', cs.props.map, h => (cs.props.map = h))}
    {@render texField('normal map', 'normalMap', cs.props.normalMap, h => (cs.props.normalMap = h))}
    <FormField label="normal scale">
      <input type="range" min="0" max="5" step="0.01" bind:value={cs.props.normalScale} />
      <span>{(cs.props.normalScale ?? 0).toFixed(2)}</span>
    </FormField>
    {@render texField(
      'roughness map',
      'roughnessMap',
      cs.props.roughnessMap,
      h => (cs.props.roughnessMap = h)
    )}
    {#if cs.props.map}
      <FormField label="derived roughness map">
        <div class="derived-map-controls">
          {#if cs.shaders?.roughnessReverseColorRamp}
            <span>enabled</span>
            <button onclick={() => cs.shaders && (cs.shaders.roughnessReverseColorRamp = undefined)}>
              disable
            </button>
          {:else}
            <span>disabled</span>
            <button onclick={() => (showRoughnessConfigurator = true)}>configure</button>
          {/if}
        </div>
      </FormField>
    {/if}
    {@render texField(
      'metalness map',
      'metalnessMap',
      cs.props.metalnessMap,
      h => (cs.props.metalnessMap = h)
    )}
    {#if cs.props.map}
      <FormField label="derived metalness map">
        <div class="derived-map-controls">
          {#if cs.shaders?.metalnessReverseColorRamp}
            <span>enabled</span>
            <button onclick={() => cs.shaders && (cs.shaders.metalnessReverseColorRamp = undefined)}>
              disable
            </button>
          {:else}
            <span>disabled</span>
            <button onclick={() => (showMetalnessConfigurator = true)}>configure</button>
          {/if}
        </div>
      </FormField>
    {/if}
    <FormField label="texture scale" help="The scale of the texture coordinates.">
      <input
        type="number"
        step="0.1"
        value={cs.props.uvScale?.[0] ?? 1}
        oninput={e => ((cs.props.uvScale ??= [1, 1])[0] = (e.target as HTMLInputElement).valueAsNumber)}
        style="width: 80px"
      />
      <input
        type="number"
        step="0.1"
        value={cs.props.uvScale?.[1] ?? 1}
        oninput={e => ((cs.props.uvScale ??= [1, 1])[1] = (e.target as HTMLInputElement).valueAsNumber)}
        style="width: 80px"
      />
    </FormField>

    {#if host.showUvUnwrap}
      <FormField
        label="texture mapping"
        help="Controls how textures are mapped to the mesh's surface.  Triplanar mapping works great for many uses.  'mesh uv' samples the mesh's own UV attribute (e.g. analytic UVs emitted by rail_sweep) with no reprojection — required for tangent-space POM.  'uv' generates UVs via boundary-first flattening, useful for exporting to tools without triplanar support."
      >
        <div class="toggle-group">
          <button
            class:selected={mappingMode(cs) === 'triplanar'}
            onclick={() => {
              cs.options.useTriplanarMapping = true;
              cs.options.useGeneratedUVs = false;
              cs.options.tileBreaking = undefined;
              cs.meshUvUnwrap = undefined;
            }}
          >
            triplanar
          </button>
          <button
            class:selected={mappingMode(cs) === 'mesh_uv'}
            onclick={() => {
              if (mappingMode(cs) !== 'mesh_uv') {
                cs.options.useTriplanarMapping = false;
                cs.meshUvUnwrap = undefined;
                host.rerun(false);
              }
            }}
          >
            mesh uv
          </button>
          <button
            class:selected={mappingMode(cs) === 'uv'}
            onclick={() => {
              if (mappingMode(cs) !== 'uv') {
                cs.options.useTriplanarMapping = false;
                cs.meshUvUnwrap = {
                  numCones: 0,
                  flattenToDisk: false,
                  mapToSphere: false,
                  enableUVIslandRotation: true,
                };
                host.rerun(true);
              }
            }}
          >
            uv
          </button>
        </div>
      </FormField>
      {#if mappingMode(cs) === 'uv'}
        <UvPropertiesEditor material={cs} rerun={host.rerun} />
        <div style="display: flex; padding-left: 8px">
          <button class="edit-shaders" onclick={host.onviewuvmappings} style="width:240px">
            view generated uv mappings
          </button>
        </div>
      {/if}
      {#if mappingMode(cs) === 'uv' || mappingMode(cs) === 'mesh_uv'}
        <FormField label="enable hex tilebreaking">
          <input
            type="checkbox"
            checked={cs.options.tileBreaking !== undefined}
            onchange={() => {
              cs.options.tileBreaking = cs.options.tileBreaking
                ? undefined
                : { type: 'neyret', patchScale: 1.0 };
              host.rerun(false);
            }}
          />
        </FormField>
        {#if cs.options.tileBreaking}
          <div style="margin-top: 8px; padding-left: 16px">
            <FormField
              label="hex patch scale"
              help="Scale factor for the hexagonal tiles used to break up the texture. This parameter is crucial in controlling the hex tiling's appearance and requires adjustment for each texture."
            >
              <input
                type="number"
                step="0.1"
                bind:value={cs.options.tileBreaking.patchScale}
                style="width: 80px"
                onchange={() => host.rerun(false)}
              />
            </FormField>
          </div>
        {/if}
      {/if}
    {:else}
      <FormField
        label="triplanar mapping"
        help="Project textures along the three world axes (no UVs needed)."
      >
        <input
          type="checkbox"
          checked={!!cs.options.useTriplanarMapping}
          onchange={e => (cs.options.useTriplanarMapping = (e.target as HTMLInputElement).checked)}
        />
      </FormField>
      {#if host.showLevelProps}
        <FormField label="generated UVs">
          <input type="checkbox" bind:checked={cs.options.useGeneratedUVs} />
        </FormField>
        <FormField label="randomize UV offset">
          <input type="checkbox" bind:checked={cs.options.randomizeUVOffset} />
        </FormField>
      {/if}
      <FormField label="tile breaking" help="Neyret hex-grid tile-breaking to hide texture repetition.">
        <input
          type="checkbox"
          checked={cs.options.tileBreaking !== undefined}
          onchange={() => {
            cs.options.tileBreaking = cs.options.tileBreaking
              ? undefined
              : { type: 'neyret', patchScale: 1.0 };
            host.rerun(false);
          }}
        />
      </FormField>
      {#if cs.options.tileBreaking?.type === 'neyret'}
        <FormField label="hex patch scale">
          <input
            type="number"
            step="0.1"
            bind:value={cs.options.tileBreaking.patchScale}
            style="width: 80px"
            onchange={() => host.rerun(false)}
          />
        </FormField>
      {/if}
    {/if}

    <div style="display: flex; padding-left: 8px">
      <button class="edit-shaders" onclick={host.oneditshaders}>edit shaders</button>
    </div>

    <div class="advanced-options">
      <button
        class="advanced-toggle"
        onclick={() => {
          showAdvanced = !showAdvanced;
        }}
      >
        {showAdvanced ? 'hide' : 'show'} advanced options
      </button>
      {#if showAdvanced}
        <div class="advanced-content">
          <FormField label="clearcoat">
            <input type="range" min="0" max="1" step="0.01" bind:value={cs.props.clearcoat} />
            <span>{(cs.props.clearcoat ?? 0).toFixed(2)}</span>
          </FormField>
          <FormField label="clearcoat roughness">
            <input type="range" min="0" max="1" step="0.01" bind:value={cs.props.clearcoatRoughness} />
            <span>{(cs.props.clearcoatRoughness ?? 0).toFixed(2)}</span>
          </FormField>
          {@render texField(
            'clearcoat normal map',
            'clearcoatNormalMap',
            cs.props.clearcoatNormalMap,
            h => (cs.props.clearcoatNormalMap = h)
          )}
          <FormField label="clearcoat normal scale">
            <input type="range" min="0" max="5" step="0.01" bind:value={cs.props.clearcoatNormalScale} />
            <span>{(cs.props.clearcoatNormalScale ?? 0).toFixed(2)}</span>
          </FormField>
          <FormField label="iridescence">
            <input type="range" min="0" max="1" step="0.01" bind:value={cs.props.iridescence} />
            <span>{(cs.props.iridescence ?? 0).toFixed(2)}</span>
          </FormField>
          <FormField label="sheen">
            <input type="range" min="0" max="1" step="0.01" bind:value={cs.props.sheen} />
            <span>{(cs.props.sheen ?? 0).toFixed(2)}</span>
          </FormField>
          <FormField label="sheen color">
            <ColorPicker value={cs.props.sheenColor} onchange={n => (cs.props.sheenColor = n)} />
          </FormField>
          <FormField label="sheen roughness">
            <input type="range" min="0" max="1" step="0.01" bind:value={cs.props.sheenRoughness} />
            <span>{(cs.props.sheenRoughness ?? 0).toFixed(2)}</span>
          </FormField>

          {#if host.showLevelProps}
            <div class="section-divider"></div>
            <FormField label="material class">
              <select bind:value={cs.options.materialClass}>
                <option value={undefined}>default</option>
                <option value="rock">rock</option>
                <option value="crystal">crystal</option>
                <option value="metalplate">metalplate</option>
              </select>
            </FormField>
            <FormField label="opacity">
              <input
                type="number"
                min="0"
                max="1"
                step="0.01"
                bind:value={cs.props.opacity}
                style="width: 80px"
              />
            </FormField>
            <FormField label="transmission">
              <input
                type="number"
                min="0"
                max="1"
                step="0.01"
                bind:value={cs.props.transmission}
                style="width: 80px"
              />
            </FormField>
            <FormField label="ior">
              <input
                type="number"
                min="1"
                max="2.5"
                step="0.01"
                bind:value={cs.props.ior}
                style="width: 80px"
              />
            </FormField>
            <FormField label="fog multiplier">
              <input type="number" step="0.1" bind:value={cs.props.fogMultiplier} style="width: 80px" />
            </FormField>
            <FormField label="ambient light scale">
              <input type="number" step="0.1" bind:value={cs.props.ambientLightScale} style="width: 80px" />
            </FormField>
            <FormField label="map disable distance">
              <input
                type="checkbox"
                checked={cs.props.mapDisableDistance != null}
                onchange={e =>
                  (cs.props.mapDisableDistance = (e.target as HTMLInputElement).checked ? 100 : undefined)}
              />
              {#if cs.props.mapDisableDistance != null}
                <input type="number" step="1" bind:value={cs.props.mapDisableDistance} style="width: 80px" />
              {/if}
            </FormField>
          {/if}

          <div class="pom-section">
            <div class="pom-header">parallax occlusion mapping</div>
            <FormField
              label="enable POM"
              help="Raymarches a height field to fake carved/inset geometry. Requires a procedural height shader and/or a height map. Needs triplanar mapping for height-map / normal-map combos; a procedural height field works with any mapping."
            >
              <input
                type="checkbox"
                checked={!!cs.options.pom}
                onchange={() => {
                  cs.options.pom = cs.options.pom ? undefined : { depth: 0.1 };
                }}
              />
            </FormField>

            {#if cs.options.pom}
              <FormField label="depth" help="Max carve depth in world units.">
                <input type="number" step="0.01" bind:value={cs.options.pom.depth} style="width: 80px" />
              </FormField>
              <FormField label="steps" help="Linear search step count. Default 24.">
                <input
                  type="number"
                  step="1"
                  min="1"
                  placeholder="24"
                  bind:value={cs.options.pom.steps}
                  style="width: 80px"
                />
              </FormField>
              <FormField
                label="tangent-space march"
                help="Marches in the mesh's tangent frame so relief follows the analytic UVs (rail_sweep arc-length U / profile V) along swept or curved meshes. Requires 'mesh uv' texture mapping and a tangent attribute; no normal map yet."
              >
                <input type="checkbox" bind:checked={cs.options.pom.tangentSpace} />
              </FormField>
              <FormField
                label="apply relief normal"
                help="Applies the carved relief's per-pixel normal to the shading normal so inset walls catch direct light even without a normal map."
              >
                <input type="checkbox" bind:checked={cs.options.pom.applyReliefNormal} />
              </FormField>
              <FormField
                label="bounded silhouette"
                help="Subtractive silhouette carving. Only works well on convex meshes."
              >
                <input type="checkbox" bind:checked={cs.options.pom.boundedSilhouette} />
              </FormField>
              <FormField
                label="refinement"
                help="Surface refinement after the linear search brackets the hit. 'binary' fixes banding on cliff/step height fields."
              >
                <select bind:value={cs.options.pom.refinement}>
                  <option value={undefined}>secant (default)</option>
                  <option value="secant">secant</option>
                  <option value="binary">binary</option>
                </select>
              </FormField>
              {#if cs.options.pom.refinement === 'binary'}
                <FormField label="refinement steps" help="Bisection count. Default 5; 4–6 is the sweet spot.">
                  <input
                    type="number"
                    step="1"
                    min="1"
                    placeholder="5"
                    bind:value={cs.options.pom.refinementSteps}
                    style="width: 80px"
                  />
                </FormField>
              {/if}
              <FormField
                label="LOD fade start"
                help="World-units distance at which POM starts fading to the flat base surface. Derived from depth if unset."
              >
                <input type="number" step="1" bind:value={cs.options.pom.lodFadeStart} style="width: 80px" />
              </FormField>
              <FormField label="LOD fade range" help="World-units range over which the LOD fade completes.">
                <input type="number" step="1" bind:value={cs.options.pom.lodFadeRange} style="width: 80px" />
              </FormField>
              <FormField
                label="normal eps"
                help="World-space floor for the analytic-normal finite-difference radius. Widen this when using a height map to avoid per-pixel normal noise (~3-4x texel size)."
              >
                <input type="number" step="0.001" bind:value={cs.options.pom.normalEps} style="width: 80px" />
              </FormField>
              {@render texField(
                'height map',
                'pomHeightMap',
                cs.props.pomHeightMap,
                h => (cs.props.pomHeightMap = h)
              )}
              <FormField
                label="debug viz"
                help="Replaces the final color with a diagnostic visualization of an intermediate POM quantity. 'samples' heat-maps per-fragment march cost (blue=cheap, red=worst case). 'skip' shows the refine decision (binary only): green=bisection skipped, red=full bisection, dark blue=no refinement."
              >
                <select bind:value={cs.options.pom.debug}>
                  <option value={undefined}>off</option>
                  <option value="heightmap">heightmap</option>
                  <option value="depth">depth</option>
                  <option value="normal">normal</option>
                  <option value="normalDelta">normalDelta</option>
                  <option value="axis">axis</option>
                  <option value="hit">hit</option>
                  <option value="samples">samples (cost heatmap)</option>
                  <option value="skip">skip (refine decision)</option>
                </select>
              </FormField>
            {/if}
          </div>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .properties-editor {
    padding: 16px;
    border-left: 1px solid #444;
    flex-grow: 1;
    overflow-y: auto;
    font-size: 12px;
    position: relative;
  }

  .placeholder {
    color: #888;
    font-style: italic;
    padding: 12px 0;
  }

  .save-to-library {
    background: none;
    border: none;
    cursor: pointer;
    font-size: 12px;
    color: #ddd;
    text-decoration: underline;
    padding: 4px 8px;
    white-space: nowrap;
    flex-shrink: 0;
    position: absolute;
    top: 8px;
    left: 8px;
  }

  .save-to-library:hover {
    color: #8bb8ff;
  }

  .derived-map-controls {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .derived-map-controls span {
    color: #aaa;
  }

  .derived-map-controls button {
    background: none;
    border: 1px solid #555;
    color: #f0f0f0;
    cursor: pointer;
    font-size: 12px;
    padding: 2px 6px;
  }

  .section-divider {
    border-top: 1px solid #333;
    margin: 10px 0;
  }

  .advanced-options {
    margin-top: 16px;
    border-top: 1px solid #444;
    padding-top: 8px;
  }

  .advanced-toggle {
    background: none;
    border: none;
    color: #aaa;
    cursor: pointer;
    padding: 0;
    margin-bottom: 16px;
    font-size: 12px;
  }

  .advanced-content {
    padding-left: 16px;
    font-size: 12px;
  }

  .pom-section {
    margin-top: 16px;
    border-top: 1px solid #444;
    padding-top: 8px;
  }

  .pom-header {
    color: #aaa;
    margin-bottom: 12px;
    font-size: 12px;
  }

  .edit-shaders {
    background: #333;
    border: 1px solid #555;
    color: #f0f0f0;
    padding: 2px 2px 3px 4px;
    cursor: pointer;
    margin-bottom: 16px;
    width: 180px;
    font-size: 12px;
  }

  .edit-shaders:hover {
    background: #3d3d3d;
  }

  .toggle-group {
    display: flex;
  }

  .toggle-group button {
    background: #333;
    border: 1px solid #555;
    color: #f0f0f0;
    padding: 4px 8px;
    cursor: pointer;
    font-size: 12px;
  }

  .toggle-group button.selected {
    background: #555;
    border-color: #777;
  }

  .toggle-group button:not(:last-child) {
    border-right: none;
  }
</style>
