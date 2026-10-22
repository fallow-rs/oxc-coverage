const key = 'computed';

class Modifiers {
  static create() {
    return new Modifiers();
  }
  *items() {
    yield 1;
  }
  async load() {
    return 1;
  }
  async *stream() {
    yield 2;
  }
  get size() {
    return 3;
  }
  set size(value) {
    this.value = value;
  }
  static async build() {
    return 4;
  }
  [key]() {
    return 5;
  }
  *[Symbol.iterator]() {
    yield 6;
  }
}

const literal = {
  *items() {
    yield 1;
  },
  async load() {
    return 2;
  },
  async *stream() {
    yield 3;
  },
  get size() {
    return 4;
  },
  [key]() {
    return 5;
  },
};

globalThis.methodModifierResult = [Modifiers.create(), [...new Modifiers().items()], literal.size];
