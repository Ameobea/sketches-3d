/**
 * Prepends `// @ts-nocheck` to all openapi-generated files in src/api/.
 * Generated files are identified by the `tslint:disable` header the generator emits.
 * Run this after regenerating the API client.
 */
import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const TS_NOCHECK = '// @ts-nocheck\n';

function processDir(dir) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const fullPath = join(dir, entry.name);
    if (entry.isDirectory()) {
      processDir(fullPath);
    } else if (entry.name.endsWith('.ts')) {
      const content = readFileSync(fullPath, 'utf8');
      if (content.startsWith(TS_NOCHECK)) continue;
      if (!content.includes('/* tslint:disable */')) continue;
      writeFileSync(fullPath, TS_NOCHECK + content);
      console.log(`patched ${fullPath}`);
    }
  }
}

processDir('src/api');
