// Placeholder: proves the `Mode` seam with a second implementor. `render_texture` outputs
// aren't readable from JS yet (`rendered_textures` is ctx-internal), so there's nothing to draw.

import type { TreeDef } from 'src/geoscript/geotoyAPIClient';
import { disposeRunObjects } from 'src/geoscript/runner/geoscriptRunner';
import type { RunResult } from 'src/geoscript/runner/runner';
import { runtimeMetric, type MenuSection, type Mode, type StatusMetric } from 'src/geotoy/modes/mode';
import type { RunStats } from 'src/geoscript/runner/runner';

export class TextureMode implements Mode {
  readonly kind = 'texture' as const;

  /** Last run's outputs, once the readback getter exists. Tracked now only so the
   *  placeholder surface can say whether a run has landed. */
  hasRun = $state(false);

  consume(result: RunResult, _tree: TreeDef, _moduleNameToNodeId: Record<string, string>) {
    disposeRunObjects(result);
    this.hasRun = true;
  }

  clearScene = () => {
    this.hasRun = false;
  };

  focus = (_nodeId: string | null) => {};

  buildViewState = () => null;

  restoreViewState = () => {};

  /** The real texture menu (resolution, wrap mode, channel, export png…) is texture-engine work. */
  sceneMenu(): MenuSection[] {
    return [{ items: [{ label: 'no texture settings yet', disabled: true, action: () => {} }] }];
  }

  /** No 3D viewport, so none of the mesh display/camera controls apply. */
  viewSections(): MenuSection[] {
    return [];
  }

  statusMetrics(stats: RunStats): StatusMetric[] {
    return [runtimeMetric(stats)];
  }
}
