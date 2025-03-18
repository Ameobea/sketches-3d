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
import { API } from 'src/api/client';
import type { TransparentWritable } from '../util/TransparentWritable';

export interface ParkourMaterials {
  dashToken: {
    core: THREE.Material;
    ring: THREE.Material;
  };
  checkpoint: THREE.Material;
}

export class ParkourManager {
  private viz: Viz;
  private locations: { [key: string]: { pos: THREE.Vector3; rot: THREE.Vector3 } };
  private scoreThresholds: ScoreThresholds;
  private mapID: string;
  private useExternalVelocity: boolean;

  private curDashCharges: TransparentWritable<number>;
  private curRunStartTimeSeconds: number | null = null;
  private winState: { winTimeSeconds: number; displayComp: any } | null = null;

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
    this.viz.setSpawnPos(this.locations.spawn.pos, this.locations.spawn.rot);
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
  };

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
}
