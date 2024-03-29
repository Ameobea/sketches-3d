import type { VizState } from '../..';
import { createStatsContainer } from '../statsContainer';
import PlayerKinematicsDebugger from './PlayerKinematicsDebugger.svelte';

export const initPlayerKinematicsDebugger = (viz: VizState, container: HTMLElement, topPx: number) => {
  const targetDisplayElem = createStatsContainer(topPx);
  container.appendChild(targetDisplayElem);

  const props = { verticalVelocity: 0, isJumping: false, isOnGround: false, isDashing: false };
  const comp = new PlayerKinematicsDebugger({ target: targetDisplayElem, props });

  viz.collisionWorldLoadedCbs.push(fpCtx => {
    viz.registerBeforeRenderCb(() => {
      props.verticalVelocity = fpCtx.playerStateGetters.getVerticalVelocity();
      props.isJumping = fpCtx.playerStateGetters.getIsJumping();
      props.isOnGround = fpCtx.playerStateGetters.getIsOnGround();
      props.isDashing = fpCtx.playerStateGetters.getIsDashing();

      comp.$set(props);
    });
  });
};
