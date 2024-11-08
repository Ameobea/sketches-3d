import typescriptEslint from '@typescript-eslint/eslint-plugin';
import globals from 'globals';
import tsParser from '@typescript-eslint/parser';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import js from '@eslint/js';
import { FlatCompat } from '@eslint/eslintrc';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const compat = new FlatCompat({
  baseDirectory: __dirname,
  recommendedConfig: js.configs.recommended,
  allConfig: js.configs.all,
});

export default [
  ...compat.extends('plugin:@typescript-eslint/recommended', 'prettier'),
  {
    plugins: {
      '@typescript-eslint': typescriptEslint,
    },

    languageOptions: {
      globals: {
        ...globals.browser,
        process: true,
        ga: true,
        module: true,
        __dirname: true,
        require: true,
      },

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
];
