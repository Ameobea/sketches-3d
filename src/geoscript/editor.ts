import { defaultKeymap, indentWithTab } from '@codemirror/commands';
import { basicSetup } from 'codemirror';
import {
  foldNodeProp,
  foldInside,
  indentNodeProp,
  LRLanguage,
  LanguageSupport,
  syntaxTree,
  type Language,
  continuedIndent,
  delimitedIndent,
} from '@codemirror/language';
import { linter, type Diagnostic } from '@codemirror/lint';
import { EditorState, Prec, type Extension } from '@codemirror/state';
import { EditorView, keymap, type KeyBinding } from '@codemirror/view';
import { gruvboxDark } from 'cm6-theme-gruvbox-dark';

import { parser as geoscriptParser } from './parser/geoscript';
import { filterNils } from '../viz/util/util';

const buildGeoscriptLanguage = (): { language: Language } => {
  const parserWithMetadata = geoscriptParser.configure({
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

  return { language: geoscriptLang };
};

export const buildGLSLLanguage = async (): Promise<LanguageSupport> => {
  const glslParser = await import('lezer-glsl').then(mod => mod.parser);

  const glslLanguage = LRLanguage.define({
    name: 'glsl',
    parser: glslParser.configure({
      props: [
        indentNodeProp.add({
          IfStatement: continuedIndent({ except: /^\s*({|else\b)/ }),
          CaseStatement: context => context.baseIndent + context.unit,
          BlockComment: () => null,
          CompoundStatement: delimitedIndent({ closing: '}' }),
          Statement: continuedIndent({ except: /^{/ }),
        }),
        foldNodeProp.add({
          'StructDeclarationList CompoundStatement': foldInside,
          BlockComment(tree) {
            return { from: tree.from + 2, to: tree.to - 2 };
          },
        }),
      ],
    }),
    languageData: {
      commentTokens: { line: '//', block: { open: '/*', close: '*/' } },
      indentOnInput: /^\s*(?:case |default:|\{|\})$/,
      closeBrackets: {
        stringPrefixes: ['L', 'u', 'U', 'u8', 'LR', 'UR', 'uR', 'u8R', 'R'],
      },
    },
  });

  return new LanguageSupport(glslLanguage);
};

interface BuildEditorArgs {
  container: HTMLElement;
  customKeymap?: readonly KeyBinding[];
  initialCode?: string;
  readonly?: boolean;
  lineNumbers?: boolean;
  onDocChange?: () => void;
  buildLanguage?: () => { language: Language } | LanguageSupport;
}

export const buildEditor = ({
  container,
  customKeymap,
  initialCode = '',
  readonly = false,
  onDocChange,
  buildLanguage = buildGeoscriptLanguage,
}: BuildEditorArgs) => {
  const lang = buildLanguage();
  const languageSupport = !!(lang as any).support
    ? (lang as LanguageSupport)
    : new LanguageSupport(lang.language);

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

  const extensions: Extension = filterNils([
    onDocChange
      ? EditorView.updateListener.of(update => {
          if (update.docChanged) {
            onDocChange();
          }
        })
      : null,
    customKeymap ? Prec.highest(keymap.of(customKeymap)) : null,
    basicSetup,
    keymap.of(defaultKeymap),
    keymap.of([indentWithTab]),
    gruvboxDark,
    EditorView.theme({
      '&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection': {
        backgroundColor: '#1b3830 !important',
      },
      ...(readonly
        ? {}
        : {
            '.cm-activeLine': { backgroundColor: 'rgba(255, 255, 255, 0.08) !important' },
            '.cm-activeLineGutter': { backgroundColor: 'rgba(255, 255, 255, 0.08) !important' },
          }),
    }),
    languageSupport,
    syntaxErrorLinter,
    readonly ? EditorView.editable.of(false) : null,
    readonly
      ? EditorView.theme({
          '.cm-cursor, .cm-dropCursor': { display: 'none !important' },
          '.cm-activeLine': { backgroundColor: 'transparent !important' },
          '.cm-activeLineGutter': { backgroundColor: 'transparent !important' },
          '&.cm-focused': { outline: 'none' },
          '.cm-content': { cursor: 'text' },
        })
      : null,
    EditorState.allowMultipleSelections.of(true),
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
