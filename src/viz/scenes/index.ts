import type { SvelteSeoProps } from 'svelte-seo/types/SvelteSeo';
import type { Writable } from 'svelte/store';

import type { VizState } from '..';
import type { SfxConfig } from '../audio/SfxManager';
import type { VizConfig } from '../conf';
import type { DeepPartial } from '../util';

export interface SceneConfigLocation {
  pos: THREE.Vector3;
  rot: THREE.Vector3;
}

export type SceneLocations = {
  [key: string]: {
    pos: THREE.Vector3 | [number, number, number];
    rot: THREE.Vector3 | [number, number, number];
  };
};

export interface PlayerMoveSpeed {
  onGround: number;
  inAir: number;
}

export const DefaultMoveSpeed: PlayerMoveSpeed = Object.freeze({
  onGround: 12,
  inAir: 12,
});

export interface DashChargeConfig {
  curCharges: Writable<number>;
}

export interface DashConfig {
  /**
   * Default: true
   */
  enable: boolean;
  /**
   * If not provided, dashes will be unmetered
   */
  chargeConfig?: DashChargeConfig;
  dashMagnitude: number;
  minDashDelaySeconds: number;
}

export const DefaultDashConfig: DashConfig = Object.freeze({
  enable: true,
  dashMagnitude: 16,
  minDashDelaySeconds: 0.85,
});

export interface SceneConfig {
  viewMode?: { type: 'firstPerson' } | { type: 'orbit'; pos: THREE.Vector3; target: THREE.Vector3 };
  locations: SceneLocations;
  spawnLocation: string;
  /**
   * If true, the current position in the world of the player will be displayed
   */
  debugPos?: boolean;
  /**
   * If true, the name of the object at the center of the screen will be displayed
   */
  debugTarget?: boolean;
  /**
   * If true, the player's movement and collision world state will be displayed
   */
  debugPlayerKinematics?: boolean;
  gravity?: number;
  player?: {
    dashConfig?: Partial<DashConfig>;
    jumpVelocity?: number;
    colliderCapsuleSize?: { height: number; radius: number };
    moveSpeed?: { onGround: number; inAir: number };
    oobYThreshold?: number;
  };
  renderOverride?: (timeDiffSeconds: number) => void;
  enableInventory?: boolean;
  sfx?: DeepPartial<SfxConfig>;
  legacyLights?: boolean;
}

export const buildDefaultSceneConfig = () => ({
  viewMode: { type: 'firstPerson' as const },
});

export interface SceneDef {
  sceneName: string | null;
  sceneLoader: () => Promise<
    (viz: VizState, loadedWorld: THREE.Group, config: VizConfig) => SceneConfig | Promise<SceneConfig>
  >;
  metadata: SvelteSeoProps;
  gltfName?: string | null;
  extension?: 'gltf' | 'glb';
  needsDraco?: boolean;
  legacyLights?: boolean;
}

const ParticleConduit: SceneDef = {
  sceneName: 'blink',
  sceneLoader: () => import('./blink').then(mod => mod.processLoadedScene),
  legacyLights: true,
  metadata: {
    title: 'Particle Conduit',
    description:
      '3D physics-based particle system where particles move along a conduit.  Various forces interact along with noise-based fields to send the particles on chaotic paths.  Built with Three.JS + Rust + WebAssembly',
    openGraph: {
      title: 'Particle Conduit',
      description:
        'Physics-based particle system where particles move along a conduit.  Various forces interact along with noise-based fields to send the particles on chaotic paths.',
      images: [
        {
          url: 'https://i.ameo.link/a9c.png',
          alt: 'A screenshot of the particle conduit visualization.  Shows red and orange particles moving in stream-like patterns across the screen.  Includes a control panel on the right with many controls for configuring the simulation.',
          width: 1508,
          height: 1194,
        },
      ],
    },
  },
};

