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
import { API, checkLogin, LoggedInUserID, MetricsAPI } from 'src/api/client';
import type { TransparentWritable } from '../util/TransparentWritable';
import type { BtRigidBody } from 'src/ammojs/ammoTypes';
import { Scheduler, type SchedulerHandle } from '../bulletHell/Scheduler';

export interface ParkourMaterials {
  dashToken: {
    core: THREE.Material;
    ring: THREE.Material;
  };
  checkpoint: THREE.Material;
}

interface Ticker {
  /**
   * If `false` is returned, the ticker will be removed and never called again.
   *
   * Returning `true`, `undefined`, or any other value will keep the ticker alive.
   */
  tick: (curTimeSeconds: number, tDiffSeconds: number) => boolean | void;
  /**
   * If `false` is returned, the ticker will be removed and never called again.
   *
   * Returning `true`, `undefined`, or any other value will keep the ticker alive.
   */
  reset: () => boolean | void;
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
  private locations: { [key: string]: { pos: THREE.Vector3; rot: THREE.Vector3 } };
  private scoreThresholds: ScoreThresholds;
  private mapID: string;
  private useExternalVelocity: boolean;

  private curDashCharges: TransparentWritable<number>;
  private lastResetTime: number = 0;
  private curRunStartTimeSeconds: number | null = null;
  private winState: { winTimeSeconds: number; displayComp: any } | null = null;
  private tickers: Ticker[] = [];
  private onStartCbs: (() => void)[] = [];
  private timerDisplay!: any;
  private scheduler: Scheduler = new Scheduler();

  private resetDashes: () => void;
  private resetCheckpoints: () => void;

  constructor(
    viz: Viz,
    loadedWorld: THREE.Group,
    vizConf: VizConfig,
    locations: { [key: string]: { pos: THREE.Vector3; rot: THREE.Vector3 } },
    scoreThresholds: ScoreThresholds,
    materials: ParkourMaterials,
    mapID: string,
    useExternalVelocity: boolean
  ) {
    this.viz = viz;
    this.locations = locations;
    this.scoreThresholds = scoreThresholds;
    this.mapID = mapID;
    this.useExternalVelocity = useExternalVelocity;

    const {
      ctx: dashTokensCtx,
      dashCharges: curDashCharges,
      reset: resetDashes,
    } = initDashTokens(viz, loadedWorld, materials.dashToken.core, materials.dashToken.ring);
    this.curDashCharges = curDashCharges;
    this.resetCheckpoints = initCheckpoints(
      viz,
      loadedWorld,
      materials.checkpoint,
      dashTokensCtx,
      curDashCharges,
      this.onWin
    );
    this.resetDashes = resetDashes;

    this.initTimer();

    viz.collisionWorldLoadedCbs.push(fpCtx => {
      fpCtx.registerJumpCb(curTimeSeconds => {
        if (this.curRunStartTimeSeconds === null) {
          this.curRunStartTimeSeconds = curTimeSeconds;
        }
      });

      let didTick = false;
      viz.registerBeforeRenderCb((curTimeSeconds, tDiffSeconds) => {
        if (!didTick) {
          didTick = true;
          for (const cb of this.onStartCbs) {
            cb();
          }
        }

        const elapsedTimeSeconds = curTimeSeconds - this.lastResetTime;
        let tickerIx = 0;
        while (tickerIx < this.tickers.length) {
          const ticker = this.tickers[tickerIx];
          const shouldCancel = ticker.tick(elapsedTimeSeconds, tDiffSeconds) === false;
          if (shouldCancel) {
            this.tickers[tickerIx] = this.tickers[this.tickers.length - 1];
            this.tickers.pop();
          } else {
            tickerIx += 1;
          }
        }

        this.scheduler.tick(elapsedTimeSeconds);
      });
    });

    viz.registerDestroyedCb(this.destroy);
  }

  private initTimer = () => {
    const target = document.createElement('div');
    document.body.appendChild(target);
    const timerDisplayProps = $state({ curTime: 0 });
    this.timerDisplay = mount(TimerDisplay, { target, props: timerDisplayProps });
    this.viz.registerAfterRenderCb(curTimeSeconds => {
      const elapsedSeconds = (() => {
        if (this.curRunStartTimeSeconds === null) {
          return 0;
        }

        if (this.winState) {
          return this.winState.winTimeSeconds - this.curRunStartTimeSeconds;
        }

        return curTimeSeconds - this.curRunStartTimeSeconds;
      })();
      timerDisplayProps.curTime = elapsedSeconds;
    });
  };

  public registerOnStartCb = (cb: () => void) => {
    this.onStartCbs.push(cb);
  };

