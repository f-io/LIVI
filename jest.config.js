// eslint-disable-next-line @typescript-eslint/no-require-imports
const { pathsToModuleNameMapper } = require('ts-jest')
// eslint-disable-next-line @typescript-eslint/no-require-imports
const { compilerOptions } = require('./tsconfig.json')

module.exports = {
  preset: 'ts-jest',
  testEnvironment: 'node',
  rootDir: '.',
  transform: {
    '^.+\\.tsx?$': ['ts-jest', { tsconfig: 'tsconfig.jest.json' }]
  },
  moduleNameMapper: pathsToModuleNameMapper(compilerOptions.paths || {}, {
    prefix: '<rootDir>/'
  }),
  setupFiles: ['<rootDir>/jest.setup.ts'],
  globals: { 'ts-jest': { isolatedModules: true } },
  testMatch: [
    '<rootDir>/(main|preload)/**/*.test.(ts|tsx|js|jsx)',
    '<rootDir>/shared/**/*.test.(ts|tsx|js|jsx)'
  ]
}
