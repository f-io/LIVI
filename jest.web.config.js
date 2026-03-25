module.exports = {
  preset: 'ts-jest',
  testEnvironment: 'jsdom',
  maxWorkers: 2,
  collectCoverageFrom: [
    '<rootDir>/src/renderer/**/*.{ts,tsx,js,jsx}',
    '!<rootDir>/src/**/*.d.ts',
    '!<rootDir>/src/**/__tests__/**',
    '!<rootDir>/src/**/*.test.{ts,tsx,js,jsx}'
  ],
  coverageDirectory: '<rootDir>/coverage/renderer',
  coverageReporters: ['text', 'text-summary', 'html', 'lcov', 'json-summary'],
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
    '^(.*)\\?raw$': '<rootDir>/src/main/__tests__/__mocks__/rawMock.ts',
    '^@renderer/(.*)$': '<rootDir>/src/renderer/src/$1',
    '^@main/(.*)$': '<rootDir>/src/main/$1',
    '^@shared/(.*)$': '<rootDir>/src/main/shared/$1',
    '^@worker/(.*)$': '<rootDir>/src/renderer/src/components/worker/$1',
    '^@store/(.*)$': '<rootDir>/src/renderer/src/store/$1',
    '^@utils/(.*)$': '<rootDir>/src/renderer/src/utils/$1'
  },
  setupFilesAfterEnv: ['<rootDir>/jest.web.setup.ts']
}
