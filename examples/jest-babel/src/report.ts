// No test imports this file. Jest `collectCoverageFrom` and nyc `all` still
// report it, from the coverage object in the instrumented code.

export interface Line {
  label: string;
  amount: number;
}

export function renderReport(lines: Line[], width = 40): string {
  if (lines.length === 0) {
    return '(empty)';
  }
  return lines
    .map(({ label, amount }) => {
      const value = amount.toFixed(2);
      const gap = Math.max(1, width - label.length - value.length);
      return label + ' '.repeat(gap) + value;
    })
    .join('\n');
}
