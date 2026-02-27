import * as THREE from 'three';

import type { Viz } from '../index';
import { buildCustomShader, MaterialClass } from '../shaders/customShader';
import type { BtCollisionObject, BtPairCachingGhostObject } from 'src/ammojs/ammoTypes';
import { delay } from '../util/util';
import killerBulletColorShader from './shaders/killerBulletColor.frag?raw';
import { EasingFnType } from '../util/easingFns';
import { Scheduler } from './Scheduler';
import type { PhysicsTickerHandle } from '../collision';

interface BaseBulletHellEvent {
  /**
   * Time in seconds from the start of the manager at which the event will be initiated
   */
  time: number;
}

export type BulletPattern =
  | {
      /**
       * Spawns bullets with trajectories pointing outwards from the center (`pos`).
       */
      type: 'circle';
      count: number;
      direction: 'cw' | 'ccw';
      /**
       * Defaults to 0, which is straight up
       */
      startAngleRads?: number;
      /**
       * Defaults to 1
       */
      revolutions?: number;
    }
  | { type: 'custom'; getDefs: () => BulletDef[] };

type BulletHellEventVariant =
  | { type: 'spawnBullets'; defs: BulletDef[] }
  | {
      type: 'spawnPattern';
      pattern: BulletPattern;
      /**
       * Global offset of the pattern.  Positions in generated `BulletDef`s are relative to this.
       */
      pos: THREE.Vector3;
      /**
       * Time in seconds between each bullet spawn.  If not specified, all bullets will be spawned at once.
       */
      spawnIntervalSeconds?: number;
      velocity?: number;
      shape?: BulletShape;
    };

export type BulletHellEvent = BaseBulletHellEvent & BulletHellEventVariant;

type BulletShape = { type: 'sphere'; radius: number } | { type: 'custom'; geometry: THREE.BufferGeometry };

type BulletMat = { type: 'standard' } | { type: 'custom'; mat: THREE.Material };

type BulletTrajectory =
  | { type: 'linear'; dir: THREE.Vector3 }
  | {
      type: 'custom';
      getPos: (secondsSinceSpawn: number, tDiffSeconds: number) => THREE.Vector3;
    };

export interface BulletDef {
  shape: BulletShape;
  material?: BulletMat;
  spawnPos: THREE.Vector3;
  trajectory: BulletTrajectory;
}

interface Bullet {
  mesh: THREE.Mesh;
  spawnTime: number;
  def: BulletDef;
}

const DefaultBulletVelocity = 14;
const DefaultBulletShape: BulletShape = Object.freeze({ type: 'sphere', radius: 0.45 });
const MinBulletCollisionDepth = 0.05;

const StandardBulletMat = buildCustomShader(
  { color: 0xc32308, roughness: 0.5, metalness: 0.4 },
  {},
  { materialClass: MaterialClass.Instakill }
);
const KillerBulletMaterial = buildCustomShader({}, { colorShader: killerBulletColorShader }, {});

type BulletHellOutcome = { type: 'win' } | { type: 'loss' };

class BulletManager {
  private viz: Viz;
  private bullets: Bullet[] = [];
  private bounds: THREE.Box3;

  constructor(viz: Viz, bounds: THREE.Box3) {
    this.viz = viz;
    this.bounds = bounds;
  }

  public size = () => this.bullets.length;

  public tick = (curTimeSeconds: number, tDiff: number) => {
    for (let i = this.bullets.length - 1; i >= 0; i--) {
      const bullet = this.bullets[i];
      const elapsed = curTimeSeconds - bullet.spawnTime;

      const newPos = (() => {
        switch (bullet.def.trajectory.type) {
          case 'linear':
            return bullet.def.spawnPos.clone().add(bullet.def.trajectory.dir.clone().multiplyScalar(elapsed));
          case 'custom':
            return bullet.def.trajectory.getPos(elapsed, tDiff);
          default:
            bullet.def.trajectory satisfies never;
            console.error('Unsupported trajectory type', bullet.def.trajectory);
            throw new Error('Unsupported trajectory type');
        }
      })();
      bullet.mesh.position.copy(newPos);
      const bulletCollider: BtCollisionObject = bullet.mesh.userData.collisionObj;
      const tfn = bulletCollider.getWorldTransform();
      tfn.setOrigin(this.viz.fpCtx!.btvec3(newPos.x, newPos.y, newPos.z));
      bulletCollider.setWorldTransform(tfn);

      if (!this.bounds.containsPoint(newPos)) {
        this.despawnBullet(bullet);
        this.bullets[i] = this.bullets[this.bullets.length - 1];
        this.bullets.pop();
      }
    }
  };

