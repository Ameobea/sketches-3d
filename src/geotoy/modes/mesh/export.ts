import * as THREE from 'three';

import { CustomShaderMaterial } from 'src/viz/shaders/customShader';
import { AsyncOnce } from 'src/viz/util/AsyncOnce';
import { download } from 'src/viz/util/util';

const GLTFExporterP = new AsyncOnce(() =>
  import('three/examples/jsm/exporters/GLTFExporter.js').then(mod => mod.GLTFExporter)
);
const OBJExporterP = new AsyncOnce(() =>
  import('three/examples/jsm/exporters/OBJExporter.js').then(mod => mod.OBJExporter)
);

const getExportableObjects = (objects: (THREE.Mesh | THREE.Line | THREE.Light)[]): THREE.Object3D[] =>
  objects
    .filter(object => object instanceof THREE.Mesh || object instanceof THREE.Light)
    .map(object => {
      const clone = object.clone();
      if (clone instanceof THREE.Mesh && clone.material) {
        const newMaterial = new THREE.MeshStandardMaterial();
        const oldMaterial = clone.material;
        if (oldMaterial instanceof CustomShaderMaterial) {
          if (oldMaterial.color) {
            newMaterial.color.copy(oldMaterial.color);
          }
          if (typeof oldMaterial.uniforms?.metalness?.value === 'number') {
            newMaterial.metalness = oldMaterial.uniforms.metalness.value;
          }
          if (typeof oldMaterial.uniforms?.roughness?.value === 'number') {
            newMaterial.roughness = oldMaterial.uniforms.roughness.value;
          }
        }
        clone.material = newMaterial;
      }
      return clone;
    });

export type MeshExportFormat = 'glb' | 'gltf' | 'obj';

/**
 * Headless variant of the export buttons: serialize the rendered objects to the given
 * format and return the raw bytes/text (no download). Used by `geotoy eval`.
 */
export const exportObjectsToData = async (
  objects: (THREE.Mesh | THREE.Line | THREE.Light)[],
  format: MeshExportFormat
): Promise<{ binary: Uint8Array } | { text: string }> => {
  const exportableObjects = getExportableObjects(objects);
  const sceneToExport = new THREE.Scene();
  for (const obj of exportableObjects) sceneToExport.add(obj);

  if (format === 'obj') {
    const exporter = new (await OBJExporterP.get())();
    return { text: exporter.parse(sceneToExport) };
  }

  const exporter = new (await GLTFExporterP.get())();
  return new Promise((resolve, reject) => {
    exporter.parse(
      sceneToExport,
      result =>
        result instanceof ArrayBuffer
          ? resolve({ binary: new Uint8Array(result) })
          : resolve({ text: JSON.stringify(result) }),
      err => reject(err instanceof Error ? err : new Error(String(err))),
      { binary: format === 'glb', trs: false, onlyVisible: true, truncateDrawRange: true }
    );
  });
};

export const exportGLTF = async (
  objects: (THREE.Mesh | THREE.Line | THREE.Light)[],
  binary: boolean
): Promise<void> => {
  const exporter = new (await GLTFExporterP.get())();
  const exportableObjects = getExportableObjects(objects);

  if (exportableObjects.length === 0) {
    alert('No exportable objects in the scene.');
    return;
  }

  const sceneToExport = new THREE.Scene();
  for (const obj of exportableObjects) {
    sceneToExport.add(obj);
  }

  exporter.parse(
    sceneToExport,
    result => {
      const output = result instanceof ArrayBuffer ? new Uint8Array(result) : JSON.stringify(result, null, 2);
      const blob = new Blob([output], { type: 'text/plain' });
      download(blob, `scene.${binary ? 'glb' : 'gltf'}`);
    },
    error => {
      console.error('error exporting gltf:', error);
      alert('Error exporting gltf; see for details.');
    },
    {
      binary,
      trs: false,
      onlyVisible: true,
      truncateDrawRange: true,
    }
  );
};

export const exportOBJ = async (objects: (THREE.Mesh | THREE.Line | THREE.Light)[]): Promise<void> => {
  const exporter = new (await OBJExporterP.get())();
  const exportableObjects = getExportableObjects(objects);

  if (exportableObjects.length === 0) {
    alert('No exportable objects in the scene.');
    return;
  }

  const sceneToExport = new THREE.Scene();
  for (const obj of exportableObjects) {
    sceneToExport.add(obj);
  }

  const result = exporter.parse(sceneToExport);
  const blob = new Blob([result], { type: 'text/plain' });
  download(blob, 'scene.obj');
};
