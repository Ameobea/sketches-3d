import { AsyncOnce } from 'src/viz/util/AsyncOnce';
import WasmURL from './clipper2z.wasm?url';

const Clipper2Wasm = new AsyncOnce(() =>
  import('./clipper2z.js')
    .then(mod => {
      (mod.default as any).locateFile = (_path: string) => WasmURL;
      return mod.default;
    })
    .then(mod => mod({ locateFile: (_path: string) => WasmURL }))
);

export const initClipper2 = (): Promise<void> | true => {
  if (Clipper2Wasm.isSome()) {
    return true;
  }
  return Clipper2Wasm.get().then(() => {});
};

export const clipper2_get_is_loaded = (): boolean => Clipper2Wasm.isSome();

export const enum Clipper2JoinType {
  Square = 0,
  Bevel = 1,
  Round = 2,
  Miter = 3,
  Superellipse = 4,
  Knob = 5,
  Step = 6,
  Spike = 7,
}

export const enum Clipper2EndType {
  Polygon = 0,
  Joined = 1,
  Butt = 2,
  Square = 3,
  Round = 4,
  Superellipse = 5,
  Triangle = 6,
  Arrow = 7,
  Teardrop = 8,
}

export const enum Clipper2FillRule {
  EvenOdd = 0,
  NonZero = 1,
  Positive = 2,
  Negative = 3,
}

const buildPathD = (Clipper2: any, coords: Float64Array): any => {
  const path = new Clipper2.PathD();
  const pointCount = coords.length / 2;
  for (let i = 0; i < pointCount; i++) {
    const pt = new Clipper2.PointD(coords[i * 2], coords[i * 2 + 1], 0);
    path.push_back(pt);
  }
  return path;
};

const buildPathsD = (Clipper2: any, coords: Float64Array, pathLengths: Uint32Array): any => {
  const paths = new Clipper2.PathsD();
  let offset = 0;
  for (let i = 0; i < pathLengths.length; i++) {
    const len = pathLengths[i];
    const pathCoords = coords.subarray(offset, offset + len * 2);
    const path = buildPathD(Clipper2, pathCoords);
    paths.push_back(path);
    path.delete();
    offset += len * 2;
  }
  return paths;
};

const extractPathD = (path: any): Float64Array => {
  const size = path.size();
  const result = new Float64Array(size * 2);
  for (let i = 0; i < size; i++) {
    const pt = path.get(i);
    result[i * 2] = pt.x;
    result[i * 2 + 1] = pt.y;
  }
  return result;
};

const extractPathsD = (paths: any): { coords: Float64Array; pathLengths: Uint32Array } => {
  const pathCount = paths.size();
  const pathLengths = new Uint32Array(pathCount);

  let totalPoints = 0;
  for (let i = 0; i < pathCount; i++) {
    const path = paths.get(i);
    pathLengths[i] = path.size();
    totalPoints += pathLengths[i];
  }

  const coords = new Float64Array(totalPoints * 2);
  let offset = 0;
  for (let i = 0; i < pathCount; i++) {
    const path = paths.get(i);
    const size = path.size();
    for (let j = 0; j < size; j++) {
      const pt = path.get(j);
      coords[offset++] = pt.x;
      coords[offset++] = pt.y;
    }
  }

  return { coords, pathLengths };
};

const extractVectorDouble = (Clipper2: any, vec: any): Float64Array => {
  if (!vec) {
    return new Float64Array();
  }
  if (Array.isArray(vec)) {
    return Float64Array.from(vec);
  }
  if (typeof vec.size === 'function' && typeof vec.data === 'function') {
    const length = vec.size();
    const ptr = vec.data();
    const HEAPF64 = Clipper2.HEAPF64 as Float64Array;
    const result = HEAPF64.subarray(ptr / 8, ptr / 8 + length).slice();
    if (typeof vec.delete === 'function') {
      vec.delete();
    }
    return result;
  }
  if (typeof vec.length === 'number') {
    return Float64Array.from(vec);
  }
  return new Float64Array();
};

let OutputPaths: { coords: Float64Array; pathLengths: Uint32Array } | null = null;
let OutputCriticalTValues: Float64Array | null = null;

export const clipper2_get_output_coords = (): Float64Array => {
  if (!OutputPaths) {
    throw new Error('No Clipper2 output paths set');
  }
  return OutputPaths.coords;
};

export const clipper2_get_output_path_lengths = (): Uint32Array => {
  if (!OutputPaths) {
    throw new Error('No Clipper2 output paths set');
  }
  return OutputPaths.pathLengths;
};

export const clipper2_clear_output = (): void => {
  OutputPaths = null;
  OutputCriticalTValues = null;
};

