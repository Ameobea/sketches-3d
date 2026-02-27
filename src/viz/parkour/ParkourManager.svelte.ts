import * as THREE from 'three';
import { mount, unmount } from 'svelte';

import type { Viz } from 'src/viz';
import type { VizConfig } from 'src/viz/conf';
import { initDashTokens } from './DashToken';
import { initCheckpoints } from './checkpoints';
import TimerDisplay from './TimerDisplay.svelte';
import type { ScoreThresholds } from './TimeDisplay.svelte';
import TimeDisplay from './TimeDisplay.svelte';
import type { SceneConfig } from '../scenes';
import { API, MetricsAPI } from 'src/api/client';
import { rwritable, type TransparentWritable } from '../util/TransparentWritable';
import type { BtRigidBody } from 'src/ammojs/ammoTypes';
import { Scheduler, type SchedulerHandle } from '../bulletHell/Scheduler';
import { getAmmoJS, type PhysicsTicker, type PhysicsTickerHandle } from '../collision';

export interface ParkourMaterials {
  dashToken: {
    core: THREE.Material;
    ring: THREE.Material;
  };
  checkpoint: THREE.Material;
}

interface MakeSliderArgs {
  getPos: (curTimeSeconds: number, secondsSinceSpawn: number) => THREE.Vector3;
  despawnCond?: (mesh: THREE.Mesh, curTimeSeconds: number) => boolean;
  /**
   * default true
   */
  removeOnReset?: boolean;
  spawnTimeSeconds?: number;
}

export class ParkourManager {
  private viz: Viz;
  private loadedWorld: THREE.Group;
  private locations: { [key: string]: { pos: THREE.Vector3; rot: THREE.Vector3 } };
  private scoreThresholds: ScoreThresholds;
  private mapID: string;
  private useExternalVelocity: boolean;

  private curDashCharges: TransparentWritable<number> = rwritable(0);
  private lastResetPhysicsTime: number = 0;
  private curRunStartTimeSeconds: number | null = null;
  private winState: { winTimeSeconds: number; displayComp: any } | null = null;
  private managerTickerHandle: PhysicsTickerHandle | null = null;
  private pendingPhysicsActions: (() => void)[] = [];
  private resetLifecycleCbs: (() => boolean | void)[] = [];
  private destroyLifecycleCbs: (() => void)[] = [];
  private physicsTickerHandles: PhysicsTickerHandle[] = [];
  private initializedSpinners = new WeakSet<THREE.Mesh>();
  private onStartCbs: (() => void)[] = [];
  private timerDisplay!: any;
  private scheduler: Scheduler = new Scheduler();

  private resetDashes: () => void = () => {
    throw new Error('materials not set yet');
  };
  private resetCheckpoints: () => void = () => {
    throw new Error('materials not set yet');
  };

  constructor(
    viz: Viz,
    loadedWorld: THREE.Group,
    vizConf: VizConfig,
    locations: { [key: string]: { pos: THREE.Vector3; rot: THREE.Vector3 } },
    scoreThresholds: ScoreThresholds,
    materials: ParkourMaterials | undefined,
    mapID: string,
    useExternalVelocity: boolean
  ) {
    // pre-load physics engine since we know we'll need it and the viz can't start loading that until we return from
    // `processLoadedScene` function
    getAmmoJS();

    this.viz = viz;
    this.loadedWorld = loadedWorld;
    this.locations = locations;
    this.scoreThresholds = scoreThresholds;
    this.mapID = mapID;
    this.useExternalVelocity = useExternalVelocity;

    if (materials) {
      this.setMaterials(materials);
    }

    this.initTimer();

    viz.collisionWorldLoadedCbs.push(fpCtx => {
      this.lastResetPhysicsTime = fpCtx.getPhysicsTime();
      fpCtx.registerJumpCb(() => {
        if (this.curRunStartTimeSeconds === null) {
          this.curRunStartTimeSeconds = fpCtx.getPhysicsTime();
        }
      });

      let didStart = false;
      this.managerTickerHandle = fpCtx.registerPhysicsTicker({
        tick: (physicsTime: number) => {
          this.flushPendingPhysicsActions();
          if (!didStart) {
            didStart = true;
            this.runOnStartCbs();
          }
          this.scheduler.tick(physicsTime - this.lastResetPhysicsTime);
        },
      });
    });

    viz.registerDestroyedCb(this.destroy);
  }

