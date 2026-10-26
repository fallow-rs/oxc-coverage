import { describe, expect, it } from 'vitest';
import { PostStore } from './routes';

describe('PostStore', () => {
  it('gives each post a unique slug', () => {
    const store = new PostStore();
    expect(store.create('Release notes').slug).toBe('release-notes');
    expect(store.create('Release notes').slug).toBe('release-notes-2');
    expect(store.find('release-notes-2')?.title).toBe('Release notes');
  });
});
