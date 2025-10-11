import typescriptEslint from '@typescript-eslint/eslint-plugin';
import globals from 'globals';
import tsParser from '@typescript-eslint/parser';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import js from '@eslint/js';
import { globalIgnores } from 'eslint/config';
import svelte from 'eslint-plugin-svelte';
import { FlatCompat } from '@eslint/eslintrc';
import ts from 'typescript-eslint';

import svelteConfig from './svelte.config.js';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const compat = new FlatCompat({
  baseDirectory: __dirname,
  recommendedConfig: js.configs.recommended,
  allConfig: js.configs.all,
});

export default [
  ...compat.extends('plugin:@typescript-eslint/recommended', 'prettier'),
  ...svelte.configs.recommended,
  {
    languageOptions: {
      globals: {
        ...globals.browser,
        ...globals.node,
        process: true,
        ga: true,
        module: true,
        __dirname: true,
        require: true,
      },
    },
  },
  globalIgnores([
    'node_modules',
    'build',
    'dist',
    '.svelte-kit',
    'public/build',
    'geoscript_backend',
    'backend',
    'src/viz/wasm/target',
    'src/viz/wasm/geodesics',
    'src/viz/wasm/uv_unwrap',
    'src/viz/wasm/cgal',
    'src/api',
    'src/geoscript/parser',
    'static',
    'src/viz/wasmComp',
    'src/viz/wasm/build',
    'src/geodesics/geodesics.js',
    'src/ammojs',
  ]),
  {
    rules: {
      '@typescript-eslint/indent': 0,

      quotes: [
        1,
        'single',
        {
          avoidEscape: true,
        },
      ],

      'linebreak-style': [2, 'unix'],
      semi: 0,
      'comma-dangle': [1, 'only-multiline'],
      'no-console': 0,
      'no-global-assign': 0,

      'no-multiple-empty-lines': [
        2,
        {
          max: 1,
        },
      ],

      'no-unused-vars': 0,

      '@typescript-eslint/no-unused-vars': [
        'warn',
        {
          argsIgnorePattern: '^_',
          varsIgnorePattern: '^_',
          caughtErrorsIgnorePattern: '^_',
        },
      ],

      'prefer-const': [
        'error',
        {
          destructuring: 'any',
          ignoreReadBeforeAssign: false,
        },
      ],

      '@typescript-eslint/explicit-function-return-type': 0,
      '@typescript-eslint/no-explicit-any': 0,
      '@typescript-eslint/no-non-null-assertion': 0,
      '@typescript-eslint/camelcase': 0,
      '@typescript-eslint/explicit-module-boundary-types': 0,
      '@typescript-eslint/no-empty-interface': 'off',

      '@typescript-eslint/consistent-type-imports': [
        'warn',
        {
          disallowTypeAnnotations: false,
        },
      ],
    },
  },
  {
    files: ['**/*.ts', '**/*.tsx'],
    plugins: {
      '@typescript-eslint': typescriptEslint,
    },

    languageOptions: {
      parser: tsParser,
      ecmaVersion: 2017,
      sourceType: 'module',

      parserOptions: {
        ecmaFeatures: {
          experimentalObjectRestSpread: true,
          jsx: true,
        },
      },
    },
  },
  {
    files: ['**/*.svelte', '**/*.svelte.ts', '**/*.svelte.js'],
    languageOptions: {
      parserOptions: {
        projectService: true,
        extraFileExtensions: ['.svelte'],
        parser: ts.parser,
        svelteConfig,
      },
    },

    rules: {
      'prefer-const': 'off',
    },
  },
];
