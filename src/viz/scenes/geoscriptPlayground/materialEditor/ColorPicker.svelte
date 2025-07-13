<script lang="ts">
  import type { RGBColor } from 'src/geoscript/materials';

  let { color = $bindable() }: { color: RGBColor } = $props();

  const toHex = (c: number) => Math.round(c * 255).toString(16).padStart(2, '0');
  const fromHex = (hex: string) => parseInt(hex, 16) / 255;

  let hexColor = $derived(`#${toHex(color.r)}${toHex(color.g)}${toHex(color.b)}`);

  const onColorChange = (e: Event) => {
    const newHex = (e.target as HTMLInputElement).value;
    color = {
      r: fromHex(newHex.slice(1, 3)),
      g: fromHex(newHex.slice(3, 5)),
      b: fromHex(newHex.slice(5, 7)),
    };
  };
</script>

<input type="color" value={hexColor} oninput={onColorChange} />
