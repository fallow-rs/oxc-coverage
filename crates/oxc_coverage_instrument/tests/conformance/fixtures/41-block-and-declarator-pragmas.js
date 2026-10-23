const source = { ready: true };

// A pragma before a function or arrow block body ignores the statements in it.
const arrowBody = () => /* istanbul ignore next */ {
  return source.ready ? 1 : 2;
};
const functionBody = function () /* istanbul ignore next */ {
  return 3;
};
function declaredBody() /* istanbul ignore next */ {
  return 4;
}

// A pragma before a catch or finally block ignores that block.
let recovered = 0;
try {
  recovered = arrowBody();
} catch (error) /* istanbul ignore next */ {
  recovered = -1;
} finally /* istanbul ignore next */ {
  recovered += 1;
}

// A pragma before a declarator ignores its initializer.
var /* istanbul ignore next */ fallback = source.missing || 'default';
var first = 1, /* istanbul ignore next */ second = source.other || first;
for (var /* istanbul ignore next */ index = 0; index < 1; index++) {
  recovered += index;
}

// A class expression is not ignored, and a concise arrow body is not a block.
const Counted = /* istanbul ignore next */ class {
  value() {
    return 5;
  }
};
const concise = () => /* istanbul ignore next */ source.ready;

globalThis.blockPragmaResult = [
  functionBody(),
  declaredBody(),
  recovered,
  fallback,
  second,
  new Counted().value(),
  concise(),
];
