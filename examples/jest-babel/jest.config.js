const useIstanbul = process.env.COVERAGE_PLUGIN === 'istanbul';

module.exports = {
  testEnvironment: 'node',
  coverageProvider: 'babel',
  collectCoverageFrom: ['src/**/*.{js,ts}'],
  coverageDirectory: useIstanbul ? 'coverage-istanbul' : 'coverage',
  coverageReporters: ['json', 'text'],
};
