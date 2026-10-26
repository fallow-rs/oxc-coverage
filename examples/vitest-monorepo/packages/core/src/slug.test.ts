import { describe, expect, it } from 'vitest';
import { slugify, uniqueSlug } from './slug';

describe('slugify', () => {
  it('strips accents and punctuation', () => {
    expect(slugify('Crème Brûlée, à la carte!')).toBe('creme-brulee-a-la-carte');
  });
});

describe('uniqueSlug', () => {
  it('adds a counter when the slug is taken', () => {
    expect(uniqueSlug('Hello', new Set(['hello', 'hello-2']))).toBe('hello-3');
  });

  it('falls back to item for an empty title', () => {
    expect(uniqueSlug('!!!', new Set())).toBe('item');
  });
});
