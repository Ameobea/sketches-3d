#!/usr/bin/env python3
"""Compare two capture dirs: FLIP + SSIM + 32x32 per-tile maxima, exact sidecar diffs, A|B|FLIP triptychs.

Exit status 1 if any image is flagged. Per-image thresholds are max(default, FLOOR_MULT x the metric
measured in a `--floor` summary from a same-build A-vs-A' run); renderer.info counts that already drifted in the
floor run are tolerated up to that drift (program counts depend on async compile order in some scenes).
"""
import argparse, json, pathlib, re, sys
import numpy as np
from PIL import Image
import flip_evaluator as flip

TILE = 32
DEFAULTS = {'flipMean': 0.003, 'flipMaxTile': 0.02, 'ssimMinTile': 0.98, 'boostFlipMaxTile': 0.05}
DARK_LUMA = 0.05
FLOOR_MULT = 3
INFO_KEYS = ('programs', 'calls', 'triangles', 'points', 'lines', 'geometries', 'textures')

ap = argparse.ArgumentParser()
ap.add_argument('a', type=pathlib.Path)
ap.add_argument('b', type=pathlib.Path)
ap.add_argument('--out', type=pathlib.Path, help='summary.json + triptychs (default: <b>/diff-vs-<a>)')
ap.add_argument('--floor', type=pathlib.Path, help='summary.json of a noise-floor run, for per-image thresholds')
ap.add_argument('--all-heatmaps', action='store_true', help='write triptychs for unflagged images too')
args = ap.parse_args()
out = args.out or args.b / f'diff-vs-{args.a.name}'
out.mkdir(parents=True, exist_ok=True)
floor = json.loads(args.floor.read_text())['results'] if args.floor else {}


def load(p): return np.asarray(Image.open(p).convert('RGB'), dtype=np.float32) / 255


