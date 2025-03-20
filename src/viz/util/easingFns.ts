export enum EasingFnType {
  Linear,
  InOutCubic,
  InOutQuint,
  OutCubic,
  OutQuint,
}

/**
 * Expects `t` to be in the range [0, 1].
 */
export type EasingFn = (t: number) => number;

export const buildEasingFn = (fnType: EasingFnType): EasingFn => {
  switch (fnType) {
    case EasingFnType.Linear:
      return (t: number) => t;
    case EasingFnType.InOutCubic:
      return (t: number) => (t < 0.5 ? 4 * t ** 3 : 1 - (-2 * t + 2) ** 3 / 2);
    case EasingFnType.InOutQuint:
      return (t: number) => (t < 0.5 ? 16 * t ** 5 : 1 - (-2 * t + 2) ** 5 / 2);
    case EasingFnType.OutCubic:
      return (t: number) => --t * t * t + 1;
    case EasingFnType.OutQuint:
      return (t: number) => --t * t * t * t * t + 1;
    default:
      fnType satisfies never;
      throw new Error(`Unknown easing function type: ${fnType}`);
  }
};
