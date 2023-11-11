import * as THREE from 'three';

import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import { buildCustomShader, setDefaultDistanceAmpParams } from 'src/viz/shaders/customShader';
import { loadNamedTextures } from 'src/viz/textureLoading';
import { delay } from 'src/viz/util';
import type { SceneConfig } from '..';
import type { VizState } from '../..';
import { initWebSynth } from '../../../viz/webSynth';
import { buildAndAddFractals } from './3DvicsekFractal';
import { Locations } from './locations';
import { configurePostprocessing } from './postprocessing';
import BgMonolithColorShader from './shaders/bgMonolith/color.frag?raw';

export const processLoadedScene = async (
  viz: VizState,
  loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  viz.camera.position.copy(Locations.spawn.pos.add(new THREE.Vector3(0, 1.5, 0)));

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

  setDefaultDistanceAmpParams({
    ampFactor: 1.6,
    falloffStartDistance: 0,
    falloffEndDistance: 30,
    exponent: 1.34,
  });

  initWebSynth({ compositionIDToLoad: 107 }).then(async ctx => {
    await delay(1200);

    ctx.setGlobalBpm(66);
    ctx.startAll();
  });

  const building = loadedWorld.getObjectByName('Cube') as THREE.Mesh;
  building.material = buildCustomShader(
    {
      color: new THREE.Color(0x999999),
      map: buildingTexture,
      roughness: 0.4,
      metalness: 0.4,
      uvTransform: new THREE.Matrix3().scale(0.1682, 0.1682),
      ambientDistanceAmp: {
        ampFactor: 0.5,
        falloffStartDistance: 0,
        falloffEndDistance: 30,
        exponent: 1.34,
      },
    },
    {},
    {
      useGeneratedUVs: true,
      randomizeUVOffset: true,
    }
  );

  viz.renderer.shadowMap.enabled = true;
  viz.renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  viz.renderer.shadowMap.needsUpdate = true;

  let lightPos = new THREE.Vector3(-32, 27, -32);
  let lightTarget = new THREE.Vector3(22, 2, 10);

  // Move light pos away in a line from target by 0.2x its initial distance
  const lightPosToTarget = lightPos.clone().sub(lightTarget);
  lightPosToTarget.multiplyScalar(0.2);
  lightPos.add(lightPosToTarget);

  const shadowMapSize = {
    [GraphicsQuality.Low]: 1024,
    [GraphicsQuality.Medium]: 2048,
    [GraphicsQuality.High]: 4096,
  }[vizConf.graphics.quality];

  const dirLight = new THREE.DirectionalLight(LIGHT_COLOR, 1);
  dirLight.name = 'godraysLight';
  dirLight.target.position.copy(lightTarget);
  dirLight.castShadow = true;
  dirLight.shadow.bias = 0.01;
  dirLight.shadow.mapSize.width = shadowMapSize;
  dirLight.shadow.mapSize.height = shadowMapSize;
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

  viz.renderer.domElement.style.visibility = 'hidden';
  const populateShadowMap = () => {
    // Render the scene once to populate the shadow map
    dirLight.shadow.needsUpdate = true;
    viz.renderer.shadowMap.needsUpdate = true;
    viz.renderer.render(viz.scene, viz.camera);
    dirLight.shadow.needsUpdate = false;
    dirLight.shadow.autoUpdate = false;
    viz.renderer.shadowMap.needsUpdate = false;
    viz.renderer.shadowMap.autoUpdate = false;
    viz.renderer.shadowMap.enabled = true;
    viz.renderer.domElement.style.visibility = 'visible';
  };

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
      tileBreaking:
        vizConf.graphics.quality >= GraphicsQuality.Low ? { type: 'neyret', patchScale: 3.5 } : undefined,
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
      ambientLightScale: 1.5,
      ambientDistanceAmp: {
        ampFactor: 0.4,
        falloffStartDistance: 0,
        falloffEndDistance: 10,
        exponent: 1.34,
      },
    },
    {},
    {
      tileBreaking:
        vizConf.graphics.quality >= GraphicsQuality.Low ? { type: 'neyret', patchScale: 3.5 } : undefined,
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
      ambientLightScale: 4,
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

  const pipeLightPosts = loadedWorld.children.filter(
    c => c.name.startsWith('pipe_light_post') || c.name.startsWith('torch_light_post')
  ) as THREE.Mesh[];
  const pipeLights = loadedWorld.children.filter(
    c => !c.name.includes('post') && (c.name.startsWith('pipe_light') || c.name.startsWith('torch_light'))
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
        // TODO: Init next level properly
        // navigate to /cave
        window.location.assign(window.location.origin.includes('localhost') ? '/cave' : '/cave.html');
      }
    )
  );

  configurePostprocessing(viz, dirLight, vizConf.graphics.quality, populateShadowMap);

  populateShadowMap();

  return {
    spawnLocation: 'spawn',
    gravity: 30,
    player: {
      movementAccelPerSecond: { onGround: 9, inAir: 9 },
      colliderCapsuleSize: { height: 2.2, radius: 0.8 },
      jumpVelocity: 12,
      oobYThreshold: -110,
    },
    locations: Locations,
    debugPos: true,
    debugPlayerKinematics: true,
  };
};