export const ScenesByName: { [key: string]: SceneDef } = {
  bridge: {
    sceneName: 'bridge',
    sceneLoader: () => import('./bridge').then(mod => mod.processLoadedScene),
    metadata: { title: 'bridge' },
    legacyLights: true,
  },
  blink: ParticleConduit,
  particle_conduit: ParticleConduit,
  walkways: {
    sceneName: 'walkways',
    sceneLoader: () => import('./walkways').then(mod => mod.processLoadedScene),
    metadata: { title: 'walkways' },
    legacyLights: true,
  },
  subdivided: {
    sceneName: 'subdivided',
    sceneLoader: () => import('./subdivided').then(mod => mod.processLoadedScene),
    metadata: { title: 'subdivided' },
    legacyLights: true,
  },
  fractal_cube: {
    sceneName: 'fractal_cube',
    sceneLoader: () => import('./fractal_cube').then(mod => mod.processLoadedScene),
    metadata: { title: 'fractal cube' },
    legacyLights: true,
  },
  bridge2: {
    sceneName: 'bridge2',
    sceneLoader: () => import('./bridge2').then(mod => mod.processLoadedScene),
    metadata: { title: 'bridge2' },
    gltfName: 'checkpoint5',
    legacyLights: true,
  },
  collisiondemo: {
    sceneName: 'collisionDemo',
    sceneLoader: () => import('./collisionDemo').then(mod => mod.processLoadedScene),
    metadata: { title: 'collisionDemo' },
    legacyLights: true,
  },
  chasms: {
    sceneName: 'chasms',
    sceneLoader: () => import('./chasms/chasms').then(mod => mod.processLoadedScene),
    metadata: { title: 'chasms' },
    gltfName: 'chasms',
    legacyLights: true,
  },
  godrays_test: {
    sceneName: 'godrays_test',
    sceneLoader: () => import('./experiments/godrays-test/godraysTest').then(mod => mod.processLoadedScene),
    metadata: { title: 'godrays_test' },
    gltfName: null,
    legacyLights: true,
  },
  rainy: {
    sceneName: 'Scene',
    sceneLoader: () => import('./rainy/rainy').then(mod => mod.processLoadedScene),
    metadata: { title: 'rainy' },
    gltfName: 'rainy',
    extension: 'glb',
    needsDraco: true,
    legacyLights: true,
  },
  depthPrepassDemo: {
    sceneName: null,
    sceneLoader: () => import('./depthPrepassDemo').then(mod => mod.processLoadedScene),
    metadata: { title: 'depthPrepassDemo' },
    gltfName: null,
    legacyLights: true,
  },
  smoke: {
    sceneName: 'Scene',
    sceneLoader: () => import('./smoke/smoke').then(mod => mod.processLoadedScene),
    metadata: {
      title: 'smoke',
      openGraph: {
        images: [
          {
            url: 'https://i.ameo.link/bf1.png',
            width: 1829,
            height: 1304,
            alt: 'A screenshot of the "smoke" level.  Shows intense orange fog, floating fractal structures composed out of large dark cubes with stone-like texturing and patterns, and four orange/yellow lights glowing in the distance supported by long poles.',
          },
        ],
      },
    },
    gltfName: 'smoke',
    extension: 'glb',
    legacyLights: true,
  },
  cave: {
    sceneName: 'proc',
    sceneLoader: () => import('./cave/cave').then(mod => mod.processLoadedScene),
    metadata: { title: 'cave' },
    gltfName: 'cave',
    extension: 'glb',
    legacyLights: true,
  },
  gn_inst_test: {
    sceneName: 'Scene',
    sceneLoader: () => import('./experiments/gn_inst_test/gnInstTest').then(mod => mod.processLoadedScene),
    metadata: { title: 'gn_inst_test' },
    gltfName: 'gn_inst_test',
    extension: 'glb',
    legacyLights: true,
  },
  fogTest: {
    sceneName: 'Scene',
    sceneLoader: () => import('./experiments/fog/fog').then(mod => mod.processLoadedScene),
    metadata: { title: 'volumetric fog test' },
    gltfName: 'fogTest',
    extension: 'glb',
    legacyLights: true,
  },
  terrainTest: {
    sceneName: 'Scene',
    sceneLoader: () => import('./experiments/terrain/terrain').then(mod => mod.processLoadedScene),
    metadata: { title: 'LOD terrain test' },
    gltfName: 'fogTest',
    extension: 'glb',
    legacyLights: true,
  },
  stone: {
    sceneName: 'Scene',
    sceneLoader: () => import('./stone/stone').then(mod => mod.processLoadedScene),
    metadata: { title: 'stone' },
    gltfName: 'stone',
    extension: 'glb',
    legacyLights: true,
  },
  terrainSandbox: {
    gltfName: null,
    sceneLoader: () => import('./terrainSandbox/terrainSandbox').then(mod => mod.processLoadedScene),
    sceneName: null,
    metadata: { title: 'terrain sandbox' },
    legacyLights: true,
  },
  runeGenTest: {
    gltfName: 'torus',
    extension: 'glb',
    sceneLoader: () => import('./experiments/runeGen/runeGen').then(mod => mod.processLoadedScene),
    sceneName: 'Scene',
    metadata: { title: 'Geodesic Mesh Mapping Demo' },
    legacyLights: true,
  },
  construction: {
    gltfName: 'construction',
    extension: 'glb',
    sceneLoader: () => import('./construction/construction').then(mod => mod.processLoadedScene),
    sceneName: 'Scene',
    metadata: { title: 'Under Construction' },
    legacyLights: true,
  },
  pk_pylons: {
    gltfName: 'pk_pylons',
    extension: 'glb',
    sceneLoader: () => import('./pkPylons/pkPylons').then(mod => mod.processLoadedScene),
    sceneName: 'Scene',
    metadata: { title: 'pk_pylons' },
  },
};