export const clipper2_get_output_critical_t_values = (): Float64Array => {
  if (!OutputCriticalTValues) {
    return new Float64Array();
  }
  return OutputCriticalTValues;
};

/**
 * Simplifies a single path using the given epsilon and closed state.
 * Returns the simplified path (caller must delete it).
 */
const simplifyPathD = (Clipper2: any, path: any, epsilon: number, isClosed: boolean): any => {
  const tempPaths = new Clipper2.PathsD();
  tempPaths.push_back(path);
  const simplified = Clipper2.SimplifyPathsD(tempPaths, epsilon, isClosed);
  tempPaths.delete();

  // Extract the first (and only) path from the result
  if (simplified.size() > 0) {
    const result = simplified.get(0);
    // We need to copy it since we're deleting the container
    const copy = new Clipper2.PathD();
    for (let i = 0; i < result.size(); i++) {
      const pt = result.get(i);
      copy.push_back(new Clipper2.PointD(pt.x, pt.y, 0));
    }
    simplified.delete();
    return copy;
  }
  simplified.delete();
  return new Clipper2.PathD();
};

/**
 * Offsets paths by the specified delta amount.
 *
 * @param coords - Flattened array of x,y coordinates for all paths
 * @param pathLengths - Number of points in each sub-path
 * @param pathIsClosed - Boolean flags (as Uint8Array, 0=open, non-zero=closed) indicating if each sub-path is closed.
 *                       Closed paths represent polygons; open paths represent line segments.
 *                       This affects both simplification and how end caps are applied.
 * @param delta - The offset distance (positive = outward/inflate, negative = inward/deflate)
 * @param joinType - How to join path segments at corners
 * @param endType - How to cap the ends of open paths (ignored for closed paths which use Polygon end type)
 * @param joinAngleThreshold - Minimum join angle in radians for special joins; 0 disables
 * @param criticalAngleThreshold - Angle threshold in radians for critical point detection
 * @param criticalSegmentFraction - Endpoints of non-colinear segments longer than `(path_perimeter * criticalSegmentFraction)`
 *                                  are considered critical regardless of angle. Set to 1 to disable; 0 = auto.
 * @param simplifyEpsilon - Epsilon for pre-offset simplification. Set to 0 to disable simplification.
 *                          Defaults to 0.01. Clipper2 docs strongly recommend simplifying before offset.
 */
export const clipper2_offset_paths = (
  coords: Float64Array,
  pathLengths: Uint32Array,
  pathIsClosed: Uint8Array,
  delta: number,
  joinType: Clipper2JoinType,
  endType: Clipper2EndType,
  miterLimit: number = 2.0,
  arcTolerance: number = 0.0,
  preserveCollinear: boolean = false,
  reverseSolution: boolean = false,
  stepCount: number = 0,
  superellipseExponent: number = 2.5,
  endExtensionScale: number = 1.0,
  arrowBackSweep: number = 0.0,
  teardropPinch: number = 0.5,
  joinAngleThreshold: number = 0.0, // radians
  chebyshevSpacing: boolean = false,
  simplifyEpsilon: number = 0.0001,
  // TODO: these are not routed through yet from the Rust side
  criticalAngleThreshold: number = 0.35, // radians
  criticalSegmentFraction: number = 0.15 // input units (0 = auto)
): void => {
  const Clipper2 = Clipper2Wasm.getSync();

  if (pathLengths.length !== pathIsClosed.length) {
    throw new Error(
      `pathLengths.length (${pathLengths.length}) must equal pathIsClosed.length (${pathIsClosed.length})`
    );
  }

  // TODO: need to set up some basic caching so we re-use the instance here if parameters are the same as last time
  //
  // Might need to go even further and add in extra handling for the case where only delta changes since that's
  // a core case for the 3D beveling stuff.
  const clipperOffset = new Clipper2.ClipperOffsetD(miterLimit, arcTolerance);
  clipperOffset.SetPreserveCollinear(preserveCollinear);
  clipperOffset.SetReverseSolution(reverseSolution);
  clipperOffset.SetStepCount(stepCount);
  clipperOffset.SetSuperellipseExponent(superellipseExponent);
  clipperOffset.SetEndCapParams(endExtensionScale, arrowBackSweep);
  clipperOffset.SetTeardropPinch(teardropPinch);
  clipperOffset.SetJoinAngleThreshold(joinAngleThreshold);
  clipperOffset.SetChebyshevSpacing(chebyshevSpacing);
  if (typeof clipperOffset.SetAngleThreshold === 'function') {
    clipperOffset.SetAngleThreshold(criticalAngleThreshold);
  }
  if (typeof clipperOffset.SetCriticalSegmentFraction === 'function') {
    clipperOffset.SetCriticalSegmentFraction(criticalSegmentFraction);
  }

  let offset = 0;
  for (let i = 0; i < pathLengths.length; i++) {
    const len = pathLengths[i];
    const isClosed = pathIsClosed[i] !== 0;
    const pathCoords = coords.subarray(offset, offset + len * 2);

    let path = buildPathD(Clipper2, pathCoords);

    if (simplifyEpsilon > 0) {
      const simplified = simplifyPathD(Clipper2, path, simplifyEpsilon, isClosed);
      path.delete();
      path = simplified;
    }

    const effectiveEndType = isClosed ? Clipper2EndType.Polygon : endType;

    const singlePath = new Clipper2.PathsD();
    singlePath.push_back(path);
    clipperOffset.AddPaths(singlePath, { value: joinType }, { value: effectiveEndType });
    singlePath.delete();
    path.delete();

    offset += len * 2;
  }

  const result = clipperOffset.Execute(delta);

  OutputPaths = extractPathsD(result);
  OutputCriticalTValues =
    typeof clipperOffset.GetCriticalTValues === 'function'
      ? extractVectorDouble(Clipper2, clipperOffset.GetCriticalTValues())
      : new Float64Array();
  clipperOffset.delete();
  result.delete();
};

