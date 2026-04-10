import { type Plugin } from 'vite';
import fs from 'node:fs';
import path from 'node:path';
const VIRTUAL_MODULE_ID = 'virtual:behaviors';
const RESOLVED_ID = '\0virtual:behaviors';

/**
 * Vite plugin that generates a virtual module re-exporting all behavior functions.
 *
 * It globs two locations:
 * - `src/viz/behaviors/*.ts` — shared behaviors available to all levels
 * - `src/levels/<name>/behaviors/*.ts` — level-local behaviors, namespaced as `<name>__<fn>`
 *
 * The virtual module exports a flat record: `Record<string, BehaviorFn>`.
 * Level-local behaviors shadow shared ones when resolved for their own level.
 */
export function behaviorsPlugin(): Plugin {
  // Resolve from the project root (vite.config.js location), not __dirname
  let projectRoot: string;

  return {
    name: 'behaviors-virtual-module',

    configResolved(config) {
      projectRoot = config.root;
    },

    resolveId(id) {
      if (id === VIRTUAL_MODULE_ID) return RESOLVED_ID;
    },

    load(id) {
      if (id !== RESOLVED_ID) return;

      const srcRoot = path.join(projectRoot, 'src');
      const imports: string[] = [];
      const entries: string[] = [];
      let importIndex = 0;

      // Shared behaviors: src/viz/behaviors/*.ts
      const sharedDir = path.join(srcRoot, 'viz/behaviors');
      if (fs.existsSync(sharedDir)) {
        for (const file of fs.readdirSync(sharedDir)) {
          if (!file.endsWith('.ts') || file.endsWith('.d.ts')) continue;
          const name = file.replace(/\.ts$/, '');
          const alias = `_s${importIndex++}`;
          imports.push(`import ${alias} from 'src/viz/behaviors/${file}';`);
          entries.push(`  '${name}': ${alias},`);
        }
      }

      // Level-local behaviors: src/levels/*/behaviors/*.ts
      const levelsDir = path.join(srcRoot, 'levels');
      if (fs.existsSync(levelsDir)) {
        for (const levelName of fs.readdirSync(levelsDir)) {
          const behaviorDir = path.join(levelsDir, levelName, 'behaviors');
          if (!fs.existsSync(behaviorDir) || !fs.statSync(behaviorDir).isDirectory()) continue;
          for (const file of fs.readdirSync(behaviorDir)) {
            if (!file.endsWith('.ts') || file.endsWith('.d.ts')) continue;
            const fnName = file.replace(/\.ts$/, '');
            const alias = `_l${importIndex++}`;
            imports.push(`import ${alias} from 'src/levels/${levelName}/behaviors/${file}';`);
            entries.push(`  '${levelName}__${fnName}': ${alias},`);
          }
        }
      }

      return `${imports.join('\n')}\n\nexport default {\n${entries.join('\n')}\n};\n`;
    },

    handleHotUpdate({ file, server }) {
      const srcRoot = path.join(projectRoot, 'src');
      const rel = path.relative(srcRoot, file);
      if (
        (rel.startsWith('viz/behaviors/') || /^levels\/[^/]+\/behaviors\//.test(rel)) &&
        file.endsWith('.ts')
      ) {
        const mod = server.moduleGraph.getModuleById(RESOLVED_ID);
        if (mod) {
          server.moduleGraph.invalidateModule(mod);
          return [mod];
        }
      }
    },
  };
}
