import * as THREE from 'three';
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js';

import type { Viz } from 'src/viz';
import type { BtDashToken } from 'src/ammojs/ammoTypes';
import { rwritable } from '../util/TransparentWritable';
import { clearPhysicsBindings, withPhysicsContext } from '../util/physics';
import type { BulletPhysics } from '../collision';
import type { DashTokenMaterials } from './dashTokenMaterials';

/**
 * A dash token to spawn. The visual is added under `parent` (a level-def marker anchor, so it
 * inherits the marker's transform — or `viz.scene` for legacy Blender markers) at `localPosition`.
 * `id` is the source level-def marker id; set it to wire editor selection.
 */
export interface DashTokenSpawn {
  parent: THREE.Object3D;
  localPosition?: THREE.Vector3;
  id?: string;
}

export class DashToken extends THREE.Object3D {
  private viz: Viz;
  private base: THREE.Object3D;
  /** Tracks whether physics bindings have been cleared from the shared base object. */
  private static clearedBases = new WeakSet<THREE.Object3D>();

  constructor(viz: Viz, base: THREE.Object3D) {
    super();
    this.viz = viz;
    this.base = base;

    // Bob/spin live on an inner group so `this` stays a clean transform anchor the editor can drive.
    const inner = new THREE.Group();
    inner.scale.setScalar(0.9);
    this.add(inner);

    // Only clear physics on the base once — clones share the same base object.
    if (!DashToken.clearedBases.has(base)) {
      DashToken.clearedBases.add(base);
      withPhysicsContext(viz, fpCtx => clearPhysicsBindings(base, fpCtx));
    }

    const core = base.children.find(c => c.name.includes('core'))!.clone();
    const rings = base.children.filter(c => c.name.includes('ring')).map(obj => obj.clone()) as THREE.Mesh[];

    // the planes within which the rings are oriented
    const ringInitialPlanes: THREE.Plane[] = [];

    // rads/sec
    const baseRotationSpeed = 2.5;
    const rotationSpeeds: number[] = [];

    // Randomize rotation axis for each ring
    rings.forEach(ring => {
      // rings start out sitting in the XZ plane
      const plane = new THREE.Plane().setFromCoplanarPoints(
        new THREE.Vector3(0, 0, 0),
        new THREE.Vector3(0, 0, 1),
        new THREE.Vector3(1, 0, 0)
      );

      // randomize the plane's normal
      const rotationAxis = new THREE.Vector3(
        Math.random() * 2 - 1,
        Math.random() * 2 - 1,
        Math.random() * 2 - 1
      ).normalize();
      plane.normal.copy(rotationAxis);

      // rotate the ring to match the plane
      const rotationMatrix = new THREE.Matrix4().lookAt(
        new THREE.Vector3(),
        plane.normal,
        new THREE.Vector3(0, 1, 0)
      );
      ring.applyMatrix4(rotationMatrix);

      ringInitialPlanes.push(plane);
      rotationSpeeds.push(0.5 * baseRotationSpeed + baseRotationSpeed * Math.random());
    });

    inner.add(core, ...rings);

    let lastBobPhase = 0;
    const bobHeight = 0.2;
    const bobRate = 1.5;

    viz.registerBeforeRenderCb((curTimeSeconds, tDiffSeconds) => {
      const bobPhase = Math.sin(curTimeSeconds * bobRate);
      const bobDelta = bobPhase - lastBobPhase;
      lastBobPhase = bobPhase;

      // bob up and down
      inner.position.y += bobDelta * bobHeight;

      rings.forEach((ring, index) => {
        // spin the ring around its plane's normal
        const plane = ringInitialPlanes[index];
        const rotationAxis = plane.normal.clone();

        const rotationSpeed = rotationSpeeds[index];
        ring.rotateOnWorldAxis(rotationAxis, rotationSpeed * tDiffSeconds);
      });
    });
  }

  public override clone(recursive?: boolean): this {
    if (recursive === false) {
      throw new Error('DashToken.clone: clone() without recursive support is not implemented.');
    }

    const clone = new DashToken(this.viz, this.base);
    return clone as this;
  }
}

export const initDashTokenGraphics = (
  loadedWorld: THREE.Group,
  coreMaterial: THREE.Material,
  ringMaterial: THREE.Material
): THREE.Object3D | undefined => {
  const base = loadedWorld.getObjectByName('dash_token');
  if (!base) {
    return;
  }

  base.visible = false;
  applyDashTokenMaterials(base, coreMaterial, ringMaterial);
  return base;
};

const applyDashTokenMaterials = (
  base: THREE.Object3D,
  coreMaterial: THREE.Material,
  ringMaterial: THREE.Material
) =>
  base.traverse(obj => {
    if (!(obj instanceof THREE.Mesh)) {
      return;
    }
    if (obj.name.includes('ring')) {
      obj.material = ringMaterial;
    } else if (obj.name.includes('core')) {
      obj.material = coreMaterial;
    }
  });

const buildFallbackDashTokenBase = (
  coreMaterial: THREE.Material,
  ringMaterial: THREE.Material
): THREE.Object3D => {
  const base = new THREE.Group();
  const core = new THREE.Mesh(new THREE.SphereGeometry(0.5), coreMaterial);
  core.name = 'core';
  base.add(core);
  const ring = new THREE.Mesh(new THREE.TorusGeometry(0.75, 0.1, 8, 24), ringMaterial);
  ring.name = 'ring';
  base.add(ring);
  return base;
};

