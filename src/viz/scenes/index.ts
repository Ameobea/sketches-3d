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

export const ScenesByName: {
  [key: string]: {
    sceneName: string;
    sceneLoader: () => Promise<
      (viz: VizState, loadedWorld: THREE.Group) => SceneConfig | Promise<SceneConfig>
    >;
    metadata: SvelteSeoProps;
  };
} = {
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
};
