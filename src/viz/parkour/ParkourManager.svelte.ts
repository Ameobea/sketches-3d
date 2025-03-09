import * as THREE from 'three';
import { mount, unmount } from 'svelte';
import type { Writable } from 'svelte/store';

import type { VizState } from 'src/viz';
import type { VizConfig } from 'src/viz/conf';
import { initDashTokens } from './DashToken';
import { initCheckpoints } from './checkpoints';
import TimerDisplay from './TimerDisplay.svelte';
import type { ScoreThresholds } from './TimeDisplay.svelte';
import TimeDisplay from './TimeDisplay.svelte';
import type { SceneConfig } from '../scenes';

export interface ParkourMaterials {
  dashToken: {
    core: THREE.Material;
    ring: THREE.Material;
  };
  checkpoint: THREE.Material;
}

export class ParkourManager {
  private viz: VizState;
  private locations: { [key: string]: { pos: THREE.Vector3; rot: THREE.Vector3 } };
  private scoreThresholds: ScoreThresholds;

  private curDashCharges: Writable<number>;
  private curRunStartTimeSeconds: number | null = null;
  private winState: { winTimeSeconds: number; displayComp: any } | null = null;

  private resetDashes: () => void;
  private resetCheckpoints: () => void;

  constructor(
    viz: VizState,
    loadedWorld: THREE.Group,
    vizConf: VizConfig,
    locations: { [key: string]: { pos: THREE.Vector3; rot: THREE.Vector3 } },
    scoreThresholds: ScoreThresholds,
    materials: ParkourMaterials
  ) {
    this.viz = viz;
    this.locations = locations;
    this.scoreThresholds = scoreThresholds;

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

    viz.collisionWorldLoadedCbs.push(fpCtx =>
      fpCtx.registerJumpCb(curTimeSeconds => {
        if (this.curRunStartTimeSeconds === null) {
          this.curRunStartTimeSeconds = curTimeSeconds;
        }
      })
    );
  }

  private initTimer = () => {
    const target = document.createElement('div');
    document.body.appendChild(target);
    const timerDisplayProps = $state({ curTime: 0 });
    const _timerDisplay = mount(TimerDisplay, { target, props: timerDisplayProps });
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

  private reset = () => {
    this.resetDashes();
    this.resetCheckpoints();
    this.viz.fpCtx!.teleportPlayer(this.locations.spawn.pos, this.locations.spawn.rot);
    this.viz.fpCtx!.reset();
    this.curRunStartTimeSeconds = null;
    if (this.winState?.displayComp) {
      unmount(this.winState.displayComp);
    }
    this.winState = null;
    this.viz.fpCtx!.setSpawnPos(this.locations.spawn.pos, this.locations.spawn.rot);
  };

  private onWin = () => {
    const curTimeSeconds = this.viz.clock.getElapsedTime();

    const target = document.createElement('div');
    document.body.appendChild(target);
    const time = curTimeSeconds - (this.curRunStartTimeSeconds ?? 0);
    const displayComp = mount(TimeDisplay, {
      target,
      props: { scoreThresholds: this.scoreThresholds, time },
    });
    this.winState = { winTimeSeconds: curTimeSeconds, displayComp };

    this.viz.fpCtx!.setSpawnPos(this.locations.spawn.pos, this.locations.spawn.rot);
  };

  public buildSceneConfig = (): SceneConfig => {
    return {
      spawnLocation: 'spawn',
      gravity: 30,
      player: {
        moveSpeed: { onGround: 10, inAir: 13 },
        colliderCapsuleSize: { height: 2.2, radius: 0.8 },
        jumpVelocity: 12,
        oobYThreshold: -10,
        dashConfig: {
          enable: true,
          chargeConfig: { curCharges: this.curDashCharges },
          useExternalVelocity: true,
          minDashDelaySeconds: 0,
        },
        externalVelocityAirDampingFactor: new THREE.Vector3(0.32, 0.3, 0.32),
        externalVelocityGroundDampingFactor: new THREE.Vector3(0.9992, 0.9992, 0.9992),
      },
      debugPos: true,
      debugPlayerKinematics: true,
      locations: this.locations,
      legacyLights: false,
      customControlsEntries: [{ label: 'Reset', key: 'f', action: this.reset }],
    };
  };
}
