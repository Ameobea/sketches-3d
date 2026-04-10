/// <reference types="@sveltejs/kit" />

declare module 'n8ao';
// declare module 'svelte-codemirror-editor';
declare module 'graphviz-builder';
declare module 'three/examples/jsm/exporters/GLTFExporter' {
  export class GLTFExporter {
    parse(
      input: object,
      onCompleted: (gltf: object | ArrayBuffer) => void,
      onError: (error: unknown) => void,
      options?: object
    ): void;
  }
}
declare module 'https://ameo.dev/web-synth-headless/headless.js' {
  export function initHeadlessWebSynth(args: unknown): Promise<any>;
}
declare module 'virtual:behaviors' {
  import type { BehaviorFn } from 'src/viz/sceneRuntime/types';
  const behaviors: Record<string, BehaviorFn>;
  export default behaviors;
}
