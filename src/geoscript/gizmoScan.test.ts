// Static gizmo-site discovery over real Lezer parses. Run with:
//   yarn tsx --test src/geoscript/gizmoScan.test.ts

import assert from 'node:assert/strict';
import { test } from 'node:test';

import { scanGizmoHandleIds, scanGizmoHandleOrder, scanSource } from './gizmoScan';

test('named call: handleId from the string literal, vec3 kind, name range recorded', () => {
  const src = 'pos = gizmo("cut1")';
  const [s] = scanSource(src);
  assert.equal(s.handleId, 'cut1');
  assert.equal(s.kind, 'vec3');
  assert.equal(s.dynamic, false);
  assert.equal(src.slice(s.nameRange![0], s.nameRange![1]), '"cut1"');
});

test('gizmo_transform and its transform_gizmo alias are transform kind', () => {
  assert.equal(scanSource('a = gizmo_transform("m")')[0].kind, 'transform');
  assert.equal(scanSource('a = transform_gizmo("m")')[0].kind, 'transform');
});

test('name passed as a kwarg resolves the same as positional', () => {
  assert.equal(scanSource('a = gizmo(name="k")')[0].handleId, 'k');
  assert.equal(scanSource('a = gizmo_transform(name="k", default=foo)')[0].handleId, 'k');
});

test('unnamed calls get positional @N keys in source order', () => {
  const sites = scanSource('a = gizmo()\nb = gizmo(origin=vec3(1,2,3))');
  assert.deepEqual(
    sites.map(s => s.handleId),
    ['@0', '@1']
  );
});

test('single quotes and escapes are unquoted to the runtime string', () => {
  assert.equal(scanSource("a = gizmo('q')")[0].handleId, 'q');
  assert.equal(scanSource('a = gizmo("a\\"b")')[0].handleId, 'a"b');
});

test('non-literal name is dynamic (no static handle id)', () => {
  const positional = scanSource('a = gizmo(someVar)')[0];
  assert.equal(positional.dynamic, true);
  assert.equal(scanSource('a = gizmo(name=foo)')[0].dynamic, true);
});

test('calls are found inside blocks and despite trailing syntax errors', () => {
  assert.equal(scanSource('x = { gizmo("inblock") }')[0]?.handleId, 'inblock');
  assert.equal(scanSource('y = gizmo("ok") +')[0]?.handleId, 'ok');
});

test('matches by callee name even when shadowed (runtime channel is authoritative)', () => {
  // A user-defined `gizmo` still trips the scanner; documented v1 limitation.
  assert.equal(scanSource('gizmo = |x| x\nz = gizmo("y")').at(-1)?.handleId, 'y');
});

test('scanGizmoHandleIds returns static ids only, excluding dynamic ones', () => {
  const ids = scanGizmoHandleIds('a = gizmo("keep")\nb = gizmo(dynVar)\nc = gizmo()');
  assert.deepEqual([...ids].sort(), ['@0', 'keep']);
});

test('arity reflects the gizmo variant (3 / 2 / 1)', () => {
  assert.equal(scanSource('a = gizmo("v")')[0].arity, 3);
  assert.equal(scanSource('a = gizmo2d("v")')[0].arity, 2);
  assert.equal(scanSource('a = gizmo1d("v")')[0].arity, 1);
  assert.equal(scanSource('a = gizmo_transform("v")')[0].arity, 3);
});

test('short aliases are recognized with matching kind/arity', () => {
  assert.deepEqual(
    [
      scanSource('a = giz("v")')[0],
      scanSource('a = giz2d("v")')[0],
      scanSource('a = giz1d("v")')[0],
      scanSource('a = giz_tfn("v")')[0],
    ].map(s => [s.kind, s.arity]),
    [
      ['vec3', 3],
      ['vec3', 2],
      ['vec3', 1],
      ['transform', 3],
    ]
  );
});

test('unnamed @N ids are shared across the whole gizmo family in source order', () => {
  const sites = scanSource('a = gizmo()\nb = gizmo2d()\nc = gizmo1d()');
  assert.deepEqual(
    sites.map(s => s.handleId),
    ['@0', '@1', '@2']
  );
});

test('scanGizmoHandleOrder lists static handles in document order (drives colors)', () => {
  const order = scanGizmoHandleOrder('a = gizmo("x")\nb = gizmo(dynVar)\nc = gizmo2d("y")\nd = gizmo1d()');
  assert.deepEqual(order, ['x', 'y', '@0']);
});