  private reset = () => {
    const elapsedTimeSeconds = this.viz.clock.getElapsedTime() - (this.curRunStartTimeSeconds ?? 0);
    this.resetDashes();
    this.resetCheckpoints();
    this.viz.fpCtx!.teleportPlayer(this.locations.spawn.pos, this.locations.spawn.rot);
    this.viz.fpCtx!.reset();
    const wasStarted = this.curRunStartTimeSeconds !== null;
    this.curRunStartTimeSeconds = null;
    this.lastResetTime = this.viz.clock.getElapsedTime();
    if (this.winState?.displayComp) {
      unmount(this.winState.displayComp);
    }
    const wasWin = !!this.winState;
    this.winState = null;
    this.viz.setSpawnPos(this.locations.spawn.pos, this.locations.spawn.rot);

    const newTickers: Ticker[] = [];
    for (const ticker of this.tickers) {
      const retain = ticker.reset();
      if (retain !== false) {
        newTickers.push(ticker);
      }
    }
    this.tickers = newTickers;
    this.scheduler.clear();

    for (const cb of this.onStartCbs) {
      cb();
    }

    if (!wasWin && wasStarted) {
      console.log({ elapsedTimeSeconds });
      (async () => {
        if (!LoggedInUserID.current) {
          await checkLogin();
        }
        MetricsAPI.recordRestart(this.mapID, LoggedInUserID.current, elapsedTimeSeconds);
      })();
    }
  };

  private onWin = () => {
    const curTimeSeconds = this.viz.clock.getElapsedTime();

    const target = document.createElement('div');
    document.body.appendChild(target);
    const time = curTimeSeconds - (this.curRunStartTimeSeconds ?? 0);
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
    this.winState = { winTimeSeconds: curTimeSeconds, displayComp };

    this.viz.setSpawnPos(this.locations.spawn.pos, this.locations.spawn.rot);

    API.addPlay({ mapId: this.mapID, timeLength: time })
      .then(res => {
        timeDisplayProps.userPlayID = res.id ?? null;
        timeDisplayProps.userID = res.playerId ?? null;
      })
      .catch(() => {});

    (async () => {
      if (!LoggedInUserID.current) {
        await checkLogin();
      }
      MetricsAPI.recordPlayCompletion(this.mapID, true, time, LoggedInUserID.current);
    })();
  };

  public addTicker = (ticker: Ticker) => {
    this.tickers.push(ticker);
  };

  public makeSpinner = (mesh: THREE.Mesh, rpm: number) => {
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
    this.addTicker({
      tick: (curTimeSeconds, _tDiffSeconds) => {
        mesh.rotation.y = initialRot - rps * curTimeSeconds * Math.PI * 2;
        tfn.setEulerZYX(0, mesh.rotation.y, 0);
        rigidBody.getMotionState()!.setWorldTransform(tfn);
      },
      reset: () => {
        mesh.rotation.y = initialRot;
      },
    });

    this.viz.registerDestroyedCb(() => {
      this.viz.fpCtx!.Ammo.destroy(tfn);
    });
  };

  public makeSlider = (
    mesh: THREE.Mesh,
    {
      getPos,
      despawnCond,
      spawnTimeSeconds = this.viz.clock.getElapsedTime(),
      removeOnReset = true,
    }: MakeSliderArgs
  ) => {
    const fpCtx = this.viz.fpCtx;
    if (!fpCtx) {
      throw new Error('fpCtx not initialized');
    }

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

    const reset = () => {
      if (removeOnReset) {
        this.viz.scene.remove(mesh);
        fpCtx.removeCollisionObject(rigidBody);
        return false;
      } else {
        const startPos = getPos(spawnTimeSeconds, 0);
        mesh.position.copy(startPos);
        tfn.setOrigin(fpCtx.btvec3(startPos.x, startPos.y, startPos.z));
        rigidBody.getMotionState()!.setWorldTransform(tfn);
        return true;
      }
    };

    const ticker = {
      tick: (curTimeSeconds: number) => {
        if (despawnCond?.(mesh, curTimeSeconds)) {
          reset();
          return !removeOnReset;
        }

        const secondsSinceSpawn = curTimeSeconds - spawnTimeSeconds;
        const newPos = getPos(curTimeSeconds, secondsSinceSpawn);
        mesh.position.copy(newPos);
        tfn.setOrigin(fpCtx.btvec3(newPos.x, newPos.y, newPos.z));
        rigidBody.getMotionState()!.setWorldTransform(tfn);
      },
      reset,
    };
    this.addTicker(ticker);

    this.viz.registerDestroyedCb(() => {
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
    unmount(this.timerDisplay);
    if (this.winState?.displayComp) {
      unmount(this.winState.displayComp);
      this.winState = null;
    }
  };
}
