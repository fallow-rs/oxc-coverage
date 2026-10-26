import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { PriceTag } from './PriceTag';

describe('PriceTag', () => {
  it('renders a plain price', () => {
    expect(renderToStaticMarkup(<PriceTag amount={10} />)).toBe('<span class="price">€10.00</span>');
  });

  it('renders a sale price with the old price struck through', () => {
    expect(renderToStaticMarkup(<PriceTag amount={20} discount={0.25} />)).toBe(
      '<span class="price price--sale"><s>€20.00</s>€15.00</span>',
    );
  });
});
