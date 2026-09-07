// Run with: yarn tsx --test src/geoscript/signatureHelp.test.ts
import assert from 'node:assert/strict';
import { test } from 'node:test';

import { autocompletion, closeCompletion, completionStatus, startCompletion } from '@codemirror/autocomplete';
import { LRLanguage } from '@codemirror/language';
import { EditorState, type TransactionSpec } from '@codemirror/state';
import { keymap, showTooltip, type Command, type EditorView } from '@codemirror/view';

import type { SignatureHelp } from './analysisClient';
import { buildSignatureHelpExtension, setHelp, sigHelpField } from './signatureHelp';
import { parser } from './parser/geoscript';

const help: SignatureHelp = {
  docs: {
    name: 'path_union',
    module: 'path',
    signatures: [0, 1].map(() => ({
      params: [{ name: 'paths', ty: 'path[]', description: '' }],
      description: '',
      return_type: 'path',
    })),
  },
  active_signature: 0,
  active_params: [0, 0],
  compatible: [true, true],
  call_line: 1,
  call_col: 1,
};

const editor = (marked: string, withCompletion = false) => {
  const anchor = marked.indexOf('‸');
  assert.notEqual(anchor, -1);
  return EditorState.create({
    doc: marked.replace('‸', ''),
    selection: { anchor },
    extensions: [
      LRLanguage.define({ parser }),
      buildSignatureHelpExtension(
        () => false,
        () => ''
      ),
      withCompletion ? autocompletion({ override: [() => null], activateOnTyping: false }) : [],
    ],
  });
};

const receiveHelp = (state: EditorState, explicit = false, result: SignatureHelp | null = help) =>
  state.update({ effects: setHelp.of({ help: result, explicit }) }).state;
const shown = (state: EditorState) => state.facet(showTooltip).some(tooltip => tooltip !== null);
const insert = (state: EditorState, text: string) =>
  state.update({
    changes: { from: state.selection.main.head, insert: text },
    selection: { anchor: state.selection.main.head + text.length },
    userEvent: 'input.type',
  }).state;

// These commands only use state and dispatch; no DOM or request worker is needed.
const runCommand = (state: EditorState, command: Command) => {
  const target = {
    state,
    dispatch(spec: TransactionSpec) {
      this.state = this.state.update(spec).state;
    },
  };
  assert.equal(command(target as EditorView), true);
  return target.state;
};
const press = (state: EditorState, key: string) => {
  const command = state
    .facet(keymap)
    .flat()
    .find(binding => binding.key === key)?.run;
  assert.ok(command, key);
  return runCommand(state, command);
};

test('typing an argument hides help immediately and keeps it hidden through the complete name', () => {
  let state = receiveHelp(editor('path_union(‸)'));
  assert.ok(shown(state));
  for (const char of 'build_path') {
    state = insert(state, char);
    assert.equal(shown(state), false, `before analysis: ${state.doc}`);
    state = receiveHelp(state);
    assert.equal(shown(state), false, `after analysis: ${state.doc}`);
  }
  state = insert(state, ',');
  assert.equal(shown(state), false, 'do not flash the previous argument highlight');
  assert.ok(shown(receiveHelp(state)), 'restore help at the next argument');
});

test('argument boundaries allow help, including keyword values and nested calls', () => {
  for (const src of [
    'path_union(‸)',
    'path_union(\n  ‸\n)',
    'path_union(a, ‸)',
    'path_union(paths=‸)',
    'path_union(paths = ‸)',
    'path_union(build_path(‸))',
    'path_union(build_path()‸)',
    'path_union([a, b]‸)',
    'path_union("finished"‸)',
    'path_union(a, // finished argument\n  ‸)',
  ]) {
    assert.ok(shown(receiveHelp(editor(src))), src);
  }
});

test('word editing suppresses help even with no completion results', () => {
  for (const src of [
    'path_union(b‸)',
    'path_union(build_path‸)',
    'path_union(build_‸path)',
    'path_union(‸build_path)',
    'path_union(unmatched_word‸)',
    'path_union(@‸)',
    'path_union(@build_path‸)',
    'path_union(pa‸ths=[])',
    'path_union(paths=bu‸)',
    'path_union(paths=build_path‸)',
    'path_union(12.‸)',
    'path_union(a + ‸)',
    'path_union(a.‸)',
    'path_union("text ‸ here")',
    'path_union("unfinished‸',
    'path_union("escaped\\"‸',
    'path_union(// a comment ‸\n)',
  ]) {
    assert.equal(shown(receiveHelp(editor(src))), false, src);
  }
});

test('cursor movement and deletion can restore temporarily hidden help', () => {
  let state = receiveHelp(editor('path_union(a, bu‸)'));
  assert.equal(shown(state), false);
  state = state.update({ selection: { anchor: state.selection.main.head - 3 } }).state;
  assert.equal(shown(state), false, 'wait for the highlight at the new cursor position');
  assert.ok(shown(receiveHelp(state)));

  state = receiveHelp(editor('path_union(b‸)'));
  const head = state.selection.main.head;
  state = state.update({ changes: { from: head - 1, to: head }, userEvent: 'delete.backward' }).state;
  assert.ok(shown(receiveHelp(state)));
});

test('explicit completion takes precedence at an empty argument and closing it restores help', () => {
  let state = receiveHelp(editor('path_union(‸)', true));
  assert.ok(shown(state));
  state = runCommand(state, startCompletion);
  assert.equal(completionStatus(state), 'pending');
  assert.equal(shown(state), false);
  state = runCommand(state, closeCompletion);
  assert.ok(shown(state));
});

test('manual signature help closes completion and overrides word suppression until the next edit', () => {
  let state = receiveHelp(editor('path_union(bu‸)', true));
  state = runCommand(state, startCompletion);
  state = press(state, 'Ctrl-Shift-Space');
  assert.equal(completionStatus(state), null);
  state = receiveHelp(state, true);
  assert.ok(shown(state));
  state = receiveHelp(insert(state, 'i'));
  assert.equal(shown(state), false);
});

test('temporary suppression preserves overload choice and Escape dismissal for the same call', () => {
  let state = receiveHelp(editor('path_union(‸)'));
  state = press(state, 'Alt-ArrowDown');
  assert.equal(state.field(sigHelpField).override, 1);
  state = receiveHelp(insert(state, 'a'));
  assert.equal(shown(state), false);
  state = receiveHelp(insert(state, ','));
  assert.ok(shown(state));
  assert.equal(state.field(sigHelpField).override, 1);

  state = press(state, 'Escape');
  state = receiveHelp(insert(state, 'b'));
  state = receiveHelp(insert(state, ','));
  assert.equal(shown(state), false, 'the dismissed call stays dismissed');
  state = receiveHelp(state, false, null);
  assert.ok(shown(receiveHelp(state)), 'leaving the call clears its dismissal');
});
