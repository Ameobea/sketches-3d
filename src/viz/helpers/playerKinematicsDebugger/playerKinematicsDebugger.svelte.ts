import { mount } from 'svelte';

import type { Viz } from '../..';
import { createStatsContainer } from '../statsContainer';
import PlayerKinematicsDebugger from './PlayerKinematicsDebugger.svelte';

/**
 * @returns the Svelte component that was mounted
 */
export const initPlayerKinematicsDebugger = (viz: Viz, container: HTMLElement, topPx: number) => {
  const targetDisplayElem = createStatsContainer(topPx);
  container.appendChild(targetDisplayElem);

  const props = $state({
    verticalVelocity: 0,
    verticalOffset: 0,
    isJumping: false,
    jumpAxis: [0, 0, 0] as [number, number, number],
    externalVelocity: [0, 0, 0] as [number, number, number],
    isOnGround: false,
    isDashing: false,
  });
  const comp = mount(PlayerKinematicsDebugger, { target: targetDisplayElem, props });

  viz.collisionWorldLoadedCbs.push(fpCtx => {
    viz.registerBeforeRenderCb(() => {
      props.verticalVelocity = fpCtx.playerStateGetters.getVerticalVelocity();
      props.verticalOffset = fpCtx.playerStateGetters.getVerticalOffset();
      props.isJumping = fpCtx.playerStateGetters.getIsJumping();
      props.jumpAxis = fpCtx.playerStateGetters.getJumpAxis();
      props.externalVelocity = fpCtx.playerStateGetters.getExternalVelocity();
      props.isOnGround = fpCtx.playerStateGetters.getIsOnGround();
      props.isDashing = fpCtx.playerStateGetters.getIsDashing();
    });
  });

  return comp;
};
