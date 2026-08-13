#!/usr/bin/env bash
# Diff a fresh corpus capture against a baseline, resolving known wobblers against every
# recorded state (see `corpus_renders/wobble-alt/`). Usage: corpus-diff.sh <baseline> <new>
set -u
A="corpus_renders/$1"
B="corpus_renders/$2"

diff <(cd "$A" && ls ./*.png | sort) <(cd "$B" && ls ./*.png | sort) >/dev/null &&
  echo "file sets identical ($(ls "$B"/*.png | wc -l) rasters)" || echo "FILE SET MISMATCH"

real=0
for f in "$B"/*.png; do
  n=$(basename "$f")
  cmp -s "$A/$n" "$f" && continue
  matched=""
  for alt in corpus_renders/wobble-alt/"${n%.png}"*.png corpus_renders/p7-full/"$n" corpus_renders/p8-3/"$n" corpus_renders/p8-4/"$n"; do
    [ -f "$alt" ] && cmp -s "$alt" "$f" && { matched=$(basename "$alt"); break; }
  done
  if [ -n "$matched" ]; then echo "  wobble: $n (matches $matched)"; else echo "  REAL DIFF: $n"; real=$((real + 1)); fi
done
echo "unexplained raster diffs: $real"

python3 -c "
import json
print('_failures.json equal:', json.load(open('$A/_failures.json'))==json.load(open('$B/_failures.json')))"

python3 - "$2" <<'EOF'
import json, glob, os, sys
tag = sys.argv[1]
ok = True
for a, b in [('p7-eval', f'{tag}-eval'), ('p7-eval-expr', f'{tag}-eval-expr'), ('p7-eval-obj', f'{tag}-eval-obj')]:
    ns = {os.path.basename(p) for p in glob.glob(f'corpus_renders/{a}/*.eval.json')}
    m = all(json.load(open(f'corpus_renders/{a}/{n}')) == json.load(open(f'corpus_renders/{b}/{n}')) for n in ns)
    ok &= m
    print(f'{a} vs {b}: {len(ns)} -> {"MATCH" if m else "MISMATCH"}')
print('ALL EVAL ENVELOPES MATCH' if ok else 'EVAL MISMATCH')
EOF
