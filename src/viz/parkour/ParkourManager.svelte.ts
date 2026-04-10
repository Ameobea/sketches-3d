import * as THREE from 'three';
import { mount, unmount } from 'svelte';

import type { Viz } from 'src/viz';
import type { VizConfig } from 'src/viz/conf';
import { initDashTokens } from './DashToken';
import { initCheckpoints } from './checkpoints';
import TimerDisplay from './TimerDisplay.svelte';
import TimeDisplay from './TimeDisplay.svelte';
import type { ScoreThresholds } from './timeDisplayTypes';
import type { SceneConfig } from '../scenes';
import { MetricsAPI } from 'src/api/client';
import type { PlayResponse } from 'src/api/models/PlayResponse';
import { mergeDeep, type DeepPartial } from '../util/util';
import { rwritable, type TransparentWritable } from '../util/TransparentWritable';
import { SceneRuntime } from '../sceneRuntime';
import { RecorderEventType } from '../flightRecorder';

export interface ParkourMaterials {
  dashToken: {
    core: THREE.Material;
    ring: THREE.Material;
  };
  /**
   * Material for checkpoint / win-zone meshes.
   * Optional when `checkpointMeshes` is provided to `setMaterials` and the meshes
   * already have their material assigned by the level def system.
   */
  checkpoint?: THREE.Material | (() => THREE.Material);
}

export class ParkourManager {
  private viz: Viz;
  public loadedWorld: THREE.Group;
  public readonly runtime: SceneRuntime;
  private locations: { [key: string]: { pos: THREE.Vector3; rot: THREE.Vector3 } };
  private scoreThresholds: ScoreThresholds;
  private mapID: string;
  private useExternalVelocity: boolean;
  private sceneConfigOverrides: DeepPartial<SceneConfig>;

  private curDashCharges: TransparentWritable<number> = rwritable(0);
  private curRunStartTimeSeconds: number | null = null;
  private winState: { winTimeSeconds: number; displayComp: any; replayBlob: Uint8Array | null } | null = null;
  private timerDisplay!: any;

  private syncDashTokensFromController: () => void = () => {
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
    useExternalVelocity: boolean,
    sceneConfigOverrides: DeepPartial<SceneConfig> = {}
  ) {
    this.viz = viz;
    this.loadedWorld = loadedWorld;
    this.locations = locations;
    this.scoreThresholds = scoreThresholds;
    this.mapID = mapID;
    this.useExternalVelocity = useExternalVelocity;
    this.sceneConfigOverrides = sceneConfigOverrides;

    this.runtime = new SceneRuntime(viz);

    if (materials) {
      this.setMaterials(materials);
    }

    this.initTimer();

    viz.collisionWorldLoadedCbs.push(fpCtx => {
      fpCtx.registerJumpCb(() => {
        if (this.curRunStartTimeSeconds === null) {
          this.curRunStartTimeSeconds = fpCtx.getPhysicsTime();
          if (!fpCtx.isReplayActive) {
            fpCtx.flightRecorder.setMetadataString(
              'third_person_xray',
              this.viz.vizConfig.current.gameplay.thirdPersonXray ? 'true' : 'false'
            );
            fpCtx.flightRecorder.recordEvent(RecorderEventType.RunStart);
          }
        }
      });
    });

    // Register our reset as a callback on the runtime so it runs when the runtime resets
    this.runtime.registerResetCb(() => this.onRuntimeReset());
    this.runtime.registerDestroyCb(() => this.onRuntimeDestroy());
  }