  private buildBullet = (def: BulletDef, spawnTimeSeconds: number): Bullet => {
    const geometry = (() => {
      switch (def.shape.type) {
        case 'sphere':
          return new THREE.SphereGeometry(def.shape.radius, 16, 16);
        case 'custom':
          return def.shape.geometry;
        default:
          def.shape satisfies never;
          throw new Error('Unsupported bullet shape');
      }
    })();

    const material = (() => {
      switch (def.material?.type) {
        case undefined:
        case null:
        case 'standard':
          return StandardBulletMat;
        case 'custom':
          return def.material.mat;
        default:
          def.material satisfies never;
          throw new Error('Unsupported material type');
      }
    })();

    const mesh = new THREE.Mesh(geometry, material);
    mesh.position.copy(def.spawnPos);
    mesh.userData.instakill = true;
    this.viz.scene.add(mesh);

    return {
      mesh,
      spawnTime: spawnTimeSeconds,
      def,
    };
  };

  public spawnBullet = (def: BulletDef, spawnTimeSeconds: number) => {
    const bullet = this.buildBullet(def, spawnTimeSeconds);
    this.bullets.push(bullet);
    this.viz.scene.add(bullet.mesh);
    const collisionObj = this.viz.fpCtx!.addPlayerRegionContactCb(
      { type: 'mesh', mesh: bullet.mesh },
      () => this.viz.onInstakillTerrainCollision(collisionObj, bullet.mesh),
      undefined,
      MinBulletCollisionDepth
    );
    bullet.mesh.userData.collisionObj = collisionObj;
  };

  private despawnBullet = (bullet: Bullet) => {
    this.viz.scene.remove(bullet.mesh);
    this.viz.fpCtx!.removePlayerRegionContactCb(bullet.mesh.userData.collisionObj);
  };

  public reset = () => {
    this.bullets.forEach(this.despawnBullet);
    this.bullets = [];
  };
}

export class BulletHellManager {
  private viz: Viz;
  private startPhysicsTimeSeconds: number | null = null;
  private events: BulletHellEvent[] = [];
  private nextEventIx: number = 0;
  private bulletManager: BulletManager;
  private scheduler: Scheduler = new Scheduler();
  private gameEndCb?: (outcome: BulletHellOutcome) => void;
  private isPaused: boolean = false;
  private pausePhysicsTimeSeconds: number = 0;
  private physicsTickerHandle: PhysicsTickerHandle | null = null;

  constructor(viz: Viz, events: BulletHellEvent[], bounds: THREE.Box3) {
    // ensure events are sorted by time
    for (let i = 1; i < events.length; i++) {
      if (events[i].time < events[i - 1].time) {
        throw new Error('Events must be sorted ascending by start time');
      }
    }

    this.viz = viz;
    this.events = events;
    this.bulletManager = new BulletManager(viz, bounds);
  }

  /**
   * Builds bullet definitions for a given pattern. For custom patterns,
   * it offsets each bullet’s spawn position by `pos`.
   */
  private buildPattern = (
    pattern: BulletPattern,
    pos: THREE.Vector3,
    velocity: number,
    shape: BulletShape
  ): BulletDef[] => {
    switch (pattern.type) {
      case 'circle': {
        const count = pattern.count;
        const startAngle = pattern.startAngleRads ?? 0;
        const revolutions = pattern.revolutions ?? 1;
        const deltaAngle = (2 * Math.PI * revolutions) / count;
        const defs: BulletDef[] = [];

        for (let i = 0; i < count; i++) {
          const angle = startAngle + (pattern.direction === 'cw' ? -1 : 1) * i * deltaAngle;
          const dir = new THREE.Vector3(Math.sin(angle), 0, Math.cos(angle))
            .normalize()
            .multiplyScalar(velocity);
          defs.push({
            shape,
            spawnPos: pos.clone(),
            trajectory: { type: 'linear', dir },
          });
        }

        return defs;
      }
      case 'custom':
        return pattern.getDefs().map(def => ({
          ...def,
          spawnPos: def.spawnPos.clone().add(pos),
        }));
      default:
        pattern satisfies never;
        throw new Error('Unsupported pattern type');
    }
  };

  private triggerEvent = (event: BulletHellEvent) => {
    switch (event.type) {
      case 'spawnBullets':
        for (const def of event.defs) {
          this.bulletManager.spawnBullet(def, event.time);
        }
        break;
      case 'spawnPattern': {
        const defs = this.buildPattern(
          event.pattern,
          event.pos,
          event.velocity ?? DefaultBulletVelocity,
          event.shape ?? DefaultBulletShape
        );
        if (!event.spawnIntervalSeconds) {
          for (const def of defs) {
            this.bulletManager.spawnBullet(def, event.time);
          }
          break;
        }

        for (let i = 0; i < defs.length; i++) {
          const def = defs[i];
          this.scheduler.schedule(
            () => void this.bulletManager.spawnBullet(def, event.time + i * event.spawnIntervalSeconds!),
            event.time + i * event.spawnIntervalSeconds
          );
        }
        break;
      }
      default:
        event satisfies never;
        console.warn('Unknown event type:', event);
    }
  };

  private getPhysicsTime = (): number => {
    const fpCtx = this.viz.fpCtx;
    if (!fpCtx) {
      throw new Error('fpCtx not initialized');
    }
    return fpCtx.getPhysicsTime();
  };