/**
 * This function removes vertices that are less than the specified epsilon distance from an imaginary line that
 * passes through its 2 adjacent vertices.
 *
 * Logically, smaller epsilon values will be less aggressive in removing vertices than larger epsilon values.
 *
 * This function is strongly recommended before offsetting (ie before inflating/shrinking) when paths may contain
 * redundant segments.
 *
 * https://www.angusj.com/clipper2/Docs/Units/Clipper/Functions/SimplifyPaths.htm
 */
export const clipper2_simplify_paths = (
  coords: Float64Array,
  pathLengths: Uint32Array,
  epsilon: number,
  isClosedPath: boolean
): void => {
  const Clipper2 = Clipper2Wasm.getSync();

  const paths = buildPathsD(Clipper2, coords, pathLengths);
  const result = Clipper2.SimplifyPathsD(paths, epsilon, isClosedPath);
  paths.delete();

  OutputPaths = extractPathsD(result);
  OutputCriticalTValues = new Float64Array();
  result.delete();
};

/**
 * Removes the vertices between adjacent collinear segments. It will also remove duplicate vertices
 * (adjacent vertices with identical coordinates).
 */
export const clipper2_trim_collinear = (
  coords: Float64Array,
  pathLengths: Uint32Array,
  isClosedPath: boolean
): void => {
  const Clipper2 = Clipper2Wasm.getSync();

  const resultPaths: Float64Array[] = [];
  const resultLengths: number[] = [];

  let offset = 0;
  for (let i = 0; i < pathLengths.length; i++) {
    const len = pathLengths[i];
    const pathCoords = coords.subarray(offset, offset + len * 2);
    const path = buildPathD(Clipper2, pathCoords);

    const trimmed = Clipper2.TrimCollinearD(path, 2, isClosedPath);
    path.delete();

    const extracted = extractPathD(trimmed);
    trimmed.delete();

    if (extracted.length > 0) {
      resultPaths.push(extracted);
      resultLengths.push(extracted.length / 2);
    }

    offset += len * 2;
  }

  const totalCoords = resultPaths.reduce((sum, p) => sum + p.length, 0);
  const combinedCoords = new Float64Array(totalCoords);
  let outOffset = 0;
  for (const p of resultPaths) {
    combinedCoords.set(p, outOffset);
    outOffset += p.length;
  }

  OutputPaths = { coords: combinedCoords, pathLengths: new Uint32Array(resultLengths) };
  OutputCriticalTValues = new Float64Array();
};

/**
 * Computes the union of two sets of paths.
 * The union contains all areas that are inside either subject or clip paths.
 *
 * @param subjectCoords - Flattened array of x,y coordinates for subject paths
 * @param subjectPathLengths - Number of points in each subject sub-path
 * @param clipCoords - Flattened array of x,y coordinates for clip paths
 * @param clipPathLengths - Number of points in each clip sub-path
 * @param fillRule - Fill rule for determining path interiors
 */
export const clipper2_union_paths = (
  subjectCoords: Float64Array,
  subjectPathLengths: Uint32Array,
  clipCoords: Float64Array,
  clipPathLengths: Uint32Array,
  fillRule: Clipper2FillRule = Clipper2FillRule.NonZero
): void => {
  const Clipper2 = Clipper2Wasm.getSync();

  const subjects = buildPathsD(Clipper2, subjectCoords, subjectPathLengths);
  const clips = buildPathsD(Clipper2, clipCoords, clipPathLengths);

  const result = Clipper2.UnionD(subjects, clips, { value: fillRule });

  subjects.delete();
  clips.delete();

  OutputPaths = extractPathsD(result);
  OutputCriticalTValues = new Float64Array();
  result.delete();
};

