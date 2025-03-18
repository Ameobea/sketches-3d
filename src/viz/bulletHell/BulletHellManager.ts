import * as THREE from 'three';

import type { Viz } from '../index';
import { buildCustomShader, MaterialClass } from '../shaders/customShader';
import type { BtCollisionObject } from 'src/ammojs/ammoTypes';

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

const DefaultBulletVelocity = 20;
const DefaultBulletShape: BulletShape = Object.freeze({ type: 'sphere', radius: 1 });
const MinBulletCollisionDepth = 0.2;

const StandardBulletMat = buildCustomShader(
  { color: 0xff0000 },
  {},
  { materialClass: MaterialClass.Instakill }
);

interface ScheduledEvent {
  time: number;
  callback: () => void;
  /**
   * If set, the callback will be re-called every `interval` seconds after the first call
   */
  interval?: number;
  id: number;
  cancelled: boolean;
}

export interface SchedulerHandle {
  cancel: () => void;
}

class Scheduler {
  private events: ScheduledEvent[] = [];
  private nextId: number = 0;

  public schedule(callback: () => void, time: number, interval?: number): SchedulerHandle {
    const event: ScheduledEvent = {
      time,
      callback,
      id: this.nextId++,
      cancelled: false,
      interval,
    };
    this.push(event);
    return {
      cancel: () => {
        event.cancelled = true;
      },
    };
  }

  public tick(currentTime: number) {
    while (this.events.length > 0 && this.peek().time <= currentTime) {
      const event = this.pop();
      if (event.cancelled) {
        continue;
      }

      event.callback();
      if (!!event.interval && !event.cancelled) {
        event.time += event.interval;
        this.push(event);
      }
    }
  }

  private push(event: ScheduledEvent) {
    this.events.push(event);
    this.heapifyUp(this.events.length - 1);
  }

  private pop(): ScheduledEvent {
    const top = this.events[0];
    const last = this.events.pop()!;
    if (this.events.length > 0) {
      this.events[0] = last;
      this.heapifyDown(0);
    }
    return top;
  }

  private peek(): ScheduledEvent {
    return this.events[0];
  }

  private heapifyUp(index: number) {
    while (index > 0) {
      const parent = Math.floor((index - 1) / 2);
      if (this.events[parent].time <= this.events[index].time) {
        break;
      }
      [this.events[parent], this.events[index]] = [this.events[index], this.events[parent]];
      index = parent;
    }
  }

  private heapifyDown(index: number) {
    const length = this.events.length;
    while (true) {
      const left = 2 * index + 1;
      const right = 2 * index + 2;
      let smallest = index;
      if (left < length && this.events[left].time < this.events[smallest].time) {
        smallest = left;
      }
      if (right < length && this.events[right].time < this.events[smallest].time) {
        smallest = right;
      }
      if (smallest === index) break;
      [this.events[smallest], this.events[index]] = [this.events[index], this.events[smallest]];
      index = smallest;
    }
  }

  public clear() {
    this.events = [];
  }
}

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

  private buildBullet = (def: BulletDef): Bullet => {
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
      spawnTime: this.viz.clock.getElapsedTime(),
      def,
    };
  };

  public spawnBullet = (def: BulletDef) => {
    const bullet = this.buildBullet(def);
    this.bullets.push(bullet);
    this.viz.scene.add(bullet.mesh);
    // this.viz.fpCtx!.addTriMesh(bullet.mesh);
    const collisionObj = this.viz.fpCtx!.addPlayerRegionContactCb(
      { type: 'mesh', mesh: bullet.mesh },
      this.viz.onInstakillTerrainCollision,
      undefined,
      MinBulletCollisionDepth
    );
    bullet.mesh.userData.collisionObj = collisionObj;
  };

  private despawnBullet = (bullet: Bullet) => {
    this.viz.scene.remove(bullet.mesh);
    this.viz.fpCtx!.removeCollisionObject(bullet.mesh.userData.collisionObj);
  };

  public reset = () => {
    this.bullets.forEach(bullet => {
      this.viz.scene.remove(bullet.mesh);
      this.viz.fpCtx!.removeCollisionObject(bullet.mesh.userData.collisionObj);
    });
    this.bullets = [];
  };
}

export class BulletHellManager {
  private viz: Viz;
  private startTimeSeconds: number = 0;
  private events: BulletHellEvent[] = [];
  private nextEventIx: number = 0;
  private bulletManager: BulletManager;
  private onWin?: () => void;
  private scheduler: Scheduler = new Scheduler();

  constructor(viz: Viz, events: BulletHellEvent[], bounds: THREE.Box3, onWin?: () => void) {
    // ensure events are sorted by time
    for (let i = 1; i < events.length; i++) {
      if (events[i].time < events[i - 1].time) {
        throw new Error('Events must be sorted ascending by start time');
      }
    }

    this.viz = viz;
    this.events = events;
    this.bulletManager = new BulletManager(viz, bounds);
    this.onWin = onWin;
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
          this.bulletManager.spawnBullet(def);
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
            this.bulletManager.spawnBullet(def);
          }
          break;
        }

        for (let i = 0; i < defs.length; i++) {
          const def = defs[i];
          this.scheduler.schedule(
            () => void this.bulletManager.spawnBullet(def),
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

  private tick = (curTimeSeconds: number, tDiffSeconds: number) => {
    const elapsedSinceStart = curTimeSeconds - this.startTimeSeconds;

    this.scheduler.tick(curTimeSeconds);

    while (this.nextEventIx < this.events.length && this.events[this.nextEventIx].time <= elapsedSinceStart) {
      this.triggerEvent(this.events[this.nextEventIx]);
      this.nextEventIx += 1;
    }

    this.bulletManager.tick(curTimeSeconds, tDiffSeconds);

    if (
      this.nextEventIx >= this.events.length &&
      this.bulletManager.size() === 0 &&
      (!this.events.length || elapsedSinceStart >= this.events[this.events.length - 1].time + 1)
    ) {
      this.onWin?.();
      this.reset();
    }
  };

  public start = () => {
    if (this.startTimeSeconds !== 0) {
      throw new Error('`BulletHellManager` already started');
    }

    this.startTimeSeconds = this.viz.clock.getElapsedTime();
    this.viz.registerBeforeRenderCb(this.tick);
    this.viz.registerOnRespawnCb(this.reset);
  };

  public reset = () => {
    this.startTimeSeconds = 0;
    this.nextEventIx = 0;
    this.bulletManager.reset();
    this.scheduler.clear();
    this.viz.unregisterBeforeRenderCb(this.tick);
    this.viz.unregisterOnRespawnCb(this.reset);
  };

  /**
   * Registers a periodic callback that will be executed at `initialTimeSeconds` and then repeatedly every
   * `intervalSeconds` after that.
   */
  private schedulePeriodic(
    callback: () => void,
    initialTimeSeconds: number,
    intervalSeconds: number
  ): SchedulerHandle {
    return this.scheduler.schedule(callback, initialTimeSeconds, intervalSeconds);
  }
}