  public setMaterials = (materials: ParkourMaterials) => {
    const {
      ctx: dashTokensCtx,
      dashCharges: curDashCharges,
      reset: resetDashes,
    } = initDashTokens(
      this.viz,
      this.loadedWorld,
      materials.dashToken.core,
      materials.dashToken.ring,
      this.curDashCharges
    );
    this.resetCheckpoints = initCheckpoints(
      this.viz,
      this.loadedWorld,
      materials.checkpoint,
      dashTokensCtx,
      curDashCharges,
      this.onWin
    );
    this.resetDashes = resetDashes;
  };

  private initTimer = () => {
    const target = document.createElement('div');
    document.body.appendChild(target);
    const timerDisplayProps = $state({ curTime: 0 });
    this.timerDisplay = mount(TimerDisplay, { target, props: timerDisplayProps });
    this.viz.registerAfterRenderCb(() => {
      const elapsedSeconds = (() => {
        if (this.curRunStartTimeSeconds === null) {
          return 0;
        }

        if (this.winState) {
          return this.winState.winTimeSeconds - this.curRunStartTimeSeconds;
        }

        const fpCtx = this.viz.fpCtx;
        if (!fpCtx) {
          return 0;
        }
        return fpCtx.getPhysicsTime() - this.curRunStartTimeSeconds;
      })();
      timerDisplayProps.curTime = elapsedSeconds;
    });
  };

  public registerOnStartCb = (cb: () => void) => {
    this.onStartCbs.push(cb);
  };

  private runOnStartCbs = () => {
    for (const cb of this.onStartCbs) {
      cb();
    }
  };

  private registerManagedPhysicsTicker = (
    ticker: PhysicsTicker,
    opts?: { mesh?: THREE.Object3D; body?: BtRigidBody }
  ): PhysicsTickerHandle => {
    const fpCtx = this.viz.fpCtx;
    if (!fpCtx) {
      throw new Error('fpCtx not initialized');
    }
    const handle = fpCtx.registerPhysicsTicker(ticker, opts);
    this.physicsTickerHandles.push(handle);
    return handle;
  };

  private unregisterManagedPhysicsTicker = (handle: PhysicsTickerHandle) => {
    handle.unregister();
    const handleIx = this.physicsTickerHandles.indexOf(handle);
    if (handleIx !== -1) {
      this.physicsTickerHandles[handleIx] = this.physicsTickerHandles[this.physicsTickerHandles.length - 1];
      this.physicsTickerHandles.pop();
    }
  };

  private unregisterAllManagedPhysicsTickers = () => {
    for (const handle of this.physicsTickerHandles) {
      handle.unregister();
    }
    this.physicsTickerHandles = [];
  };

  private registerResetLifecycleCb = (cb: () => boolean | void) => {
    this.resetLifecycleCbs.push(cb);
  };

  private registerDestroyLifecycleCb = (cb: () => void) => {
    this.destroyLifecycleCbs.push(cb);
  };

  private queuePhysicsAction = (cb: () => void) => {
    this.pendingPhysicsActions.push(cb);
  };

  private flushPendingPhysicsActions = () => {
    while (this.pendingPhysicsActions.length > 0) {
      const cb = this.pendingPhysicsActions.shift();
      cb?.();
    }
  };

  private reset = () => {
    const fpCtx = this.viz.fpCtx!;
    const physicsTime = fpCtx.getPhysicsTime();
    const elapsedTimeSeconds = physicsTime - (this.curRunStartTimeSeconds ?? 0);
    this.resetDashes();
    this.resetCheckpoints();
    fpCtx.teleportPlayer(this.locations.spawn.pos, this.locations.spawn.rot);
    fpCtx.reset();
    const wasStarted = this.curRunStartTimeSeconds !== null;
    this.curRunStartTimeSeconds = null;
    this.lastResetPhysicsTime = physicsTime;
    if (this.winState?.displayComp) {
      unmount(this.winState.displayComp);
    }
    const wasWin = !!this.winState;
    this.winState = null;
    this.viz.setSpawnPos(this.locations.spawn.pos, this.locations.spawn.rot);

    this.flushPendingPhysicsActions();
    this.unregisterAllManagedPhysicsTickers();

    const newResetLifecycleCbs: (() => boolean | void)[] = [];
    for (const cb of this.resetLifecycleCbs) {
      const retain = cb();
      if (retain !== false) {
        newResetLifecycleCbs.push(cb);
      }
    }
    this.resetLifecycleCbs = newResetLifecycleCbs;
    this.scheduler.clear();
    this.runOnStartCbs();

    if (!wasWin && wasStarted) {
      MetricsAPI.recordRestart(this.mapID, elapsedTimeSeconds);
    }
  };

