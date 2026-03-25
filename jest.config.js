// eslint-disable-next-line @typescript-eslint/no-require-imports
const { pathsToModuleNameMapper } = require('ts-jest')
// eslint-disable-next-line @typescript-eslint/no-require-imports
const { compilerOptions } = require('./tsconfig.json')

const aliasMapper = pathsToModuleNameMapper(compilerOptions.paths || {}, {
  prefix: '<rootDir>/'
})

module.exports = {
  preset: 'ts-jest',
  testEnvironment: 'node',
  rootDir: '.',
  transform: {
    '^.+\\.tsx?$': ['ts-jest', { tsconfig: 'tsconfig.jest.json' }]
  },
  moduleNameMapper: {
    '^(\\.{1,2}/.*)\\.js$': '$1',
    '^.+\\?raw$': '<rootDir>/src/main/__tests__/__mocks__/rawMock.ts',
    ...aliasMapper
  },
  setupFiles: ['<rootDir>/jest.setup.ts'],
  collectCoverageFrom: [
    '<rootDir>/src/main/**/*.{ts,tsx,js,jsx}',
    '<rootDir>/src/preload/**/*.{ts,tsx,js,jsx}',
    '!<rootDir>/src/**/*.d.ts',
    '!<rootDir>/src/**/__tests__/**',
    '!<rootDir>/src/**/*.test.{ts,tsx,js,jsx}'
  ],
  coverageDirectory: '<rootDir>/coverage/main',
  coverageReporters: ['text', 'text-summary', 'html', 'lcov', 'json-summary'],
  testMatch: [
    '<rootDir>/src/main/**/*.test.(ts|tsx|js|jsx)',
    '<rootDir>/src/preload/**/*.test.(ts|tsx|js|jsx)',
    '<rootDir>/src/main/shared/**/*.test.(ts|tsx|js|jsx)'
  ]
}
