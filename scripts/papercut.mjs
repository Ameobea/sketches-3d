import { appendEntry } from './papercut-lib.mjs';

const argv = process.argv.slice(2);
let model = '';
const rest = [];
for (let i = 0; i < argv.length; i++) {
  const a = argv[i];
  if (a === '-m' || a === '--model') model = argv[++i] ?? '';
  else rest.push(a);
}
const message = rest.join(' ').trim();

if (!model || !message) {
  console.error('usage: yarn papercut -m <model> "what you were doing → what got in the way"');
  process.exit(1);
}

appendEntry(model, message);
console.log('logged → PAPERCUTS.md');
