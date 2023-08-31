import * as THREE from 'three';

import type { VizState } from 'src/viz';
import type { VizConfig } from 'src/viz/conf';
import { buildCustomShader, setDefaultDistanceAmpParams } from 'src/viz/shaders/customShader';
import { loadNamedTextures } from 'src/viz/textureLoading';
import type { SceneConfig } from '..';

export const processLoadedScene = async (
  viz: VizState,
  loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  viz.scene.add(new THREE.AmbientLight(0xffffff, 0.5));
  const pointLight = new THREE.PointLight(0xdedede, 0.5, 100, 0.5);
  // pointLight.castShadow = true;
  // pointLight.shadow.mapSize.width = 1024;
  // pointLight.shadow.mapSize.height = 1024;
  pointLight.position.set(-1, 0, -0.5);
  viz.scene.add(pointLight);

  setDefaultDistanceAmpParams({
    ampFactor: 6,
    falloffEndDistance: 30,
    falloffStartDistance: 0.1,
    exponent: 1.5,
  });

  // add a sphere to debug the position of the point light
  const sphere = new THREE.Mesh(
    new THREE.SphereGeometry(0.1, 16, 16),
    new THREE.MeshBasicMaterial({ color: 0xff0000 })
  );
  sphere.position.copy(pointLight.position);
  sphere.castShadow = false;
  sphere.receiveShadow = false;
  viz.scene.add(sphere);

  const loader = new THREE.ImageBitmapLoader();
  const { caveTexture, caveNormal, caveRoughness } = await loadNamedTextures(loader, {
    caveTexture: 'https://i.ameo.link/bfj.jpg',
    caveNormal: 'https://i.ameo.link/bfk.jpg',
    caveRoughness: 'https://i.ameo.link/bfl.jpg',
  });

  const cave = loadedWorld.getObjectByName('Cylinder') as THREE.Mesh;
  cave.castShadow = true;
  cave.receiveShadow = true;
  cave.material = buildCustomShader(
    {
      color: new THREE.Color(0xffffff),
      map: caveTexture,
      normalMap: caveNormal,
      normalScale: 0.8,
      roughnessMap: caveRoughness,
      metalness: 0.9,
      // uvTransform: new THREE.Matrix3().scale(10, 10),
      uvTransform: new THREE.Matrix3().scale(0.1, 0.1),
    },
    {},
    { useTriplanarMapping: true }
  );

  return {
    spawnLocation: 'spawn',
    gravity: 30,
    player: {
      movementAccelPerSecond: { onGround: 9, inAir: 9 },
      colliderCapsuleSize: { height: 2.2, radius: 0.8 },
      jumpVelocity: 12,
      oobYThreshold: -110,
    },
    debugPos: true,
    locations: {
      spawn: {
        pos: new THREE.Vector3(0, 0, 0),
        rot: new THREE.Vector3(-1.5707963267948966, -0.013999999999999973, 0),
      },
    },
  };
};
