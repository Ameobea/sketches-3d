<script lang="ts">
  import { z } from 'zod';

  import type { ReverseColorRampParams } from 'src/viz/shaders/reverseColorRamp';

  const {
    onclose,
    onsave,
  }: {
    onclose: () => void;
    onsave: (params: ReverseColorRampParams) => void;
  } = $props();

  let jsonInput = $state('');
  let parseError = $state<string | null>(null);
  let parseSuccess = $state(false);

  const ReverseColorRampParamsSchema = z.object({
    colorA_srgb: z.tuple([z.number(), z.number(), z.number()]),
    colorB_srgb: z.tuple([z.number(), z.number(), z.number()]),
    vMin: z.number(),
    vMax: z.number(),
    curveSteepness: z.number().min(1.0),
    curveOffset: z.number().min(0.0).max(1.0),
    perpSigma: z.number(),
    baseFallback: z.number(),
  });

  function validateAndSave() {
    parseError = null;
    parseSuccess = false;
    if (!jsonInput) {
      return;
    }
    try {
      const json = JSON.parse(jsonInput);
      const parsed = ReverseColorRampParamsSchema.parse(json);
      onsave(parsed);
      parseSuccess = true;
    } catch (e: any) {
      if (e instanceof z.ZodError) {
        parseError = e.issues.map(err => `${err.path.join('.')}: ${err.message}`).join('\n');
      } else {
        parseError = e.message;
      }
    }
  }

  $effect(() => {
    validateAndSave();
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="modal-overlay" onclick={onclose}>
  <div class="modal-content" onclick={e => e.stopPropagation()}>
    <button class="close-button" onclick={onclose}>×</button>
    <h2>Configure Derived Map</h2>
    <p>
      This feature uses a reverse color ramp to derive a PBR map (like roughness or metalness) from the base
      color texture. You can use the external <a
        href="https://texture-utils.ameo.design/reverse-color-ramp/"
        target="_blank"
        rel="noopener noreferrer"
      >
        Reverse Color Ramp Tool
      </a>
      to generate the necessary JSON configuration.
    </p>
    <p>
      Paste the JSON from the tool into the text area below. It will be validated and saved automatically.
    </p>
    <textarea bind:value={jsonInput} placeholder="Paste JSON here..."></textarea>
    {#if parseError}
      <div class="error-message">{parseError}</div>
    {/if}
    {#if parseSuccess}
      <div class="success-message">
        Configuration parsed and saved successfully! You can close this dialog.
      </div>
    {/if}
  </div>
</div>

<style>
  .modal-overlay {
    position: fixed;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    justify-content: center;
    align-items: center;
    z-index: 1000;
  }

  .modal-content {
    background: #222;
    border: 1px solid #555;
    padding: 24px;
    width: 600px;
    max-width: 90%;
    color: #f0f0f0;
    position: relative;
  }

  .close-button {
    position: absolute;
    top: 8px;
    right: 8px;
    background: none;
    border: none;
    color: #aaa;
    font-size: 18px;
    cursor: pointer;
  }

  h2 {
    margin-top: 0;
    font-size: 18px;
    color: #fff;
  }

  p {
    font-size: 12px;
    line-height: 1.5;
    color: #ccc;
  }

  a {
    color: #8af;
    text-decoration: none;
  }
  a:hover {
    text-decoration: underline;
  }

  textarea {
    width: 100%;
    height: 200px;
    background: #111;
    border: 1px solid #555;
    color: #f0f0f0;
    font-family: monospace;
    font-size: 12px;
    padding: 8px;
    margin-top: 16px;
    box-sizing: border-box;
  }

  .error-message {
    color: #ff8a8a;
    margin-top: 12px;
    white-space: pre-wrap;
    font-size: 12px;
  }

  .success-message {
    color: #8aff8a;
    margin-top: 12px;
    font-size: 12px;
  }
</style>