  private tickPhysics = (physicsTimeSeconds: number, fixedDtSeconds: number) => {
    KillerBulletMaterial.setCurTimeSeconds(physicsTimeSeconds);

    if (this.isPaused || this.startPhysicsTimeSeconds === null) {
      return;
    }

    const elapsedSinceStart = physicsTimeSeconds - this.startPhysicsTimeSeconds;

    this.scheduler.tick(elapsedSinceStart);

    while (this.nextEventIx < this.events.length && this.events[this.nextEventIx].time <= elapsedSinceStart) {
      this.triggerEvent(this.events[this.nextEventIx]);
      this.nextEventIx += 1;
    }

    this.bulletManager.tick(elapsedSinceStart, fixedDtSeconds);

    if (
      this.nextEventIx >= this.events.length &&
      this.bulletManager.size() === 0 &&
      (!this.events.length || elapsedSinceStart >= this.events[this.events.length - 1].time + 1)
    ) {
      this.gameEndCb?.({ type: 'win' });
      this.reset();
    }
  };

  public start = (): Promise<BulletHellOutcome> => {
    const promise = new Promise<BulletHellOutcome>(resolve => {
      this.gameEndCb = resolve;
    });
    if (this.startPhysicsTimeSeconds !== null) {
      throw new Error('`BulletHellManager` already started');
    }
    const fpCtx = this.viz.fpCtx;
    if (!fpCtx) {
      throw new Error('fpCtx not initialized');
    }

    this.startPhysicsTimeSeconds = this.getPhysicsTime();
    this.pausePhysicsTimeSeconds = 0;
    this.physicsTickerHandle = fpCtx.registerPhysicsTicker({ tick: this.tickPhysics });
    this.viz.registerOnRespawnCb(this.reset);
    this.viz.setOnInstakillTerrainCollisionCb(this.onBulletHit);

    return promise;
  };

  public reset = () => {
    this.startPhysicsTimeSeconds = null;
    this.pausePhysicsTimeSeconds = 0;
    this.nextEventIx = 0;
    this.bulletManager.reset();
    this.scheduler.clear();
    this.gameEndCb = undefined;
    this.isPaused = false;
    this.physicsTickerHandle?.unregister();
    this.physicsTickerHandle = null;
    this.viz.unregisterOnRespawnCb(this.reset);
    this.viz.setOnInstakillTerrainCollisionCb(null);
  };

  public pause = () => {
    if (this.startPhysicsTimeSeconds === null) {
      return;
    }
    this.pausePhysicsTimeSeconds = this.getPhysicsTime();
    this.isPaused = true;
  };

  public resume = () => {
    if (this.startPhysicsTimeSeconds === null) {
      return;
    }
    this.startPhysicsTimeSeconds += this.getPhysicsTime() - this.pausePhysicsTimeSeconds;
    this.isPaused = false;
  };

  private onBulletHit = async (_sensor: BtPairCachingGhostObject, bulletMesh: THREE.Mesh | null) => {
    if (this.isPaused) {
      return;
    }

    this.pause();
    this.viz.controlState.movementEnabled = false;
    this.viz.controlState.cameraControlEnabled = false;
    this.viz.fpCtx!.playerController.setExternalVelocity(this.viz.fpCtx!.btvec3(0, 0, 0));
    for (const k of Object.keys(this.viz.keyStates)) {
      this.viz.keyStates[k] = false;
    }
    this.viz.sfxManager.playSfx('player_die');

    const animationLengthSecs = 2;
    let didRunDeathAnimation = false;

    if (bulletMesh) {
      bulletMesh.material = KillerBulletMaterial;

      const startCameraPos = this.viz.camera.position.clone();
      const endCameraPos = bulletMesh!.position.clone().lerp(startCameraPos, 0.2);
      const startCameraRot = this.viz.camera.rotation.clone();

      try {
        let animationDoneCb!: () => void;
        const animationDonePromise = new Promise<void>(resolve => {
          animationDoneCb = resolve;
        });

        this.viz.startViewModeInterpolation(
          {
            durationSecs: animationLengthSecs,
            endCameraFov: this.viz.camera.fov,
            endCameraPos,
            endCameraRot: startCameraRot,
            startCameraFov: this.viz.camera.fov,
            startCameraPos,
            startCameraRot,
            startTimeSecs: this.viz.clock.getElapsedTime(),
          },
          EasingFnType.OutCubic,
          animationDoneCb,
          0.3
        );

        await animationDonePromise;
        didRunDeathAnimation = true;
      } catch (err) {
        console.warn('Some view mode interpolation already going on; skipping death animation', err);
      }
    } else {
      console.error('No bullet mesh found');
    }

    if (!didRunDeathAnimation) {
      await delay(animationLengthSecs * 1000);
    }

    this.gameEndCb?.({ type: 'loss' });
    this.reset();
    this.viz.controlState.movementEnabled = true;
    this.viz.controlState.cameraControlEnabled = true;
  };

}