/** The reusable default dash-token mesh (core + rings), extracted from `plats.glb`. */
const DEFAULT_DASH_TOKEN_URL = 'dash_token.glb';
let defaultDashTokenBase: Promise<THREE.Object3D> | null = null;

/** Loads the shared default dash-token base mesh on demand (memoized across the page). */
export const loadDefaultDashTokenBase = (): Promise<THREE.Object3D> => {
  if (!defaultDashTokenBase) {
    defaultDashTokenBase = new Promise((resolve, reject) => {
      new GLTFLoader().setPath('/').load(
        DEFAULT_DASH_TOKEN_URL,
        gltf => {
          const base = gltf.scene.getObjectByName('dash_token');
          if (!base) {
            reject(new Error(`"dash_token" not found in ${DEFAULT_DASH_TOKEN_URL}`));
            return;
          }
          resolve(base);
        },
        undefined,
        reject
      );
    });
  }
  return defaultDashTokenBase;
};

/**
 * Spawns animated dash-token visuals + physics ghost tokens. Pass `markers` (level-def path) to
 * attach tokens under explicit anchor objects using the on-demand default mesh; otherwise the legacy
 * Blender path places one per `dash_token_loc` marker found in `loadedWorld`. Constructs nothing
 * when there are no tokens to place. `getMaterials` is invoked lazily — only once tokens are
 * confirmed — so a scene with no dash tokens never builds/fetches the default materials.
 */
export const initDashTokens = (
  viz: Viz,
  loadedWorld: THREE.Group,
  getMaterials: () => DashTokenMaterials | Promise<DashTokenMaterials>,
  dashCharges = rwritable(0),
  markers?: DashTokenSpawn[]
) => {
  const entries: { token: BtDashToken | null; visual: THREE.Object3D; halfExtents: THREE.Vector3 }[] = [];

  const computeHalfExtents = (obj: THREE.Object3D): THREE.Vector3 => {
    const halfExtents = new THREE.Vector3();
    obj.traverse(child => {
      if (!(child instanceof THREE.Mesh)) {
        return;
      }
      const box = new THREE.Box3().setFromObject(child);
      const size = new THREE.Vector3();
      box.getSize(size);
      halfExtents.max(size.divideScalar(2));
    });
    return halfExtents;
  };

  const syncFromController = () => {
    const fpCtx = viz.fpCtx;
    if (!fpCtx) {
      return;
    }

    const charges = fpCtx.getDashCharges();
    if (dashCharges.current !== charges) {
      dashCharges.set(charges);
    }
    for (const entry of entries) {
      if (!entry.token) {
        continue;
      }
      const isActive = entry.token.isActive();
      if (entry.visual.visible !== isActive) {
        entry.visual.visible = isActive;
      }
    }
  };

  const registerTokens = (fpCtx: BulletPhysics) => {
    fpCtx.playerController.setDashCharges(dashCharges.current);
    for (const entry of entries) {
      if (entry.token) {
        continue;
      }
      const tokenEntry = fpCtx.addDashToken(
        {
          type: 'box',
          halfExtents: entry.halfExtents,
          pos: entry.visual.getWorldPosition(new THREE.Vector3()),
        },
        { chargesGranted: 1 },
        () => {
          entry.visual.visible = false;
          viz.sfxManager.playSfx('dash_pickup');
        }
      );
      entry.token = tokenEntry.token;
    }

    fpCtx.captureInitialDashState();
    fpCtx.saveDashCheckpointState();
    syncFromController();
  };

  const placeTokens = (base: THREE.Object3D, spawns: DashTokenSpawn[]) => {
    for (const spawn of spawns) {
      const visual = new DashToken(viz, base);
      visual.name = `dash_token_${entries.length}`;
      if (spawn.localPosition) visual.position.copy(spawn.localPosition);
      spawn.parent.add(visual);
      // Parented under its marker, the token is selectable + transformable as that node in the editor.
      if (spawn.id) {
        visual.userData.levelDefId = spawn.id;
        viz.levelLoadHandle?.registerEditorSelectable(visual);
      }
      entries.push({ token: null, visual, halfExtents: computeHalfExtents(visual) });
    }
    withPhysicsContext(viz, registerTokens);
  };

  if (markers && markers.length > 0) {
    Promise.all([loadDefaultDashTokenBase(), Promise.resolve(getMaterials())])
      .then(([proto, { core, ring }]) => {
        const base = proto.clone();
        base.visible = false;
        applyDashTokenMaterials(base, core, ring);
        placeTokens(base, markers);
      })
      .catch(err => console.error('[DashToken] failed to load default dash-token mesh:', err));
  } else {
    const locMarkers: DashTokenSpawn[] = [];
    loadedWorld.traverse(obj => {
      if (obj.name.includes('dash_token_loc')) {
        locMarkers.push({ parent: viz.scene, localPosition: obj.position.clone() });
      }
    });
    if (locMarkers.length > 0) {
      Promise.resolve(getMaterials())
        .then(({ core, ring }) => {
          const base =
            initDashTokenGraphics(loadedWorld, core, ring) ?? buildFallbackDashTokenBase(core, ring);
          placeTokens(base, locMarkers);
        })
        .catch(err => console.error('[DashToken] failed to build dash-token materials:', err));
    }
  }

  viz.registerBeforeRenderCb(() => {
    syncFromController();
  });

  return { syncFromController };
};
