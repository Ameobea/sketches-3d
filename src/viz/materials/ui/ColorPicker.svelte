<script lang="ts">
  // Operates directly on the stored 0xRRGGBB sRGB int, matching how the `<input type=color>` widget
  // interprets hex — so the swatch is colorimetrically the rendered surface color.
  let { value, onchange }: { value: number | undefined; onchange: (v: number) => void } = $props();

  let hexColor = $derived('#' + ((value ?? 0x808080) >>> 0).toString(16).padStart(6, '0'));
</script>

<input
  type="color"
  value={hexColor}
  oninput={e => onchange(parseInt((e.target as HTMLInputElement).value.slice(1), 16))}
/>
