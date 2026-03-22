import js from '@eslint/js';
import globals from 'globals';

export default [
  {
    // Ignore generated and vendor directories
    ignores: ['node_modules/', 'src-tauri/'],
  },
  {
    files: ['frontend/renderer/**/*.js'],
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: 'module',
      globals: {
        ...globals.browser,
      },
    },
    rules: {
      ...js.configs.recommended.rules,

      // Style — matches existing codebase conventions
      'indent': ['error', 2, { SwitchCase: 1 }],
      'quotes': ['error', 'single', { avoidEscape: true }],
      'semi': ['error', 'always'],
      'no-trailing-spaces': 'error',
      'eol-last': ['error', 'always'],
      'comma-dangle': ['error', 'always-multiline'],

      // Quality
      'no-unused-vars': ['warn', { argsIgnorePattern: '^_', varsIgnorePattern: '^_', caughtErrorsIgnorePattern: '^_' }],
      'no-var': 'error',
      'prefer-const': 'warn',
      'eqeqeq': ['error', 'always', { null: 'ignore' }],
      'no-implicit-globals': 'error',

      // Allow console.error/warn but flag console.log in non-test files
      'no-empty': ['error', { allowEmptyCatch: true }],
      'no-console': ['warn', { allow: ['warn', 'error'] }],
    },
  },
  {
    // Relax rules for test files
    files: ['frontend/renderer/**/*.test.js'],
    rules: {
      'no-console': 'off',
    },
  },
];
