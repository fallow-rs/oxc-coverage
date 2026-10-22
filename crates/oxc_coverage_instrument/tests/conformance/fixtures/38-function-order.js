// A method gets its function id before the functions in its computed key.

class Computed {
  constructor() {
    this.ready = true;
  }
  [function () { return 'a'; }()]() {
    return 1;
  }
  static [(() => 'k')()]() {
    return 2;
  }
  get [(() => 'g')()]() {
    return 3;
  }
  set [(() => 's')()](v) {
    this.value = v;
  }
  async [(() => 'run')()]() {
    return 4;
  }
  *[(() => 'gen')()]() {
    yield 5;
  }
  [(function () { var q = 1; return q ? 'b' : 'c'; })()](x = 1) {
    return x && 6;
  }
}

class Nested {
  [(() => {
    class Inner {
      [(() => 'z')()]() {
        return 7;
      }
    }
    return new Inner().z() ? 'n' : 'o';
  })()]() {
    return 8;
  }
}

var Expression = class {
  [(() => 'e')()]() {
    return 9;
  }
};

var literal = {
  [(() => 'k')()]() {
    return 10;
  },
  get [(() => 'g')()]() {
    return 11;
  },
  set [(() => 's')()](v) {
    this.stored = v;
  },
  async [(() => 'run')()]() {
    return 12;
  },
  *[(() => 'gen')()]() {
    yield 13;
  },
  [(() => 'data')()]: function () {
    return 14;
  },
};

var computed = new Computed();
computed.s = 1;
literal.s = 2;

globalThis.functionOrderResult = [
  computed.a(),
  Computed.k(),
  computed.g,
  computed.value,
  computed.gen().next().value,
  computed.b(),
  new Nested().n(),
  new Expression().e(),
  literal.k(),
  literal.g,
  literal.stored,
  literal.gen().next().value,
  literal.data(),
];
