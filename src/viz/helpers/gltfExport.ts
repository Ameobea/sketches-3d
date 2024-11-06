import type * as THREE from 'three';
import { GLTFExporter } from 'three/examples/jsm/exporters/GLTFExporter';

export const exportScene = (scene: THREE.Scene) => {
  const exporter = new GLTFExporter();
  exporter.parse(scene, (result: unknown) => {
    console.log({ result });
    const link = document.createElement('a');
    link.href = URL.createObjectURL(new Blob([JSON.stringify(result)], { type: 'application/json' }));
    link.download = 'scene.gltf';
    link.click();
  });
};
