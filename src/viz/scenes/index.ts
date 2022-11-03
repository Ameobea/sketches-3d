import type { SvelteSeoProps } from 'svelte-seo/types/SvelteSeo';

import type { VizState } from '..';

export interface SceneConfig {
  viewMode?: { type: 'firstPerson' } | { type: 'orbit'; pos: THREE.Vector3; target: THREE.Vector3 };
  locations: {
    [key: string]: {
      pos: THREE.Vector3;
      rot: THREE.Vector3;
    };
  };
  spawnLocation: string;
  debugPos?: boolean;
  gravity?: number;
  player?: {
    /**
     * Default is true
     */
    enableDash?: boolean;
    jumpVelocity?: number;
    colliderCapsuleSize?: { height: number; radius: number };
    movementAccelPerSecond?: { onGround: number; inAir: number };
  };
  renderOverride?: (timeDiffSeconds: number) => void;
  enableInventory?: boolean;
}

export const buildDefaultSceneConfig = () => ({
  viewMode: { type: 'firstPerson' as const },
});

const ParticleConduit = {
  sceneName: 'blink',
  sceneLoader: () => import('./blink').then(mod => mod.processLoadedScene),
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
          url: 'https://ameo.link/u/a9c.png',
          alt: 'A screenshot of the particle conduit visualization.  Shows red and orange particles moving in stream-like patterns across the screen.  Includes a control panel on the right with many controls for configuring the simulation.',
          width: 1508,
          height: 1194,
        },
      ],
    },
  },
};

interface SceneDef {
  sceneName: string | null;
  sceneLoader: () => Promise<(viz: VizState, loadedWorld: THREE.Group) => SceneConfig | Promise<SceneConfig>>;
  metadata: SvelteSeoProps;
  gltfName?: string | null;
}

export const ScenesByName: { [key: string]: SceneDef } = {
  bridge: {
    sceneName: 'bridge',
    sceneLoader: () => import('./bridge').then(mod => mod.processLoadedScene),
    metadata: { title: 'bridge' },
  },
  blink: ParticleConduit,
  particle_conduit: ParticleConduit,
  walkways: {
    sceneName: 'walkways',
    sceneLoader: () => import('./walkways').then(mod => mod.processLoadedScene),
    metadata: { title: 'walkways' },
  },
  subdivided: {
    sceneName: 'subdivided',
    sceneLoader: () => import('./subdivided').then(mod => mod.processLoadedScene),
    metadata: { title: 'subdivided' },
  },
  fractal_cube: {
    sceneName: 'fractal_cube',
    sceneLoader: () => import('./fractal_cube').then(mod => mod.processLoadedScene),
    metadata: { title: 'fractal cube' },
  },
  bridge2: {
    sceneName: 'bridge2',
    sceneLoader: () => import('./bridge2').then(mod => mod.processLoadedScene),
    metadata: { title: 'bridge2' },
    gltfName: 'checkpoint5',
  },
  collisiondemo: {
    sceneName: 'collisionDemo',
    sceneLoader: () => import('./collisionDemo').then(mod => mod.processLoadedScene),
    metadata: { title: 'collisionDemo' },
  },
  chasms: {
    sceneName: 'chasms',
    sceneLoader: () => import('./chasms/chasms').then(mod => mod.processLoadedScene),
    metadata: { title: 'chasms' },
    gltfName: 'chasms',
  },
  godrays_test: {
    sceneName: 'godrays_test',
    sceneLoader: () => import('./godrays-test/godraysTest').then(mod => mod.processLoadedScene),
    metadata: { title: 'godrays_test' },
    gltfName: null,
  },
  rainy: {
    sceneName: 'Scene',
    sceneLoader: () => import('./rainy/rainy').then(mod => mod.processLoadedScene),
    metadata: { title: 'rainy' },
    gltfName: 'rainy',
  },
  depthPrepassDemo: {
    sceneName: null,
    sceneLoader: () => import('./depthPrepassDemo').then(mod => mod.processLoadedScene),
    metadata: { title: 'depthPrepassDemo' },
    gltfName: null,
  },
};
