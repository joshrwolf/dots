import {defineConfig} from 'eslint/config';
import tseslint from 'typescript-eslint';

const sourceFiles = [
  'src/{pi,pi-kit}/**/*.{ts,mjs,js}',
  'private/src/**/*.{ts,mjs,js}',
  'home/finicky/.config/finicky/finicky.js',
  'eslint.config.mjs',
];

export default defineConfig(
  {
    ignores: ['**/node_modules/**'],
  },
  {
    files: sourceFiles,
    rules: {
      eqeqeq: ['error', 'smart'],
      'one-var': ['error', 'never'],
      'no-var': 'error',
      'prefer-const': 'error',
      'no-async-promise-executor': 'error',
      'no-unsafe-finally': 'error',
      'no-unreachable': 'error',
      'valid-typeof': 'error',
    },
  },
  {
    files: [
      'src/{pi,pi-kit}/**/*.{ts,mjs,js}',
      'private/src/**/*.{ts,mjs,js}',
      'eslint.config.mjs',
    ],
    languageOptions: {
      parser: tseslint.parser,
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    plugins: {'@typescript-eslint': tseslint.plugin},
    rules: {
      '@typescript-eslint/await-thenable': 'error',
      '@typescript-eslint/no-floating-promises': [
        'error',
        {
          ignoreVoid: false,
          // Node owns test failure reporting; these calls return fulfilled
          // promises. Subtests still need awaiting to prevent cancellation.
          allowForKnownSafeCalls: [
            {
              from: 'package',
              package: 'node:test',
              name: ['test', 'it', 'describe', 'suite'],
            },
          ],
        },
      ],
      '@typescript-eslint/no-misused-promises': 'error',
      '@typescript-eslint/only-throw-error': 'error',
    },
  },
);
