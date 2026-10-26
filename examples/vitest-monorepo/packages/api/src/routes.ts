import { uniqueSlug } from '../../core/src/slug';

export interface Post {
  title: string;
  slug: string;
}

export class PostStore {
  readonly #posts = new Map<string, Post>();

  create(title: string): Post {
    const slug = uniqueSlug(title, new Set(this.#posts.keys()));
    const post = { title, slug };
    this.#posts.set(slug, post);
    return post;
  }

  find(slug: string): Post | undefined {
    return this.#posts.get(slug);
  }
}