/**
 * Computes the self-union of a single set of paths.
 * This merges overlapping paths and removes self-intersections.
 *
 * @param coords - Flattened array of x,y coordinates for all paths
 * @param pathLengths - Number of points in each sub-path
 * @param fillRule - Fill rule for determining path interiors
 */
export const clipper2_union_self = (
  coords: Float64Array,
  pathLengths: Uint32Array,
  fillRule: Clipper2FillRule = Clipper2FillRule.NonZero
): void => {
  const Clipper2 = Clipper2Wasm.getSync();

  const subjects = buildPathsD(Clipper2, coords, pathLengths);

  const result = Clipper2.UnionSelfD(subjects, { value: fillRule });

  subjects.delete();

  OutputPaths = extractPathsD(result);
  OutputCriticalTValues = new Float64Array();
  result.delete();
};

/**
 * Computes the intersection of two sets of paths.
 * The intersection contains only areas that are inside both subject and clip paths.
 *
 * @param subjectCoords - Flattened array of x,y coordinates for subject paths
 * @param subjectPathLengths - Number of points in each subject sub-path
 * @param clipCoords - Flattened array of x,y coordinates for clip paths
 * @param clipPathLengths - Number of points in each clip sub-path
 * @param fillRule - Fill rule for determining path interiors
 */
export const clipper2_intersect_paths = (
  subjectCoords: Float64Array,
  subjectPathLengths: Uint32Array,
  clipCoords: Float64Array,
  clipPathLengths: Uint32Array,
  fillRule: Clipper2FillRule = Clipper2FillRule.NonZero
): void => {
  const Clipper2 = Clipper2Wasm.getSync();

  const subjects = buildPathsD(Clipper2, subjectCoords, subjectPathLengths);
  const clips = buildPathsD(Clipper2, clipCoords, clipPathLengths);

  const result = Clipper2.IntersectD(subjects, clips, { value: fillRule });

  subjects.delete();
  clips.delete();

  OutputPaths = extractPathsD(result);
  OutputCriticalTValues = new Float64Array();
  result.delete();
};

/**
 * Computes the difference of two sets of paths (subject minus clip).
 * The difference contains areas that are inside subject paths but not inside clip paths.
 *
 * @param subjectCoords - Flattened array of x,y coordinates for subject paths
 * @param subjectPathLengths - Number of points in each subject sub-path
 * @param clipCoords - Flattened array of x,y coordinates for clip paths
 * @param clipPathLengths - Number of points in each clip sub-path
 * @param fillRule - Fill rule for determining path interiors
 */
export const clipper2_difference_paths = (
  subjectCoords: Float64Array,
  subjectPathLengths: Uint32Array,
  clipCoords: Float64Array,
  clipPathLengths: Uint32Array,
  fillRule: Clipper2FillRule = Clipper2FillRule.NonZero
): void => {
  const Clipper2 = Clipper2Wasm.getSync();

  const subjects = buildPathsD(Clipper2, subjectCoords, subjectPathLengths);
  const clips = buildPathsD(Clipper2, clipCoords, clipPathLengths);

  const result = Clipper2.DifferenceD(subjects, clips, { value: fillRule });

  subjects.delete();
  clips.delete();

  OutputPaths = extractPathsD(result);
  OutputCriticalTValues = new Float64Array();
  result.delete();
};

/**
 * Computes the exclusive-or (XOR) of two sets of paths.
 * The XOR contains areas that are inside either subject or clip paths, but not both.
 *
 * @param subjectCoords - Flattened array of x,y coordinates for subject paths
 * @param subjectPathLengths - Number of points in each subject sub-path
 * @param clipCoords - Flattened array of x,y coordinates for clip paths
 * @param clipPathLengths - Number of points in each clip sub-path
 * @param fillRule - Fill rule for determining path interiors
 */
export const clipper2_xor_paths = (
  subjectCoords: Float64Array,
  subjectPathLengths: Uint32Array,
  clipCoords: Float64Array,
  clipPathLengths: Uint32Array,
  fillRule: Clipper2FillRule = Clipper2FillRule.NonZero
): void => {
  const Clipper2 = Clipper2Wasm.getSync();

  const subjects = buildPathsD(Clipper2, subjectCoords, subjectPathLengths);
  const clips = buildPathsD(Clipper2, clipCoords, clipPathLengths);

  const result = Clipper2.XorD(subjects, clips, { value: fillRule });

  subjects.delete();
  clips.delete();

  OutputPaths = extractPathsD(result);
  OutputCriticalTValues = new Float64Array();
  result.delete();
};
