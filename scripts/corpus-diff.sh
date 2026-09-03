#!/usr/bin/env bash
# Diff a corpus capture against a baseline: rasters byte-for-byte (a mismatch is excused when it
# matches any alternate in corpus_renders/wobble-alt/<id>*.png), `_failures.json`, and whichever
# eval captures exist for both tags (<tag>-eval, <tag>-eval-expr, <tag>-eval-obj).
# Usage: corpus-diff.sh <baseline-tag> <candidate-tag>
set -u
A="corpus_renders/$1"
B="corpus_renders/$2"
[ -d "$A" ] && [ -d "$B" ] || { echo "missing $A or $B" >&2; exit 2; }

diff <(cd "$A" && ls ./*.png | sort) <(cd "$B" && ls ./*.png | sort) >/dev/null &&
  echo "file sets identical ($(ls "$B"/*.png | wc -l | tr -d ' ') rasters)" || echo "FILE SET MISMATCH"

real=0
for f in "$B"/*.png; do
  n=$(basename "$f")
  cmp -s "$A/$n" "$f" && continue
  matched=""
  for alt in corpus_renders/wobble-alt/"${n%.png}"*.png; do
    [ -f "$alt" ] && cmp -s "$alt" "$f" && { matched=$(basename "$alt"); break; }
  done
  if [ -n "$matched" ]; then echo "  wobble: $n (matches $matched)"; continue; fi
  # GPU jitter: a handful of pixels off by a little. Anything larger is a real diff.
  stats=$(cd geoscript_backend/thumbnail_generator && node -e '
    const s=require("sharp");(async()=>{const[a,b]=await Promise.all(process.argv.slice(1).map(p=>s(p).raw().toBuffer()));
    let n=0,mx=0;for(let i=0;i<a.length;i+=3){const d=Math.max(Math.abs(a[i]-b[i]),Math.abs(a[i+1]-b[i+1]),Math.abs(a[i+2]-b[i+2]));if(d){n++;if(d>mx)mx=d;}}
    console.log((100*n/(a.length/3)).toFixed(3), mx);})()' "$OLDPWD/$A/$n" "$OLDPWD/$f")
  pct=${stats%% *}
  if awk -v p="$pct" 'BEGIN{exit !(p <= 0.1)}'; then echo "  jitter: $n (${pct}% px, max delta ${stats##* })"; else echo "  REAL DIFF: $n (${pct}% px, max delta ${stats##* })"; real=$((real + 1)); fi
done
echo "unexplained raster diffs: $real"

python3 - "$1" "$2" <<'PY'
import json, glob, os, sys
a, b = sys.argv[1:3]
print('_failures.json equal:', json.load(open(f'corpus_renders/{a}/_failures.json')) == json.load(open(f'corpus_renders/{b}/_failures.json')))
ok = True
for suffix in ('eval', 'eval-expr', 'eval-obj'):
    da, db = f'corpus_renders/{a}-{suffix}', f'corpus_renders/{b}-{suffix}'
    if not (os.path.isdir(da) and os.path.isdir(db)):
        continue
    names = {os.path.basename(p) for p in glob.glob(f'{da}/*.eval.json')}
    bad = [n for n in sorted(names) if not os.path.exists(f'{db}/{n}') or json.load(open(f'{da}/{n}')) != json.load(open(f'{db}/{n}'))]
    ok &= not bad
    print(f'{suffix}: {len(names)} envelopes -> ' + ('MATCH' if not bad else 'MISMATCH ' + ' '.join(bad)))
print('ALL EVAL ENVELOPES MATCH' if ok else 'EVAL MISMATCH')
PY
