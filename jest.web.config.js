module.exports = {
  preset: 'ts-jest',
  testEnvironment: 'jsdom',
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
