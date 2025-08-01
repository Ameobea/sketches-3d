import * as THREE from 'three';
import type SvelteSEO from 'svelte-seo';

import type { Viz } from '..';
import type { SfxConfig } from '../audio/SfxManager';
import type { VizConfig } from '../conf';
import type { DeepPartial } from '../util/util.ts';
import type { ComponentProps } from 'svelte';
import type { TransparentWritable } from '../util/TransparentWritable.ts';

type SvelteSEOProps = ComponentProps<SvelteSEO>;

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
  curCharges: TransparentWritable<number>;
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
  useExternalVelocity?: boolean;
  sfx?: {
    play?: boolean;
    name?: string;
    gainDb?: number;
  };
}

export interface CustomControlsEntry {
  label: string;
  key: string;
  action: () => void;
}

export type ViewMode =
  | { type: 'firstPerson' }
  | { type: 'orbit'; pos: THREE.Vector3; target: THREE.Vector3 }
  | {
      type: 'top-down';
      cameraOffset?: THREE.Vector3;
      cameraRotation?: THREE.Euler;
      cameraFOV?: number;
      cameraFocusPoint?: { type: 'player' } | { type: 'fixed'; pos: THREE.Vector3 };
    };

export const DefaultDashConfig: DashConfig = Object.freeze({
  enable: true,
  dashMagnitude: 16,
  minDashDelaySeconds: 0.85,
});

export const DefaultOOBThreshold = -55;

export const DefaultExternalVelocityAirDampingFactor = new THREE.Vector3(0.12, 0.55, 0.12);
export const DefaultExternalVelocityGroundDampingFactor = new THREE.Vector3(0.9992, 0.9992, 0.9992);

export const DefaultTopDownCameraOffset = new THREE.Vector3(0, 80, -47);
export const DefaultTopDownCameraRotation = new THREE.Euler(-1, Math.PI, 0, 'YXZ');
export const DefaultTopDownCameraFOV = 40;
export const DefaultTopDownCameraFocusPoint = { type: 'player' as const };

export interface SceneConfig {
  viewMode?: ViewMode;
  locations: SceneLocations;
  spawnLocation: string;
  /**
   * If true, the current position in the world of the player will be displayed
   */
  debugPos?: boolean;
  debugCamera?: boolean;
  /**
   * If true, the name of the object at the center of the screen will be displayed
   */
  debugTarget?: boolean;
  /**
   * If true, the player's movement and collision world state will be displayed
   */
  debugPlayerKinematics?: boolean;
  gravity?: number;
  /**
   * Tick rate in hertz used to determine the fixed time step for the bullet physics simulation.
   *
   * Default: 160
   */
  simulationTickRate?: number;
  player?: {
    dashConfig?: Partial<DashConfig>;
    jumpVelocity?: number;
    /**
     * Over the course of a second `externalVelocityAirDampingFactor` percent of the external velocity
     * will bleed off while the player is in the air.  So vec3(0.5, 0.5, 0.5) means that 50% of external
     * velocity will be lost every second while in the air.
     */
    externalVelocityAirDampingFactor?: THREE.Vector3;
    /**
     * Over the course of a second `externalVelocityGroundDampingFactor` percent of the external velocity
     * will bleed off while the player is on the ground.  So vec3(0.5, 0.5, 0.5) means that 50% of external
     * velocity will be lost every second while on the ground.
     */
    externalVelocityGroundDampingFactor?: THREE.Vector3;
    colliderSize?: { height: number; radius: number };
    playerColliderShape?: 'capsule' | 'cylinder' | 'sphere';
    moveSpeed?: { onGround: number; inAir: number };
    stepHeight?: number;
    oobYThreshold?: number;
    /**
     * If provided, this mesh will be added to the world and moved in sync with the player.  This is not
     * usually needed in `firstPerson` view mode, but is useful for `top-down` mode.
     */
    mesh?: THREE.Mesh;
  };
  renderOverride?: (timeDiffSeconds: number) => void;
  enableInventory?: boolean;
  sfx?: DeepPartial<SfxConfig>;
  legacyLights?: boolean;
  customControlsEntries?: CustomControlsEntry[];
  /**
   * Default true.  If true, the scene will teleport the player back to the location they were at
   * when reloading after changing graphics settings.
   *
   * This should be set to false for timed or stateful scenes where the player should not be able to
   * start at arbitrary points in the scene.
   */
  goBackOnLoad?: boolean;
}

export const buildDefaultSceneConfig = () => ({
  viewMode: { type: 'firstPerson' as const },
});

type MaybePromise<T> = T | Promise<T>;

