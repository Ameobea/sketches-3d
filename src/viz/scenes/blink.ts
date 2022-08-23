import * as THREE from 'three';
import { Pane } from 'tweakpane';

import type { SceneConfig } from '.';
import type { VizState } from '..';
// import { buildCustomShader } from '../shaders/customShader';
// import redNoiseShader from '../shaders/redNoise.frag?raw';
import { initBaseScene } from '../util';

const buildControls = (
  defaultConfJson: string,
  onChange: (newConfJson: Record<string, number>) => void,
  onColorChange: (newColor: number, colorIx: number) => void,
  engine: typeof import('../wasmComp/engine'),
  ctxPtrs: number[]
) => {
  const defaultConf = JSON.parse(defaultConfJson);

  const pane = new Pane({});
  pane.addInput({ color_0: 0x8a0606, theme: 'dark' }, 'color_0', { view: 'color' });
  pane.addInput({ color_1: 0xff2424, theme: 'dark' }, 'color_1', { view: 'color' });
  pane.addInput({ drag_coefficient: defaultConf['drag_coefficient'], theme: 'dark' }, 'drag_coefficient', {
    min: 0.92,
    max: 1,
  });
  pane.addInput({ noise_frequency: defaultConf['noise_frequency'], theme: 'dark' }, 'noise_frequency', {
    min: 0.1,
    max: 4,
  });
  pane.addInput({ noise_amplitude: defaultConf['noise_amplitude'], theme: 'dark' }, 'noise_amplitude', {
    min: 0,
    max: 10000,
  });
  pane.addInput(
    { noise_time_warp_speed: defaultConf['noise_time_warp_speed'], theme: 'dark' },
    'noise_time_warp_speed',
    {
      min: 0,
      max: 1,
    }
  );
  pane.addInput(
    { conduit_acceleration_per_second: defaultConf['conduit_acceleration_per_second'], theme: 'dark' },
    'conduit_acceleration_per_second',
    {
      min: 0,
      max: 1000,
    }
  );
  pane.addInput(
    { tidal_force_amplitude: defaultConf['tidal_force_amplitude'], theme: 'dark' },
    'tidal_force_amplitude',
    {
      min: 0,
      max: 1000,
    }
  );
  pane.addInput(
    { tidal_force_frequency: defaultConf['tidal_force_frequency'], theme: 'dark' },
    'tidal_force_frequency',
    {
      min: 0,
      max: 16,
    }
  );
  pane.addInput(
    { particle_spawn_rate_per_second: defaultConf['particle_spawn_rate_per_second'], theme: 'dark' },
    'particle_spawn_rate_per_second',
    {
      min: 1,
      max: 8000,
    }
  );
  pane.addInput(
    { conduit_twist_frequency: defaultConf['conduit_twist_frequency'], theme: 'dark' },
    'conduit_twist_frequency',
    {
      min: 0,
      max: 0.1,
    }
  );
  pane.addInput(
    { conduit_twist_amplitude: defaultConf['conduit_twist_amplitude'], theme: 'dark' },
    'conduit_twist_amplitude',
    {
      min: 0,
      max: 50,
    }
  );
  pane.addInput({ conduit_radius: defaultConf['conduit_radius'], theme: 'dark' }, 'conduit_radius', {
    min: 0.01,
    max: 20,
  });
  pane.addInput(
    { conduit_attraction_magnitude: defaultConf['conduit_attraction_magnitude'], theme: 'dark' },
    'conduit_attraction_magnitude',
    {
      min: 0,
      max: 1,
    }
  );
  pane.addInput(
    {
      noise_amplitude_modulation_frequency: defaultConf['noise_amplitude_modulation_frequency'],
      theme: 'dark',
    },
    'noise_amplitude_modulation_frequency',
    {
      min: 0,
      max: 5,
    }
  );
  pane.addInput(
    {
      noise_amplitude_modulation_amplitude: defaultConf['noise_amplitude_modulation_amplitude'],
      theme: 'dark',
    },
    'noise_amplitude_modulation_amplitude',
    {
      min: 0,
      max: 8000,
    }
  );

  pane.element.parentElement!.style.width = '430px';
  window.addEventListener('keydown', evt => {
    if (evt.key === 'h') {
      pane.element.parentElement!.style.display =
        pane.element.parentElement!.style.display === 'none' ? '' : 'none';
    }
  });

  const binds = { 'particle count': 1 };
  setInterval(() => {
    let renderedParticleCount = 0;
    for (const ctxPtr of ctxPtrs) {
      renderedParticleCount += engine.get_current_conduit_rendered_particle_count(ctxPtr);
    }
    binds['particle count'] = renderedParticleCount;
    pane.refresh();
  }, 500);
  pane.addMonitor(binds, 'particle count', {});

  const curConf = { ...defaultConf };
  const saveButton = pane.addButton({
    title: 'copy conf to clipboard',
  });
  saveButton.on('click', () => {
    const confJson = JSON.stringify(curConf);
    navigator.clipboard.writeText(confJson);
  });
  const resetButton = pane.addButton({
    title: 'reset conf',
  });
  resetButton.on('click', () => {
    pane.importPreset(defaultConf);
    Object.assign(curConf, defaultConf);
    onChange(curConf);
  });

  pane.on('change', evt => {
    if (!evt.presetKey) {
      console.error(evt);
      return;
    }

    if (evt.presetKey.includes('color')) {
      const colorIx = +evt.presetKey.split('_')[1];
      onColorChange(evt.value as number, colorIx);
      return;
    }

    curConf[evt.presetKey] = evt.value;
    onChange(curConf);
  });

  onChange(curConf);
};

