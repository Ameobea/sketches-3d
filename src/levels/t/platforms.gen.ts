import * as THREE from 'three';

import { generateParkourPlatforms } from 'src/viz/parkour/platformGen';
import type { GeneratorFn } from 'src/viz/levelDef/generatorTypes';

const fn: GeneratorFn = ({ physics, params }) => {
  const rawPts = params.controlPoints as [number, number, number][];
  const pts = rawPts.map(([x, y, z]) => new THREE.Vector3(x, y, z));
  const spline = new THREE.CatmullRomCurve3(pts);

  const positions = generateParkourPlatforms(
    spline,
    {
      gravity: physics.gravity ?? 30,
      jumpVelocity: physics.player?.jumpVelocity ?? 12,
      inAirSpeed: physics.player?.moveSpeed?.inAir ?? 13,
      tickRate: physics.simulationTickRate ?? 160,
      gravityShaping: physics.gravityShaping,
    },
    (params.fudgeFactor as number) ?? 1.0
  );

  const asset = params.asset as string;
  const material = params.material as string | undefined;

  return {
    objects: [
      {
        id: 'gen_platforms',
        position: [20, 20, 0] as [number, number, number],
        children: positions.map((pos, i) => ({
          id: `gen_platform_${i}`,
          asset,
          position: [pos.x, pos.y, pos.z] as [number, number, number],
          ...(material ? { material } : {}),
        })),
      },
    ],
  };
};

export default fn;
