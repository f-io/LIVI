module.exports = {
  preset: 'ts-jest',
  testEnvironment: 'jsdom',
  collectCoverageFrom: [
    '<rootDir>/src/renderer/**/*.{ts,tsx,js,jsx}',
    '!<rootDir>/src/**/*.d.ts',
    '!<rootDir>/src/**/__tests__/**',
    '!<rootDir>/src/**/*.test.{ts,tsx,js,jsx}'
  ],
  coverageDirectory: '<rootDir>/coverage/renderer',
  coverageReporters: ['text-summary', 'html', 'lcov', 'json-summary'],
  testMatch: ['<rootDir>/src/renderer/**/*.test.(ts|tsx|js)'],
  transform: {
    '^.+\\.tsx?$': [
      'ts-jest',
      {
        tsconfig: '<rootDir>/tsconfig.jest.json'
      }
    ]
  },
  moduleNameMapper: {
    '^@renderer/(.*)$': '<rootDir>/src/renderer/$1',
    '^@main/(.*)$': '<rootDir>/src/main/$1',
    '^@store/(.*)$': '<rootDir>/src/renderer/src/store/$1',
    '^@utils/(.*)$': '<rootDir>/src/renderer/src/utils/$1'
  },
  setupFilesAfterEnv: ['<rootDir>/jest.web.setup.ts']
}
