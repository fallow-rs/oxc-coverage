import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect, userEvent, within } from 'storybook/test';
import { Counter } from './Counter';

const meta = {
  component: Counter,
} satisfies Meta<typeof Counter>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const ReachesLimit: Story = {
  args: { start: 1, step: 2, max: 4 },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole('button', { name: 'Add 2' }));
    await userEvent.click(canvas.getByRole('button', { name: 'Add 2' }));
    await expect(canvas.getByLabelText('count')).toHaveTextContent('4');
    await expect(canvas.getByText('Limit reached')).toBeInTheDocument();
  },
};
