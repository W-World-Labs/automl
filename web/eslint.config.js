import js from '@eslint/js';
import tseslint from 'typescript-eslint';

export default tseslint.config(
  { ignores: ['dist', 'src/api/schema.d.ts'] },
  {
    files: ['**/*.{ts,tsx}'],
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    languageOptions: {
      ecmaVersion: 2022,
      globals: { window: 'readonly', document: 'readonly', navigator: 'readonly', fetch: 'readonly', console: 'readonly', localStorage: 'readonly' },
    },
    rules: {
      '@typescript-eslint/no-explicit-any': 'off'
    },
  },
);