export interface SceneDef {
  /**
   * The name of the scene in the Blender file to load
   */
  sceneName: string | null;
  sceneLoader: () => MaybePromise<
    (viz: Viz, loadedWorld: THREE.Group, config: VizConfig, userData?: any) => MaybePromise<SceneConfig>
  >;
  metadata: SvelteSEOProps;
  gltfName?: string | null;
  extension?: 'gltf' | 'glb';
  needsDraco?: boolean;
  legacyLights?: boolean;
}

const ParticleConduit: SceneDef = {
  sceneName: 'blink',
  sceneLoader: () => import('./blink').then(mod => mod.processLoadedScene),
  legacyLights: false,
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
    sceneLoader: () => import('./pkPylons/pkPylons.svelte.ts').then(mod => mod.processLoadedScene),
    sceneName: 'Scene',
    metadata: { title: 'pk_pylons' },
    legacyLights: false,
  },
  tessellationSandbox: {
    gltfName: 'tessellationSandbox',
    extension: 'glb',
    sceneLoader: () =>
      import('./tessellationSandbox/tessellationSandbox').then(mod => mod.processLoadedScene),
    sceneName: 'Scene',
    metadata: { title: 'tessellation sandbox' },
  },
  basalt: {
    gltfName: 'basalt',
    extension: 'glb',
    sceneLoader: () => import('./basalt/basalt').then(mod => mod.processLoadedScene),
    sceneName: 'Scene',
    metadata: { title: 'basalt' },
    legacyLights: false,
  },
  nexus: {
    gltfName: 'nexus',
    extension: 'glb',
    sceneLoader: () => import('./nexus/nexus').then(mod => mod.processLoadedScene),
    sceneName: 'Scene',
    metadata: { title: 'nexus' },
    legacyLights: false,
  },
  csgSandbox: {
    gltfName: 'basalt',
    extension: 'glb',
    sceneLoader: () => import('./csgSandbox/csgSandbox').then(mod => mod.processLoadedScene),
    sceneName: 'Scene',
    metadata: { title: 'CSG sandbox' },
    legacyLights: false,
  },
  ssrSandbox: {
    gltfName: 'basalt',
    extension: 'glb',
    sceneLoader: () => import('./ssrSandbox/ssrSandbox').then(mod => mod.processLoadedScene),
    sceneName: 'Scene',
    metadata: { title: 'SSR sandbox' },
    legacyLights: false,
  },
  movement_v2: {
    gltfName: 'movement_v2',
    extension: 'glb',
    sceneLoader: () => import('./movement_v2/movement_v2.svelte.ts').then(mod => mod.processLoadedScene),
    sceneName: 'Scene',
    metadata: { title: 'Movement V2' },
    legacyLights: false,
  },
  infinite: {
    gltfName: null,
    extension: 'glb',
    sceneLoader: () => import('./infinite/infinite.svelte.ts').then(mod => mod.processLoadedScene),
    sceneName: null,
    metadata: { title: 'Infinite' },
  },
  kinematic_platforms: {
    gltfName: null,
    sceneLoader: () =>
      import('./kinematic_platforms/kinematic_platforms.svelte.ts').then(mod => mod.processLoadedScene),
    sceneName: null,
    metadata: { title: 'kinematic_platforms' },
    legacyLights: false,
  },
  plats: {
    gltfName: 'plats',
    extension: 'glb',
    sceneLoader: () => import('./plats/plats.svelte.ts').then(mod => mod.processLoadedScene),
    sceneName: 'Scene',
    metadata: { title: 'plats' },
    legacyLights: false,
  },
  tutorial: {
    gltfName: 'tutorial',
    extension: 'glb',
    sceneLoader: () => import('./tutorial/tutorial.svelte.ts').then(mod => mod.processLoadedScene),
    sceneName: 'Scene',
    metadata: { title: 'tutorial' },
    legacyLights: false,
  },
  cornered: {
    gltfName: 'cornered',
    extension: 'glb',
    sceneLoader: () => import('./cornered/cornered.svelte.ts').then(mod => mod.processLoadedScene),
    sceneName: 'Scene',
    metadata: { title: 'cornered' },
    legacyLights: false,
  },
  stronghold: {
    gltfName: 'stronghold',
    extension: 'glb',
    sceneLoader: () => import('./stronghold/stronghold.svelte.ts').then(mod => mod.processLoadedScene),
    sceneName: 'Scene',
    metadata: { title: 'stronghold' },
    legacyLights: false,
  },
  geoscript: {
    sceneName: null,
    sceneLoader: () =>
      import('./geoscriptPlayground/geoscriptPlayground.svelte.ts').then(mod => mod.processLoadedScene),
    gltfName: null,
    legacyLights: false,
    metadata: { title: 'geoscript ' },
  },
};