  public setMaterials = (materials: ParkourMaterials, opts: { checkpointMeshes?: THREE.Mesh[] } = {}) => {
    const { syncFromController } = initDashTokens(
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
      syncFromController,
      this.onWin,
      opts.checkpointMeshes
    );
    this.syncDashTokensFromController = syncFromController;
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

  public reset = () => {
    const fpCtx = this.viz.fpCtx!;
    const elapsedTimeSeconds = fpCtx.getPhysicsTime() - (this.curRunStartTimeSeconds ?? 0);
    const wasStarted = this.curRunStartTimeSeconds !== null;

    this.resetCheckpoints();
    fpCtx.teleportPlayer(this.locations.spawn.pos, this.locations.spawn.rot);
    fpCtx.reset();
    fpCtx.resetDashStateForNewRun();
    fpCtx.saveDashCheckpointState();
    this.syncDashTokensFromController();
    fpCtx.assertInitialState();

    this.curRunStartTimeSeconds = null;

    // Reset flight recorder for a clean recording from t=0
    fpCtx.resetRecorderForNewRun();
    const sp = this.locations.spawn.pos;
    const sr = this.locations.spawn.rot;
    fpCtx.flightRecorder.setMetadataString('spawn_pos', `${sp.x},${sp.y},${sp.z}`);
    fpCtx.flightRecorder.setMetadataString('spawn_rot', `${sr?.x ?? 0},${sr?.y ?? 0},${sr?.z ?? 0}`);
    fpCtx.flightRecorder.recordEvent(RecorderEventType.Teleport, [
      sp.x,
      sp.y,
      sp.z,
      sr?.x ?? 0,
      sr?.y ?? 0,
      sr?.z ?? 0,
    ]);
    if (this.winState?.displayComp) {
      unmount(this.winState.displayComp);
    }
    const wasWin = !!this.winState;
    this.winState = null;
    this.viz.setSpawnPos(this.locations.spawn.pos, this.locations.spawn.rot);

    // Delegate entity/ticker/scheduler reset to the runtime
    this.runtime.reset();

    if (!wasWin && wasStarted) {
      MetricsAPI.recordRestart(this.mapID, elapsedTimeSeconds);
    }
  };

  /** Called by the runtime's reset — handles any additional parkour-specific reset logic. */
  private onRuntimeReset = () => {
    // Currently all parkour reset logic lives in this.reset() which calls runtime.reset().
    // This callback exists for any future parkour-specific logic that should run
    // as part of the runtime's reset cycle.
  };

  private onWin = () => {
    const fpCtx = this.viz.fpCtx!;
    if (this.winState) {
      return;
    }

    const physicsTime = fpCtx.getPhysicsTime();
    const time = physicsTime - (this.curRunStartTimeSeconds ?? 0);

    let replayBlob: Uint8Array | null = null;
    let displayComp: any = null;

    if (!fpCtx.isReplayActive && !fpCtx.flightRecorder.isExternalReplay) {
      const recorder = fpCtx.flightRecorder;
      recorder.recordEvent(RecorderEventType.RunEnd);
      recorder.setMetadataString('map_id', this.mapID);
      recorder.setMetadataString('timestamp', Date.now().toString());
      replayBlob = recorder.serialize();

      const target = document.createElement('div');
      document.body.appendChild(target);
      const timeDisplayProps = $state({
        scoreThresholds: this.scoreThresholds,
        time,
        mapID: this.mapID,
        userPlayID: null as string | null,
        userID: null as string | null,
      });
      displayComp = mount(TimeDisplay, {
        target,
        props: timeDisplayProps,
      });

      this.viz.setSpawnPos(this.locations.spawn.pos, this.locations.spawn.rot);

      const formData = new FormData();
      formData.append('mapId', this.mapID);
      formData.append('timeLength', time.toString());
      if (replayBlob) {
        formData.append('replay', new Blob([replayBlob as unknown as BlobPart]), 'replay.frec');
      }
      fetch('/api/play', { method: 'POST', credentials: 'include', body: formData })
        .then(res => res.json() as Promise<PlayResponse>)
        .then(res => {
          timeDisplayProps.userPlayID = res.id ?? null;
          timeDisplayProps.userID = res.playerId ?? null;
        })
        .catch(() => {});

      MetricsAPI.recordPlayCompletion(this.mapID, true, time);
    }

    this.winState = { winTimeSeconds: physicsTime, displayComp, replayBlob };
  };

  public buildSceneConfig = (): SceneConfig => {
    const playerRadius = 0.8;
    const defaultSceneConfig: SceneConfig = {
      spawnLocation: 'spawn',
      gravity: 30,
      player: {
        moveSpeed: { onGround: 10, inAir: 13 },
        colliderSize: { height: 2.2, radius: playerRadius },
        jumpVelocity: 12,
        oobYThreshold: -10,
        dashConfig: {
          enable: true,
          chargeConfig: { curCharges: this.curDashCharges },
          useExternalVelocity: this.useExternalVelocity,
          minDashDelaySeconds: 0,
          sfx: { play: true, name: 'dash' },
        },
        playerShadow: { radius: playerRadius, intensity: 0.85 },
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
    return mergeDeep(defaultSceneConfig, this.sceneConfigOverrides);
  };

  private onRuntimeDestroy = () => {
    unmount(this.timerDisplay);
    if (this.winState?.displayComp) {
      unmount(this.winState.displayComp);
      this.winState = null;
    }
  };
}
