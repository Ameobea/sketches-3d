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
      const output = JSON.stringify(result, null, 2);
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
