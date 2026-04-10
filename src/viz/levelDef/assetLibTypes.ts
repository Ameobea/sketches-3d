/** A single .geo file available from the shared asset library. */
export interface AssetLibFile {
  /** Filename without extension, e.g. "gear1" */
  name: string;
  /** `__ASSETS__/`-prefixed path suitable for storing in a geoscript asset def's `file` field. */
  path: string;
}

/** A folder within the shared asset library mesh tree. */
export interface AssetLibFolder {
  name: string;
  files: AssetLibFile[];
  subfolders: AssetLibFolder[];
}
