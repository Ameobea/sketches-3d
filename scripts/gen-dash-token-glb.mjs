/**
 * Extracts the `dash_token` subtree (core + rings) from `static/plats.glb` and
 * writes it out as a standalone, origin-centered, material-stripped
 * `static/dash_token.glb` — the reusable default dash-token mesh loaded on demand
 * by level-def scenes that place dash tokens.
 *
 * Run: node scripts/gen-dash-token-glb.mjs
 */
import { readFileSync, writeFileSync } from 'fs';
import * as THREE from 'three';
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js';
import { GLTFExporter } from 'three/examples/jsm/exporters/GLTFExporter.js';

// GLTFExporter's binary path uses the DOM FileReader; provide a minimal Node shim.
if (typeof globalThis.FileReader === 'undefined') {
  globalThis.FileReader = class {
    readAsArrayBuffer(blob) {
      blob.arrayBuffer().then(ab => {
        this.result = ab;
        this.onloadend?.();
      });
    }
  };
}

const SRC = 'static/plats.glb';
const OUT = 'static/dash_token.glb';

const buf = readFileSync(SRC);
const ab = buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.byteLength);

const loader = new GLTFLoader();
loader.parse(
  ab,
  '',
  gltf => {
    const src = gltf.scene.getObjectByName('dash_token');
    if (!src) throw new Error(`"dash_token" not found in ${SRC}`);

    const token = src.clone(true);
    token.position.set(0, 0, 0);
    token.rotation.set(0, 0, 0);
    token.scale.set(1, 1, 1);
    token.updateMatrix();

    // Strip plats' textured materials; the runtime assigns real core/ring materials.
    token.traverse(o => {
      if (o.isMesh) {
        o.material = new THREE.MeshStandardMaterial({ color: 0xffffff });
      }
    });

    const exporter = new GLTFExporter();
    exporter.parse(
      token,
      result => {
        writeFileSync(OUT, Buffer.from(result));
        const meshes = [];
        token.traverse(o => o.isMesh && meshes.push(o.name));
        console.log(`Wrote ${OUT} (${result.byteLength} bytes) — meshes: ${meshes.join(', ')}`);
      },
      err => {
        throw err;
      },
      { binary: true }
    );
  },
  err => {
    throw err;
  }
);
