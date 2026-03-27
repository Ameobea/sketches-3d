export enum Score {
  SPlus = 0,
  S = 1,
  A = 2,
  B = 3,
  C = 4,
}

export type ScoreThresholds = { [K in Exclude<Score, Score.C>]: number };