  private onWin = () => {
    const physicsTime = this.viz.fpCtx!.getPhysicsTime();

    const target = document.createElement('div');
    document.body.appendChild(target);
    const time = physicsTime - (this.curRunStartTimeSeconds ?? 0);
    const timeDisplayProps = $state({
      scoreThresholds: this.scoreThresholds,
      time,
      mapID: this.mapID,
      userPlayID: null as string | null,
      userID: null as string | null,
    });
    const displayComp = mount(TimeDisplay, {
      target,
      props: timeDisplayProps,
    });
    this.winState = { winTimeSeconds: physicsTime, displayComp };

    this.viz.setSpawnPos(this.locations.spawn.pos, this.locations.spawn.rot);

    API.addPlay({ mapId: this.mapID, timeLength: time })
      .then(res => {
        timeDisplayProps.userPlayID = res.id ?? null;
        timeDisplayProps.userID = res.playerId ?? null;
      })
      .catch(() => {});

    MetricsAPI.recordPlayCompletion(this.mapID, true, time);
  };

  public makeSpinner = (mesh: THREE.Mesh, rpm: number) => {
    if (this.initializedSpinners.has(mesh)) {
      return;
    }
    this.initializedSpinners.add(mesh);

    const fpCtx = this.viz.fpCtx;
    if (!fpCtx) {
      throw new Error('fpCtx not initialized');
    }

    const rigidBody = mesh.userData.rigidBody as BtRigidBody;
    rigidBody.setCollisionFlags(2); // btCollisionObject::CF_KINEMATIC_OBJECT
    rigidBody.setActivationState(4); // DISABLE_DEACTIVATION
    const tfn = new fpCtx.Ammo.btTransform();
    tfn.setIdentity();
    tfn.setOrigin(fpCtx.btvec3(mesh.position.x, mesh.position.y, mesh.position.z));
    const initialRot = mesh.rotation.y;
    const rps = rpm / 60;

    const makeSpinnerTicker = () => ({
      tick: (physicsTime: number) => {
        const elapsed = physicsTime - this.lastResetPhysicsTime;
        tfn.setEulerZYX(0, initialRot - rps * elapsed * Math.PI * 2, 0);
        rigidBody.setWorldTransform(tfn);
      },
    });

    this.registerManagedPhysicsTicker(makeSpinnerTicker(), { mesh, body: rigidBody });
    this.registerResetLifecycleCb(() => {
      tfn.setEulerZYX(0, initialRot, 0);
      rigidBody.setWorldTransform(tfn);
      this.registerManagedPhysicsTicker(makeSpinnerTicker(), { mesh, body: rigidBody });
      return true;
    });

    this.registerDestroyLifecycleCb(() => {
      this.viz.fpCtx!.Ammo.destroy(tfn);
    });
  };

