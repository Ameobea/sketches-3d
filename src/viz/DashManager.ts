import { DefaultDashConfig, type DashConfig } from './scenes';
import { mergeDeep } from './util/util';
import type { Viz } from '.';

export class DashManager {
  public lastDashTimeSeconds = 0;
  /**
   * `true` if the player has not touched the ground since they last dashed
   */
  private dashNeedsGroundTouch = false;
  private dashCbs: ((curTimeSeconds: number) => void)[] = [];
  private viz: Viz;

  static mergeConfig(config: Partial<DashConfig> | undefined): DashConfig {
    if (!config) {
      return DefaultDashConfig;
    }
    return mergeDeep({ ...DefaultDashConfig }, config);
  }

  constructor(viz: Viz) {
    this.viz = viz;
  }

  private get config() {
    return { ...DefaultDashConfig, ...(this.viz.sceneConf.player?.dashConfig ?? {}) };
  }

  private dashInner(dashDir: THREE.Vector3, curTimeSeconds: number) {
    if (this.config.useExternalVelocity) {
      this.viz.fpCtx!.playerController.setExternalVelocity(
        this.viz.fpCtx!.btvec3(
          dashDir.x * this.config.dashMagnitude * 1.28,
          dashDir.y * this.config.dashMagnitude * 1.28,
          dashDir.z * this.config.dashMagnitude * 1.28
        )
      );
      this.viz.fpCtx!.playerController.resetFall();
    } else {
      this.viz.fpCtx!.playerController.jump(
        this.viz.fpCtx!.btvec3(
          dashDir.x * this.config.dashMagnitude,
          dashDir.y * this.config.dashMagnitude,
          dashDir.z * this.config.dashMagnitude
        )
      );
    }
    this.lastDashTimeSeconds = curTimeSeconds;
    this.dashNeedsGroundTouch = true;

    if (this.config.chargeConfig) {
      this.config.chargeConfig.curCharges.update(n => n - 1);
    }

    for (const cb of this.dashCbs) {
      cb(curTimeSeconds);
    }
  }

  public tick(curTimeSeconds: number, onGround: boolean) {
    if (
      curTimeSeconds - this.lastDashTimeSeconds > this.config.minDashDelaySeconds &&
      this.dashNeedsGroundTouch &&
      onGround
    ) {
      this.dashNeedsGroundTouch = false;
    }
  }

  /**
   * Attempts to dash if the necessary conditions are met.  Returns `true` if the dash was actually performed.
   */
  public tryDash(curTimeSeconds: number, isFlyMode: boolean, dashDir: THREE.Vector3): boolean {
    if (!this.config.enable) {
      return false;
    }

    if (!this.viz.controlState.movementEnabled) {
      return false;
    }

    // check if not enough time since last dash
    if (curTimeSeconds - this.lastDashTimeSeconds <= this.config.minDashDelaySeconds) {
      return false;
    }

    if (this.config.chargeConfig) {
      if (this.config.chargeConfig.curCharges.current <= 0) {
        return false;
      }
    }

    if (this.dashNeedsGroundTouch && !isFlyMode) {
      return false;
    }

    this.dashInner(dashDir, curTimeSeconds);
    if (this.config.sfx?.play) {
      this.viz.sfxManager.playSfx(this.config.sfx.name ?? 'dash');
    }

    return true;
  }

  public registerDashCb(cb: (curTimeSeconds: number) => void) {
    this.dashCbs.push(cb);
  }

  public deregisterDashCb(cb: (curTimeSeconds: number) => void) {
    const ix = this.dashCbs.indexOf(cb);
    if (ix === -1) {
      throw new Error('cb not registered');
    }
    this.dashCbs.splice(ix, 1);
  }
}
