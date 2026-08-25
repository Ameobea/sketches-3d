export type PlayerColliderShape = 'capsule' | 'cylinder' | 'sphere';

type NumericRange = {
  min?: number;
  max?: number;
  minExclusive?: boolean;
  maxExclusive?: boolean;
};

export const validateFiniteRange = (name: string, value: number, range: NumericRange = {}): number => {
  if (!Number.isFinite(value)) {
    throw new Error(`${name} must be finite; got ${value}`);
  }

  const { min, max, minExclusive = false, maxExclusive = false } = range;
  if (min !== undefined && (minExclusive ? value <= min : value < min)) {
    throw new Error(`${name} must be ${minExclusive ? '>' : '>='} ${min}; got ${value}`);
  }
  if (max !== undefined && (maxExclusive ? value >= max : value > max)) {
    throw new Error(`${name} must be ${maxExclusive ? '<' : '<='} ${max}; got ${value}`);
  }
  return value;
};

export const validateDampingVector = (
  name: string,
  value: { x: number; y: number; z: number } | readonly [number, number, number]
): void => {
  const components = 'x' in value ? [value.x, value.y, value.z] : value;
  components.forEach((component, index) =>
    validateFiniteRange(`${name}.${['x', 'y', 'z'][index]}`, component, { min: 0, max: 1 })
  );
};

export const getPlayerColliderCenterToFeetOffset = (
  shape: PlayerColliderShape,
  height: number,
  radius: number
): number => {
  validateFiniteRange('player.colliderSize.height', height, { min: 0, minExclusive: true });
  validateFiniteRange('player.colliderSize.radius', radius, { min: 0, minExclusive: true });

  switch (shape) {
    case 'capsule':
      return height / 2 + radius;
    case 'cylinder':
      return height / 2;
    case 'sphere':
      return radius;
    default:
      shape satisfies never;
      throw new Error(`Unknown player collider shape: ${shape}`);
  }
};
