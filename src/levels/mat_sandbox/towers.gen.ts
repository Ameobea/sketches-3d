import type { GeneratorFn } from 'src/viz/levelDef/generatorTypes';

const mulberry32 = (seed: number) => () => {
  seed |= 0;
  seed = (seed + 0x6d2b79f5) | 0;
  let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
};

/**
 * Scatters parametric tower variants in a ring. Params are quantized to coarse steps so some
 * instances collide onto the same variant and exercise the bake dedupe.
 */
const fn: GeneratorFn = ({ params }) => {
  const asset = params.asset as string;
  const material = params.material as string | undefined;
  const count = (params.count as number) ?? 8;
  const ringRadius = (params.ringRadius as number) ?? 24;
  const rand = mulberry32(0x70e35);

  return {
    objects: Array.from({ length: count }, (_, i) => {
      const angle = (i / count) * Math.PI * 2;
      const segments = 6 + 5 * Math.floor(rand() * 3);
      const radius = 0.75 + Math.floor(rand() * 2);
      const twist = Math.floor(rand() * 2) * 0.5;
      return {
        id: `gen_tower_${i}`,
        asset,
        ...(material ? { material } : {}),
        inputs: {
          segments: { type: 'int' as const, value: segments },
          radius: { type: 'float' as const, value: radius },
          twist_per_level: { type: 'float' as const, value: twist },
        },
        position: [Math.cos(angle) * ringRadius, 0, Math.sin(angle) * ringRadius] as [
          number,
          number,
          number,
        ],
        rotation: [0, rand() * Math.PI * 2, 0] as [number, number, number],
        scale: [1, 0.8 + rand() * 0.5, 1] as [number, number, number],
      };
    }),
  };
};

export default fn;
