import type { BtKinematicCharacterController, BtVec3 } from 'src/ammojs/ammoTypes';
import type { SfxManager } from './audio/SfxManager';
import { DefaultDashConfig, type DashConfig } from './scenes';
import { mergeDeep } from './util/util';

export class DashManager {
  private sfxManager: SfxManager;
  private config: DashConfig;
  public lastDashTimeSeconds = 0;
  /**
   * `true` if the player has not touched the ground since they last dashed
   */
  private dashNeedsGroundTouch = false;
  private dashCbs: ((curTimeSeconds: number) => void)[] = [];
  private playerController: BtKinematicCharacterController;
  private btvec3: (x: number, y: number, z: number) => BtVec3;

  static mergeConfig(config: Partial<DashConfig> | undefined): DashConfig {
    if (!config) {
      return DefaultDashConfig;
    }
    return mergeDeep({ ...DefaultDashConfig }, config);
  }

  constructor(
    sfxManager: SfxManager,
    config: Partial<DashConfig> | undefined,
    playerController: BtKinematicCharacterController,
    btvec3: (x: number, y: number, z: number) => BtVec3
  ) {
    this.btvec3 = btvec3;
    this.sfxManager = sfxManager;
    this.config = DashManager.mergeConfig(config);
    this.playerController = playerController;
  }

  private dashInner(dashDir: THREE.Vector3, curTimeSeconds: number) {
    if (this.config.useExternalVelocity) {
      this.playerController.setExternalVelocity(
        this.btvec3(
          dashDir.x * this.config.dashMagnitude * 1.28,
          dashDir.y * this.config.dashMagnitude * 1.28,
          dashDir.z * this.config.dashMagnitude * 1.28
        )
      );
      this.playerController.resetFall();
    } else {
      this.playerController.jump(
        this.btvec3(
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
      this.sfxManager.playSfx(this.config.sfx.name ?? 'dash');
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