  public makeSlider = (
    mesh: THREE.Mesh,
    { getPos, despawnCond, spawnTimeSeconds, removeOnReset = true }: MakeSliderArgs
  ) => {
    const fpCtx = this.viz.fpCtx;
    if (!fpCtx) {
      throw new Error('fpCtx not initialized');
    }

    // Default spawn time is current elapsed time since last reset
    const resolvedSpawnTimeSeconds = spawnTimeSeconds ?? fpCtx.getPhysicsTime() - this.lastResetPhysicsTime;

    let rigidBody = mesh.userData.rigidBody as BtRigidBody | undefined;
    if (!rigidBody) {
      if (mesh.userData.collisionObj) {
        throw new Error('Unhandled case where slider has collision object but no rigid body');
      }

      fpCtx.addTriMesh(mesh, 'kinematic');
      rigidBody = mesh.userData.rigidBody as BtRigidBody;
    } else {
      rigidBody.setCollisionFlags(2); // btCollisionObject::CF_KINEMATIC_OBJECT
      rigidBody.setActivationState(4); // DISABLE_DEACTIVATION
    }

    const tfn = new fpCtx.Ammo.btTransform();
    tfn.setIdentity();
    tfn.setOrigin(fpCtx.btvec3(mesh.position.x, mesh.position.y, mesh.position.z));
    tfn.setEulerZYX(mesh.rotation.x, mesh.rotation.y, mesh.rotation.z);

    let disposed = false;
    let removedFromWorld = false;
    let cleanupQueued = false;
    let handle: PhysicsTickerHandle;

    const removeSliderFromWorld = () => {
      if (removedFromWorld) {
        return;
      }
      this.viz.scene.remove(mesh);
      fpCtx.removeCollisionObject(rigidBody);
      removedFromWorld = true;
    };

    const queueCleanup = () => {
      if (cleanupQueued) {
        return;
      }
      cleanupQueued = true;
      this.queuePhysicsAction(() => {
        cleanupQueued = false;
        this.unregisterManagedPhysicsTicker(handle);
        if (removeOnReset) {
          removeSliderFromWorld();
          return;
        }
        const startPos = getPos(resolvedSpawnTimeSeconds, 0);
        mesh.position.copy(startPos);
        tfn.setOrigin(fpCtx.btvec3(startPos.x, startPos.y, startPos.z));
        rigidBody.setWorldTransform(tfn);
        handle = registerSliderTicker();
      });
    };

    const registerSliderTicker = (): PhysicsTickerHandle => {
      disposed = false;
      return this.registerManagedPhysicsTicker(
        {
          tick: physicsTime => {
            if (disposed) {
              return;
            }

            const elapsed = physicsTime - this.lastResetPhysicsTime;
            if (despawnCond?.(mesh, elapsed)) {
              disposed = true;
              queueCleanup();
              return;
            }

            const secondsSinceSpawn = elapsed - resolvedSpawnTimeSeconds;
            const newPos = getPos(elapsed, secondsSinceSpawn);
            tfn.setOrigin(fpCtx.btvec3(newPos.x, newPos.y, newPos.z));
            rigidBody.setWorldTransform(tfn);
          },
        },
        { mesh, body: rigidBody }
      );
    };

    handle = registerSliderTicker();

    this.registerResetLifecycleCb(() => {
      disposed = false;
      cleanupQueued = false;
      if (removeOnReset) {
        removeSliderFromWorld();
        return false;
      }
      const startPos = getPos(resolvedSpawnTimeSeconds, 0);
      mesh.position.copy(startPos);
      tfn.setOrigin(fpCtx.btvec3(startPos.x, startPos.y, startPos.z));
      rigidBody.setWorldTransform(tfn);
      handle = registerSliderTicker();
      return true;
    });

    this.registerDestroyLifecycleCb(() => {
      this.viz.fpCtx!.Ammo.destroy(tfn);
    });
  };

  /**
   * Registers a periodic callback that will be executed at `initialTimeSeconds` and then repeatedly every
   * `intervalSeconds` after that.
   */
  public schedulePeriodic(
    callback: (invokeTimeSeconds: number) => void,
    initialTimeSeconds: number,
    intervalSeconds: number
  ): SchedulerHandle {
    return this.scheduler.schedule(callback, initialTimeSeconds, intervalSeconds);
  }

  public buildSceneConfig = (): SceneConfig => {
    return {
      spawnLocation: 'spawn',
      gravity: 30,
      player: {
        moveSpeed: { onGround: 10, inAir: 13 },
        colliderSize: { height: 2.2, radius: 0.8 },
        jumpVelocity: 12,
        oobYThreshold: -10,
        dashConfig: {
          enable: true,
          chargeConfig: { curCharges: this.curDashCharges },
          useExternalVelocity: this.useExternalVelocity,
          minDashDelaySeconds: 0,
          sfx: { play: true, name: 'dash' },
        },
        externalVelocityAirDampingFactor: new THREE.Vector3(0.32, 0.3, 0.32),
        externalVelocityGroundDampingFactor: new THREE.Vector3(0.9992, 0.9992, 0.9992),
      },
      debugPos: true,
      debugCamera: true,
      debugPlayerKinematics: true,
      locations: this.locations,
      legacyLights: false,
      customControlsEntries: [{ label: 'Reset', key: 'f', action: this.reset }],
      goBackOnLoad: false,
      sfx: {
        neededSfx: ['dash', 'dash_pickup'],
      },
    };
  };

  private destroy = () => {
    this.flushPendingPhysicsActions();
    this.unregisterAllManagedPhysicsTickers();
    this.managerTickerHandle?.unregister();
    this.managerTickerHandle = null;
    for (const cb of this.destroyLifecycleCbs) {
      cb();
    }
    this.destroyLifecycleCbs = [];
    this.resetLifecycleCbs = [];

    unmount(this.timerDisplay);
    if (this.winState?.displayComp) {
      unmount(this.winState.displayComp);
      this.winState = null;
    }
  };
}
