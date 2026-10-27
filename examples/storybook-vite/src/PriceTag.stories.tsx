import type { Meta, StoryObj } from '@storybook/react-vite';
import { PriceTag } from './PriceTag';

const meta = {
  component: PriceTag,
  args: { amount: 20 },
} satisfies Meta<typeof PriceTag>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Regular: Story = {};

export const OnSale: Story = {
  args: { discount: 0.25 },
};