const locations = {
  spawn: {
    pos: new THREE.Vector3(48.17740050559579, 23.920086905508146, 8.603910511800485),
    rot: new THREE.Vector3(-0.022, 1.488, 0),
  },
};

export const processLoadedScene = async (viz: VizState, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  // const ground = loadedWorld.getObjectByName('ground')! as THREE.Mesh;
  // const groundMat = new THREE.ShaderMaterial(
  //   buildCustomShader(
  //     {
  //       roughness: 0.9,
  //       metalness: 0.6,
  //       color: new THREE.Color(0x020202),
  //     },
  //     { colorShader: redNoiseShader }
  //   )
  // );
  // ground.material = groundMat;
  loadedWorld.children.forEach(obj => {
    obj.removeFromParent();
  });

  const engine = await import('../wasmComp/engine');
  await engine.default();

  const { ambientlight, light } = initBaseScene(viz);
  ambientlight.intensity = 1.8;
  viz.scene.fog = null;

  // Add in a white cube at the position of the light
  const lightCube = new THREE.Mesh(
    new THREE.BoxGeometry(10, 10, 10),
    new THREE.MeshBasicMaterial({ color: 0xffffff })
  );
  lightCube.position.copy(light.position);
  viz.scene.add(lightCube);

  const conduitStartPos = new THREE.Vector3(-50, 40, -50);
  const conduitEndPos = new THREE.Vector3(100, 40, -100);

  // const conduitMesh = new THREE.Mesh(
  //   new THREE.TubeBufferGeometry(
  //     new THREE.LineCurve3(conduitStartPos, conduitEndPos),
  //     100,
  //     conduitRaduis,
  //     10,
  //     false
  //   ),
  //   new THREE.MeshStandardMaterial({ color: 0x181818, metalness: 0.5, roughness: 0.5 })
  // );
  // viz.scene.add(conduitMesh);

  // add cube at conduit start + end position
  // const conduitStartCube = new THREE.Mesh(
  //   new THREE.BoxGeometry(8, 8, 8),
  //   new THREE.MeshStandardMaterial({ color: 0x00ff00 })
  // );
  // conduitStartCube.position.copy(conduitStartPos);
  // viz.scene.add(conduitStartCube);
  // const conduitEndCube = new THREE.Mesh(
  //   new THREE.BoxBufferGeometry(8, 8, 8),
  //   new THREE.MeshStandardMaterial({ color: 0xff0000 })
  // );
  // conduitEndCube.position.copy(conduitEndPos);
  // viz.scene.add(conduitEndCube);

  const MAX_PARTICLE_COUNT = 80_000;
  const conduitParticles = new THREE.InstancedMesh(
    new THREE.BoxBufferGeometry(1.3, 1.3, 1.3),
    // new THREE.MeshStandardMaterial({ color: 0x8a0606, metalness: 0.8, roughness: 1 }),
    new THREE.MeshBasicMaterial({}),
    MAX_PARTICLE_COUNT
  );
  const instanceColorBuffer = new Uint8ClampedArray(MAX_PARTICLE_COUNT * 3);
  const instanceColor = new THREE.InstancedBufferAttribute(instanceColorBuffer, 3, true);
  conduitParticles.instanceColor = instanceColor;
  instanceColor.count = 0;
  conduitParticles.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
  conduitParticles.instanceColor.setUsage(THREE.DynamicDrawUsage);
  viz.scene.add(conduitParticles);

  const conduitParticles2 = new THREE.InstancedMesh(
    new THREE.BoxBufferGeometry(1.3, 1.3, 1.3),
    // new THREE.MeshStandardMaterial({ color: 0xff2424, metalness: 0.8, roughness: 1 }),
    new THREE.MeshBasicMaterial({}),
    MAX_PARTICLE_COUNT
  );
  const instanceColorBuffer2 = new Uint8ClampedArray(MAX_PARTICLE_COUNT * 3);
  const instanceColor2 = new THREE.InstancedBufferAttribute(instanceColorBuffer2, 3, true);
  instanceColor2.count = 0;
  conduitParticles2.instanceColor = instanceColor2;
  conduitParticles2.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
  conduitParticles2.instanceColor.setUsage(THREE.DynamicDrawUsage);
  viz.scene.add(conduitParticles2);

  const conduitStatePtr = engine.create_conduit_particles_state(
    conduitStartPos.x,
    conduitStartPos.y,
    conduitStartPos.z,
    conduitEndPos.x,
    conduitEndPos.y,
    conduitEndPos.z,
    0
  );
  const conduitStatePtr2 = engine.create_conduit_particles_state(
    conduitStartPos.x,
    conduitStartPos.y,
    conduitStartPos.z,
    conduitEndPos.x,
    conduitEndPos.y,
    conduitEndPos.z,
    1
  );
  viz.registerBeforeRenderCb((curTimeSecs, tDiffSecs) => {
    // groundMat.uniforms.curTimeSeconds.value = curTimeSecs;

    const newPositions = engine.tick_conduit_particles(conduitStatePtr, curTimeSecs, tDiffSecs);
    conduitParticles.count = newPositions.length / 3;
    const newColors = engine.get_conduit_particle_colors(conduitStatePtr);
    (conduitParticles.instanceColor!.array as Uint8Array).set(newColors);
    conduitParticles.instanceColor!.needsUpdate = true;

    const newPositions2 = engine.tick_conduit_particles(conduitStatePtr2, curTimeSecs, tDiffSecs);
    conduitParticles2.count = newPositions2.length / 3;
    const newColors2 = engine.get_conduit_particle_colors(conduitStatePtr2);
    (conduitParticles2.instanceColor!.array as Uint8Array).set(newColors2);
    conduitParticles2.instanceColor!.needsUpdate = true;

    const jitter = Math.pow((Math.sin(curTimeSecs) + 1) / 2, 1.5);

    const mat = new THREE.Matrix4();
    mat.makeScale(
      1 + Math.sin(curTimeSecs * 4) * 0.4,
      1 + Math.sin(curTimeSecs * 4) * 0.4,
      1 + Math.sin(curTimeSecs * 4) * 0.4
    );
    for (let i = 0; i < newPositions.length; i += 3) {
      mat.setPosition(
        newPositions[i] + Math.random() * jitter,
        newPositions[i + 1] + Math.random() * jitter,
        newPositions[i + 2] + Math.random() * jitter
      );
      conduitParticles.setMatrixAt(i / 3, mat);
    }

    mat.makeScale(
      0.6 + Math.sin(curTimeSecs * 2) * 0.2,
      0.6 + Math.sin(curTimeSecs * 2) * 0.2,
      0.6 + Math.sin(curTimeSecs * 2) * 0.2
    );
    for (let i = 0; i < newPositions2.length; i += 3) {
      mat.setPosition(newPositions2[i], newPositions2[i + 1], newPositions2[i + 2]);
      conduitParticles2.setMatrixAt(i / 3, mat);
    }

    conduitParticles.instanceMatrix.needsUpdate = true;
    conduitParticles2.instanceMatrix.needsUpdate = true;

    conduitParticles.instanceMatrix.updateRange.offset = 0;
    conduitParticles.instanceMatrix.updateRange.count =
      (newPositions.length / 3) * conduitParticles.instanceMatrix.itemSize;
    conduitParticles2.instanceMatrix.updateRange.offset = 0;
    conduitParticles2.instanceMatrix.updateRange.count =
      (newPositions2.length / 3) * conduitParticles2.instanceMatrix.itemSize;
  });

  const defaultConfJson = engine.get_default_conduit_conf_json();
  buildControls(
    defaultConfJson,
    (newConf: Record<string, number>) => {
      engine.set_conduit_conf(conduitStatePtr, JSON.stringify(newConf));
      engine.set_conduit_conf(
        conduitStatePtr2,
        JSON.stringify({ ...newConf, conduit_radius: newConf.conduit_radius * 3 })
      );
    },
    (newColor, colorIx) => {
      if (colorIx === 0) {
        conduitParticles.material.color.set(newColor);
      } else {
        conduitParticles2.material.color.set(newColor);
      }
    },
    engine,
    [conduitStatePtr, conduitStatePtr2]
  );

  return {
    locations,
    spawnLocation: 'spawn',
    viewMode: {
      type: 'orbit',
      pos: new THREE.Vector3(190.16798130391035, 70.33263180077928, 59.63493635180146),
      target: new THREE.Vector3(167.41573227055312, 37.46032347797772, -81.3361176559136),
    },
  };
};
