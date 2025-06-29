import { defaultKeymap, indentWithTab } from '@codemirror/commands';
import { basicSetup } from 'codemirror';
import {
  foldNodeProp,
  foldInside,
  indentNodeProp,
  LRLanguage,
  LanguageSupport,
  syntaxTree,
} from '@codemirror/language';
import { linter, type Diagnostic } from '@codemirror/lint';
import { EditorState, Prec, type Extension } from '@codemirror/state';
import { gruvboxDark } from 'cm6-theme-gruvbox-dark';

import { parser } from './parser/geoscript';
import { EditorView, keymap, type KeyBinding } from '@codemirror/view';
import { filterNils } from '../viz/util/util';

interface BuildEditorArgs {
  container: HTMLElement;
  customKeymap?: readonly KeyBinding[];
  initialCode?: string;
  readonly?: boolean;
  lineNumbers?: boolean;
}

export const buildEditor = ({
  container,
  customKeymap,
  initialCode = '',
  readonly = false,
}: BuildEditorArgs) => {
  const syntaxErrorLinter = linter(view => {
    const diagnostics: Diagnostic[] = [];
    syntaxTree(view.state)
      .cursor()
      .iterate(({ type, from, to }) => {
        // console.log(type.name, from, to);
        if (type.isError) {
          diagnostics.push({
            from,
            to,
            severity: 'error',
            message: 'Syntax error',
          });
        }
      });
    return diagnostics;
  });

  const parserWithMetadata = parser.configure({
    props: [
      indentNodeProp.add({
        Application: context => context.column(context.node.from) + context.unit,
      }),
      foldNodeProp.add({
        Application: foldInside,
      }),
    ],
  });

  const geoscriptLang = LRLanguage.define({
    parser: parserWithMetadata,
    languageData: {
      commentTokens: { line: '//' },
    },
  });

  const extensions: Extension = filterNils([
    customKeymap ? Prec.highest(keymap.of(customKeymap)) : null,
    basicSetup,
    keymap.of(defaultKeymap),
    keymap.of([indentWithTab]),
    gruvboxDark,
    new LanguageSupport(geoscriptLang),
    syntaxErrorLinter,
    readonly ? EditorState.readOnly.of(true) : null,
  ]);

  const editorState = EditorState.create({
    doc: initialCode,
    extensions,
  });

  const editorView = new EditorView({
    state: editorState,
    parent: container,
  });

  return {
    editorView,
    getCode: () => editorView.state.doc.toString(),
    setCode: (code: string) => {
      editorView.dispatch({
        changes: { from: 0, to: editorView.state.doc.length, insert: code },
      });
      localStorage.lastGeoscriptPlaygroundCode = code;
    },
  };
};