def tiles(x):
    h, w = (x.shape[0] // TILE) * TILE, (x.shape[1] // TILE) * TILE
    return x[:h, :w].reshape(h // TILE, TILE, w // TILE, TILE).mean(axis=(1, 3))


def ssim(a, b):
    from numpy.lib.stride_tricks import sliding_window_view as sw
    ga, gb = a @ [.299, .587, .114], b @ [.299, .587, .114]
    m = lambda x: sw(x, (7, 7)).mean(axis=(-1, -2))
    ma, mb = m(ga), m(gb)
    va, vb, cov = m(ga * ga) - ma**2, m(gb * gb) - mb**2, m(ga * gb) - ma * mb
    c1, c2 = .01**2, .03**2
    s = ((2 * ma * mb + c1) * (2 * cov + c2)) / ((ma**2 + mb**2 + c1) * (va + vb + c2))
    return float(s.mean()), float(tiles(s).min())


def thresholds(name):
    f = floor.get(name, {})
    return {
        'flipMean': max(DEFAULTS['flipMean'], FLOOR_MULT * f.get('flipMean', 0)),
        'flipMaxTile': max(DEFAULTS['flipMaxTile'], FLOOR_MULT * f.get('flipMaxTile', 0)),
        'ssimMinTile': min(DEFAULTS['ssimMinTile'], 1 - FLOOR_MULT * (1 - f.get('ssimMinTile', 1))),
        'boostFlipMaxTile': max(DEFAULTS['boostFlipMaxTile'], FLOOR_MULT * f.get('boostFlipMaxTile', 0)),
    }


norm = lambda s: re.sub(r'0x[0-9a-fA-F]+|\d+(\.\d+)?', '#', s)


def info_tol(name, k):
    a, b = floor.get(name, {}).get('sidecar', {}).get('info', {}).get(k, [0, 0])
    return abs((a or 0) - (b or 0))


def sidecar_diff(pa, pb):
    try:
        sa, sb = (json.loads(p.with_suffix('.json').read_text()) for p in (pa, pb))
    except FileNotFoundError:
        return {}
    d = {}
    if sa.get('camera') != sb.get('camera'): d['camera'] = [sa.get('camera'), sb.get('camera')]
    ia, ib = sa.get('info', {}), sb.get('info', {})
    tol = lambda k: info_tol(pa.stem, k)
    if info := {k: [ia.get(k), ib.get(k)] for k in INFO_KEYS if abs((ia.get(k) or 0) - (ib.get(k) or 0)) > tol(k) or (k in ia) != (k in ib)}:
        d['info'] = info
    if sa.get('readyError') != sb.get('readyError'): d['readyError'] = [sa.get('readyError'), sb.get('readyError')]
    ca, cb = ({f"{m['type']}: {norm(m['text'])}" for m in s.get('console', [])} for s in (sa, sb))
    if ca != cb: d['console'] = {'removed': sorted(ca - cb), 'added': sorted(cb - ca)}
    return d


results, flagged = {}, []
names = sorted(p.name for p in args.a.glob('*.png'))
for extra in sorted({p.name for p in args.b.glob('*.png')} - set(names)):
    results[extra] = {'onlyInB': True}
for name in names:
    key = name[:-4]
    pa, pb = args.a / name, args.b / name
    if not pb.exists():
        results[key] = {'missingInB': True}; flagged.append(key); continue
    a, b = load(pa), load(pb)
    if a.shape != b.shape:
        results[key] = {'shapeMismatch': [a.shape, b.shape]}; flagged.append(key); continue
    d = np.abs(a - b).max(axis=2)
    fmap, fmean, _ = flip.evaluate(a, b, 'LDR', applyMagma=False)
    fmap = fmap.squeeze()
    s_mean, s_tile = ssim(a, b)
    r = dict(
        mad=float(d.mean() * 255), p99=float(np.percentile(d, 99) * 255),
        maxTileMad=float(tiles(d).max() * 255), pctGt8=float((d > 8 / 255).mean() * 100),
        flipMean=float(fmean), flipP99=float(np.percentile(fmap, 99)), flipMaxTile=float(tiles(fmap).max()),
        ssim=s_mean, ssimMinTile=s_tile, lumaMeanA=float((a @ [.299, .587, .114]).mean()),
    )
    # 8-bit near-black frames hide even structural changes (a dead light) from FLIP; re-check under an exposure gain
    if r['lumaMeanA'] < DARK_LUMA:
        g = min(16., .5 / max(float(np.percentile(a @ [.299, .587, .114], 99)), 1 / 255))
        bm, _, _ = flip.evaluate(np.clip(a * g, 0, 1), np.clip(b * g, 0, 1), 'LDR', applyMagma=False)
        r['boostGain'], r['boostFlipMaxTile'] = g, float(tiles(bm.squeeze()).max())
    t = thresholds(key)
    over = [k for k in ('flipMean', 'flipMaxTile', 'boostFlipMaxTile') if r.get(k, 0) > t[k]] + (['ssimMinTile'] if r['ssimMinTile'] < t['ssimMinTile'] else [])
    if sd := sidecar_diff(pa, pb): r['sidecar'] = sd
    if over or sd: r['flagged'] = over + (['sidecar:' + ','.join(sd)] if sd else []); flagged.append(key)
    results[key] = r
    if over or args.all_heatmaps:
        heat = (np.clip(fmap * 4, 0, 1) * 255).astype(np.uint8)
        side = np.concatenate([(a * 255).astype(np.uint8), (b * 255).astype(np.uint8), np.repeat(heat[..., None], 3, 2)], 1)
        Image.fromarray(side).save(out / name)

summary = {'a': str(args.a), 'b': str(args.b), 'floor': str(args.floor) if args.floor else None, 'flagged': flagged, 'results': results}
(out / 'summary.json').write_text(json.dumps(summary, indent=1))
w = max(map(len, results), default=10)
print(f"{'image':<{w}}  flipMean  flipMaxTile  ssimMinTile  flags")
for k, r in results.items():
    if 'flipMean' in r:
        print(f"{k:<{w}}  {r['flipMean']:.5f}   {r['flipMaxTile']:.5f}      {r['ssimMinTile']:.5f}     {' '.join(r.get('flagged', []))}")
    else:
        print(f"{k:<{w}}  {next(iter(r))}")
print(f'\n{len(flagged)} flagged of {len(names)} -> {out}/summary.json')
sys.exit(1 if flagged else 0)
