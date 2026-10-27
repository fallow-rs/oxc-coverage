export interface Line {
  description: string;
  quantity: number;
  unitPrice: number;
}

export function lineTotal(line: Line): number {
  if (line.quantity <= 0) throw new RangeError('quantity must be positive');
  return Math.round(line.quantity * line.unitPrice * 100) / 100;
}

export function invoiceTotal(lines: readonly Line[], vatRate = 0.21): { net: number; vat: number; gross: number } {
  const net = lines.reduce((sum, line) => sum + lineTotal(line), 0);
  const vat = Math.round(net * vatRate * 100) / 100;
  return { net, vat, gross: net + vat };
}

export function formatInvoiceNumber(year: number, sequence: number): string {
  // Not covered by the tests on purpose, so the reports show a gap.
  return `${year}-${String(sequence).padStart(5, '0')}`;
}

export function discountedTotal(gross: number, discount?: { rate?: number }): number {
  // `?.` and `??` give branch entries in the default profile only.
  const rate = discount?.rate ?? 0;
  return Math.round(gross * (1 - rate) * 100) / 100;
}
