var a = 1, b = 2, c = 3, d = 4, e = 5, f = 6;
var sink = [];
function take(value) {
  sink.push(value);
  return value;
}
var x;

// A pragma before parentheses applies to the function inside them.
x = /* istanbul ignore next */ (function () { return a; });
x = /* istanbul ignore next */ (() => a);
take(/* istanbul ignore next */ (() => b));
take(/* istanbul ignore next */ (function () { return b; }));
x = { m: /* istanbul ignore next */ (() => c), n: /* istanbul ignore next */ (function () {}) };
x = [/* istanbul ignore next */ (function () {})];
x = /* istanbul ignore next */ ((function () {}));
x = (/* istanbul ignore next */ (function () {}));
x = !/* istanbul ignore next */ (function () {});
x = /* istanbul ignore next */ (async function () {});
x = /* istanbul ignore next */ (function* () {});
x = /* istanbul ignore next */ (async () => a);

// The call or member that starts at the parenthesis owns the pragma.
x = /* istanbul ignore next */ (function () { return a; })();
x = /* istanbul ignore next */ (function () { return a; }).call(this);
x = /* istanbul ignore next */ (() => a)();
x = (/* istanbul ignore next */ function () { return a; }).call(this);
x = /* istanbul ignore next */ (a, function () {});
x = /* istanbul ignore next */ (function () {}, b);
x = (/* istanbul ignore next */ function () {}, b);
x = /* istanbul ignore next */ function () { return a; }.call(this);
x = a || /* istanbul ignore next */ function () { return b; }.call(this);

// Logical operands and chains.
x = a || /* istanbul ignore next */ (b);
x = a ?? /* istanbul ignore next */ (b);
x = a || /* istanbul ignore next */ ((b));
x = a || /* istanbul ignore next */ (b) || c;
x = (a || /* istanbul ignore next */ (b)) || c;
x = a && /* istanbul ignore next */ (b || c);
x = a || /* istanbul ignore next */ (b && c) || d;
x = a || (/* istanbul ignore next */ b && c);
x = /* istanbul ignore next */ (a || b);
x = /* istanbul ignore next */ ((a || b));
x = (/* istanbul ignore next */ a || b);
x = /* istanbul ignore next */ (a) || b;
take(/* istanbul ignore next */ (a || b));
while (/* istanbul ignore next */ (a && !a)) {}
x = a || /* istanbul ignore next */ (function () {});

// Conditional arms and whole conditionals.
x = a ? (/* istanbul ignore next */ b) : c;
x = a ? b : (/* istanbul ignore next */ c);
x = a ? /* istanbul ignore next */ (b) : c;
x = a ? (/* istanbul ignore next */ function () {}) : c;
x = a ? /* istanbul ignore next */ (function () {}) : c;
x = a ? (/* istanbul ignore next */ b || c) : d;
x = a ? /* istanbul ignore next */ (b || c) : d;
x = /* istanbul ignore next */ (a ? b : c);
x = /* istanbul ignore next */ ((a ? b : c));
take(/* istanbul ignore next */ (a ? b : c));
x = /* istanbul ignore next */ (a ? b : c) ? d : e;
x = /* istanbul ignore next */ (a ? b : c) || d;
x = (a ? b : c) || /* istanbul ignore next */ (d ? e : f);
x = /* istanbul ignore next */ (a) ? b : c;
x = (/* istanbul ignore next */ (a)) ? b : c;
function returnsConditional() {
  return /* istanbul ignore next */ (a ? b : c);
}
function returnsFunction() {
  return /* istanbul ignore next */ (function () {});
}
take(returnsConditional());
take(returnsFunction());

// An ignored conditional or logical expression ignores its whole subtree.
x = /* istanbul ignore next */ a || (b ? c : d);
x = /* istanbul ignore next */ a || function () {};
x = /* istanbul ignore next */ a ? (b || c) : d;
x = /* istanbul ignore next */ (a) ? function () {} : d;

// The statement that a concise arrow body becomes is always counted.
var g1 = () => /* istanbul ignore next */ a;
var g2 = () => /* istanbul ignore next */ (a);
var g3 = () => /* istanbul ignore next */ ((a));
var g4 = () => /* istanbul ignore next */ (a ? b : c);
var g5 = () => /* istanbul ignore next */ a ? b : c;
var g6 = () => /* istanbul ignore next */ (a || b);
var g7 = () => /* istanbul ignore next */ (() => 1);
var g8 = () => /* istanbul ignore next */ (function () {});
var g9 = () => /* istanbul ignore next */ function () {};
var g10 = async () => /* istanbul ignore next */ (await a);
var g11 = (y) => /* istanbul ignore next */ (y || 1);
var g12 = () => (/* istanbul ignore next */ a ? b : c);
take([g1(), g2(), g3(), g4(), g5(), g6(), g7(), g8(), g9(), g10(), g11(0), g12()]);

// A variable initializer that is not hoisted loses its pragma.
var v1 = /* istanbul ignore next */ a || b;
var v2 = (/* istanbul ignore next */ a || b);
var v3 = /* istanbul ignore next */ a ? b : c;
var v4 = (/* istanbul ignore next */ a ? b : c);
var v5 = /* istanbul ignore next */ (a || b);
var v6 = /* istanbul ignore next */ (a ? b : c);
var v7 = /* istanbul ignore next */ sink.length || c;
var v8 = /* istanbul ignore next */ (function () {});
var v9 = /* istanbul ignore next */ (() => 1);
var v10 = /* istanbul ignore next */ function () {};
for (var v11 = /* istanbul ignore next */ function () {}; ; ) break;

// A class field value loses its pragma.
class Fields {
  m1 = /* istanbul ignore next */ () => 1;
  m2 = /* istanbul ignore next */ function () {};
  m3 = /* istanbul ignore next */ a || b;
  m4 = (/* istanbul ignore next */ () => 1);
  m5 = /* istanbul ignore next */ (() => 1);
  m6 = /* istanbul ignore next */ (a || b);
}

// A default value loses its pragma.
function withDefaults(
  p1 = /* istanbul ignore next */ function () {},
  p2 = /* istanbul ignore next */ () => 1,
  p3 = /* istanbul ignore next */ b || c,
  p4 = /* istanbul ignore next */ (b || c),
  p5 = /* istanbul ignore next */ (b ? c : d),
) {
  return [p1, p2, p3, p4, p5];
}
var { q1 = /* istanbul ignore next */ function () {} } = {};
var { q2 = /* istanbul ignore next */ (b || c) } = {};

// Statement-level pragmas around parentheses.
/* istanbul ignore next */ (function () { return a; })();
(/* istanbul ignore next */ function () { return a; })();

globalThis.parenthesizedPragmasResult = [
  sink.length,
  v1, v2, v3, v4, v5, v6, v7, v8, v9, v10, v11,
  new Fields().m3,
  withDefaults(),
  q1, q2,
];
