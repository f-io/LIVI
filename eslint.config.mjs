// @ts-check
import js from '@eslint/js'
import tseslint from 'typescript-eslint'
import react from 'eslint-plugin-react'
import reactHooks from 'eslint-plugin-react-hooks'
import globals from 'globals'
import prettier from 'eslint-config-prettier'

export default tseslint.config(
  {
    ignores: ['node_modules', 'dist', 'out', 'build', 'coverage', 'release', 'tmp', 'temp']
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ['**/*.{ts,tsx,js,jsx,cjs,mjs,cts,mts}'],
    languageOptions: {
      ecmaVersion: 2023,
      sourceType: 'module',
      globals: { ...globals.browser, ...globals.node }
    },
    plugins: {
      react,
      'react-hooks': reactHooks
    },
    settings: { react: { version: 'detect' } },
    rules: {
      // React
      'react/react-in-jsx-scope': 'off',
      'react/jsx-uses-react': 'off',
      'react/jsx-uses-vars': 'warn',

      // Hooks
      'react-hooks/rules-of-hooks': 'error',
      'react-hooks/exhaustive-deps': 'warn',

      // TS / general
      '@typescript-eslint/no-explicit-any': ['warn', { ignoreRestArgs: true }],
      '@typescript-eslint/no-unused-vars': [
        'warn',
        {
          args: 'after-used',
          argsIgnorePattern: '^_',
          varsIgnorePattern: '^_',
          caughtErrors: 'none'
        }
      ],
      'no-empty': ['warn', { allowEmptyCatch: true }],
      '@typescript-eslint/ban-ts-comment': [
        'error',
        { 'ts-ignore': 'allow-with-description', 'ts-expect-error': 'allow-with-description' }
      ]
    }
  },

  // Declaration files
  {
    files: ['**/*.d.ts'],
    rules: {
      '@typescript-eslint/no-explicit-any': 'off',
      '@typescript-eslint/no-unused-vars': 'off'
    }
  },

  // Plain JS assets
  {
    files: ['src/renderer/public/**/*.js'],
    rules: { '@typescript-eslint/no-unused-vars': 'off' }
  },

  // Prettier last
  prettier
)
