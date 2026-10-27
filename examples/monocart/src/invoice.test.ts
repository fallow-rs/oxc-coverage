import { describe, expect, it } from 'vitest';
import { discountedTotal, invoiceTotal, lineTotal } from './invoice';

describe('invoice', () => {
  it('totals the lines and adds VAT', () => {
    expect(invoiceTotal([{ description: 'Hosting', quantity: 2, unitPrice: 10 }])).toEqual({ net: 20, vat: 4.2, gross: 24.2 });
  });

  it('rejects a line without quantity', () => {
    expect(() => lineTotal({ description: 'Empty', quantity: 0, unitPrice: 5 })).toThrow(RangeError);
  });

  it('applies a discount', () => {
    expect(discountedTotal(100, { rate: 0.1 })).toBe(90);
  });
});
