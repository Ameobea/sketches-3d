import * as THREE from 'three';

import { buildCustomShader } from 'src/viz/shaders/customShader';
import { loadNamedTextures, loadTexture } from 'src/viz/textureLoading';
import type { SceneConfig } from '..';
import type { VizState } from '../..';
import { buildAndAddFractals } from './3DvicsekFractal';
import { configurePostprocessing } from './postprocessing';
import BgMonolithColorShader from './shaders/bgMonolith/color.frag?raw';

export const processLoadedScene = async (viz: VizState, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  viz.scene.add(loadedWorld);

  const LIGHT_COLOR = 0xa14e0b;
  const ambientLight = new THREE.AmbientLight(LIGHT_COLOR, 0.41);
  viz.scene.add(ambientLight);

  const loader = new THREE.ImageBitmapLoader();
  const {
    buildingTexture,
    goldTextureAlbedo,
    goldTextureNormal,
    goldTextureRoughness,
    pipeTexture,
    pipeTextureNormal,
    pipeTextureRoughness,
    cubesTexture,
    cubesTextureNormal,
    cubesTextureRoughness,
  } = await loadNamedTextures(loader, {
    buildingTexture: 'https://i.ameo.link/bdu.jpg',
    goldTextureAlbedo: 'https://i.ameo.link/be0.jpg',
    goldTextureNormal: 'https://i.ameo.link/be2.jpg',
    goldTextureRoughness: 'https://i.ameo.link/bdz.jpg',
    pipeTexture: 'https://i.ameo.link/bet.jpg',
    pipeTextureNormal: 'https://i.ameo.link/beu.jpg',
    pipeTextureRoughness: 'https://i.ameo.link/bev.jpg',
    cubesTextureRoughness: 'https://i.ameo.link/bew.jpg',
    cubesTextureNormal: 'https://i.ameo.link/bex.jpg',
    cubesTexture: 'https://i.ameo.link/bey.jpg',
  });

  const building = loadedWorld.getObjectByName('Cube') as THREE.Mesh;
  building.material = buildCustomShader(
    {
      color: new THREE.Color(0x999999),
      map: buildingTexture,
      roughness: 0.4,
      metalness: 0.4,
      uvTransform: new THREE.Matrix3().scale(0.0482, 0.0482),
    },
    {},
    {
      useGeneratedUVs: true,
      randomizeUVOffset: true,
      // tileBreaking: { type: 'neyret', patchScale: 1 },
    }
  );

  viz.renderer.shadowMap.enabled = true;
  viz.renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  viz.renderer.shadowMap.needsUpdate = true;

  let lightPos = new THREE.Vector3(-32, 27, -32);
  let lightTarget = new THREE.Vector3(22, 4, 10);

  // Move light pos away in a line from target by 0.2x its initial distance
  const lightPosToTarget = lightPos.clone().sub(lightTarget);
  lightPosToTarget.multiplyScalar(0.2);
  lightPos.add(lightPosToTarget);

  const dirLight = new THREE.DirectionalLight(LIGHT_COLOR, 1);
  dirLight.target.position.copy(lightTarget);
  dirLight.castShadow = true;
  dirLight.shadow.bias = 0.01;
  dirLight.shadow.mapSize.width = 1024 * 4;
  dirLight.shadow.mapSize.height = 1024 * 4;
  dirLight.shadow.autoUpdate = true;
  dirLight.shadow.camera.near = 0.1;
  dirLight.shadow.camera.far = 370;
  dirLight.shadow.camera.left = -180;
  dirLight.shadow.camera.right = 230;
  dirLight.shadow.camera.top = 100;
  dirLight.shadow.camera.bottom = -150.0;
  dirLight.shadow.camera.updateProjectionMatrix();
  dirLight.matrixWorldNeedsUpdate = true;
  dirLight.updateMatrixWorld();
  dirLight.target.updateMatrixWorld();
  dirLight.position.copy(lightPos);
  viz.scene.add(dirLight);

  // helper for dirlight camera
  // const helper = new THREE.CameraHelper(dirLight.shadow.camera);
  // viz.scene.add(helper);

  viz.scene.fog = new THREE.Fog(LIGHT_COLOR, 0.02, 200);
  viz.scene.background = new THREE.Color(0x8f4509);

  // Render the scene once to populate the shadow map
  viz.renderer.render(viz.scene, viz.camera);

  const bgMonoliths = loadedWorld.children.filter(c => c.name.startsWith('bg_monolith')) as THREE.Mesh[];
  const bgMonolithMaterial = buildCustomShader(
    { color: new THREE.Color(0x444444), transparent: true, fogMultiplier: 0.2, side: THREE.DoubleSide },
    { colorShader: BgMonolithColorShader },
    { enableFog: true }
  );
  bgMonoliths.forEach(m => {
    m.material = bgMonolithMaterial;
    if (m.name.includes('007')) {
      m.scale.z *= 1.12;
    }
  });

  const border = loadedWorld.getObjectByName('border') as THREE.Mesh;
  const goldMaterial = buildCustomShader(
    {
      color: new THREE.Color(0xffffff),
      map: goldTextureAlbedo,
      normalMap: goldTextureNormal,
      roughnessMap: goldTextureRoughness,
      uvTransform: new THREE.Matrix3().scale(0.8982, 0.8982),
      roughness: 0.7,
      metalness: 0.7,
    },
    { colorShader: BgMonolithColorShader },
    {
      useGeneratedUVs: true,
      randomizeUVOffset: false,
      tileBreaking: { type: 'neyret', patchScale: 3.5 },
    }
  );
  border.material = goldMaterial;

  const brokenPillar = loadedWorld.getObjectByName('broken_monolith') as THREE.Mesh;
  brokenPillar.material = buildCustomShader(
    {
      color: new THREE.Color(0xffffff),
      map: goldTextureAlbedo,
      normalMap: goldTextureNormal,
      roughnessMap: goldTextureRoughness,
      uvTransform: new THREE.Matrix3().scale(40.8982, 40.8982),
      roughness: 0.7,
      metalness: 0.7,
    },
    {},
    {
      tileBreaking: { type: 'neyret', patchScale: 3.5 },
    }
  );

  const rectArea = new THREE.RectAreaLight(LIGHT_COLOR, 28.8, 40, 1);
  rectArea.position.set(-25, 8, 28);
  rectArea.rotation.set(0, 0, 0);
  rectArea.lookAt(rectArea.position.x, -rectArea.position.y, rectArea.position.z);
  viz.scene.add(rectArea);

  const ceilingRailings = loadedWorld.getObjectByName('ceiling_railings') as THREE.Mesh;
  ceilingRailings.material = buildCustomShader(
    {
      color: new THREE.Color(LIGHT_COLOR),
      metalness: 1,
    },
    {},
    {}
  );

  const pipeMaterial = buildCustomShader(
    {
      map: pipeTexture,
      normalMap: pipeTextureNormal,
      roughnessMap: pipeTextureRoughness,
      metalness: 0.9,
      uvTransform: new THREE.Matrix3().scale(9.8982, 9.8982),
      color: new THREE.Color(0xffffff),
      ambientLightScale: 2,
    },
    {},
    {}
  );
  const pipe = loadedWorld.getObjectByName('pipe') as THREE.Mesh;
  pipe.material = pipeMaterial;
  const pipeInteriorMaterial = buildCustomShader(
    {
      map: pipeTexture,
      normalMap: pipeTextureNormal,
      roughnessMap: pipeTextureRoughness,
      metalness: 0.9,
      uvTransform: new THREE.Matrix3().scale(9.8982, 9.8982),
    },
    {},
    { enableFog: false }
  );
  const pipeInterior = loadedWorld.getObjectByName('pipe_interior') as THREE.Mesh;
  pipeInterior.material = pipeInteriorMaterial;
  const pipeBottomMaterial = buildCustomShader(
    {
      map: pipeTexture,
      normalMap: pipeTextureNormal,
      roughnessMap: pipeTextureRoughness,
      metalness: 0.9,
      uvTransform: new THREE.Matrix3().scale(9.8982, 9.8982),
      color: new THREE.Color(0xffffff),
      ambientLightScale: 2,
    },
    {
      colorShader: BgMonolithColorShader,
    },
    {}
  );
  const pipeBottom = loadedWorld.getObjectByName('pipe_bottom') as THREE.Mesh;
  pipeBottom.material = pipeBottomMaterial;

  const pipeLightPosts = loadedWorld.children.filter(c =>
    c.name.startsWith('pipe_light_post')
  ) as THREE.Mesh[];
  const pipeLights = loadedWorld.children.filter(
    c => c.name.startsWith('pipe_light') && !c.name.includes('post')
  ) as THREE.Mesh[];

  const pipeLightPostMaterial = buildCustomShader({ color: new THREE.Color(0x121212) }, {}, {});
  for (const pipeLightPost of pipeLightPosts) {
    pipeLightPost.material = pipeLightPostMaterial;
  }

  const pipeLightMaterial = buildCustomShader(
    { color: new THREE.Color(0xff4444), metalness: 0.4, roughness: 0.2 },
    {},
    {}
  );
  for (const pipeLight of pipeLights) {
    pipeLight.material = pipeLightMaterial;
  }

  buildAndAddFractals(viz, cubesTexture, cubesTextureNormal, cubesTextureRoughness);

  // Add callback for when player falls down the pipe
  viz.collisionWorldLoadedCbs.push(fpCtx =>
    fpCtx.addPlayerRegionContactCb(
      {
        type: 'box',
        pos: new THREE.Vector3().copy(pipeInterior.position),
        halfExtents: new THREE.Vector3(5, 5, 5),
      },
      () => {
        console.log('entered!!');
        // TODO: Init next level
      }
    )
  );

  configurePostprocessing(viz, dirLight);

  return {
    spawnLocation: 'spawn',
    gravity: 30,
    player: {
      movementAccelPerSecond: { onGround: 9, inAir: 9 },
      colliderCapsuleSize: { height: 2.2, radius: 0.8 },
      jumpVelocity: 12,
      oobYThreshold: -110,
    },
    locations: {
      spawn: {
        pos: new THREE.Vector3(0, 0, 0),
        rot: new THREE.Vector3(-0.01, 1.412, 0),
      },
      end: {
        pos: new THREE.Vector3(-2.0920538902282715, -2.177037000656128, 127.51612854003906),
        rot: new THREE.Vector3(-0.5647963267948956, 2.4699999999998963, 0),
      },
      outside: {
        pos: new THREE.Vector3(24.726898193359375, 2.064194917678833, 27.218582153320312),
        rot: new THREE.Vector3(-0.0019999999999998647, 4.71799999999993, 0),
      },
      out: {
        pos: new THREE.Vector3(24.726898193359375, 2.064194917678833, 27.218582153320312),
        rot: new THREE.Vector3(-0.0019999999999998647, 4.71799999999993, 0),
      },
      pipe: {
        pos: new THREE.Vector3(-20.713298797607422, -12.508797645568848, 127.8397216796875),
        rot: new THREE.Vector3(-0.6007963267948956, 2.6579999999998973, 0),
      },
      jump: {
        pos: new THREE.Vector3(87.85043334960938, -21.2853946685791, 157.130859375),
        rot: new THREE.Vector3(-0.2939999999999999, 2.953999999999946, 0),
      },
      room: {
        pos: new THREE.Vector3(-31.08810806274414, 2.132516860961914, 29.65850830078125),
        rot: new THREE.Vector3(0.04000000000000012, 10.95200000000005, 0),
      },
    },
    debugPos: true,
    // debugTarget: true,
  };
};
