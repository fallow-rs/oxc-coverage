import { useState } from 'react';

export interface CounterProps {
  start?: number;
  step?: number;
  max?: number;
}

export function clamp(value: number, max?: number): number {
  if (max === undefined) return value;
  return value > max ? max : value;
}

export function Counter({ start = 0, step = 1, max }: CounterProps) {
  const [count, setCount] = useState(start);
  const atMax = max !== undefined && count >= max;

  return (
    <div>
      <output aria-label="count">{count}</output>
      <button type="button" disabled={atMax} onClick={() => setCount((value) => clamp(value + step, max))}>
        Add {step}
      </button>
      <button type="button" onClick={() => setCount(start)}>
        Reset
      </button>
      {atMax ? <p role="status">Limit reached</p> : null}
    </div>
  );
}
