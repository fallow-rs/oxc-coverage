// A name from a binding, a key or a target goes only to its direct function
// value, also through parentheses, and never to a later function.

function withClassDefault(make = class { ['k' + 1]() { return 1; } }) {
  return [1].map(() => new make().k1());
}

function withParenDefault(cb = (() => 2)) {
  return cb();
}

function withNamedDefault(cb = function own() { return 3; }) {
  return cb();
}

var { pick = class { ['p' + 1]() { return 4; } } } = {};
var parenFunction = (function () { return 5; });
var parenArrow = (() => 6);
var ownName = function own() { return 7; };
var target = { x: {} };
target.cls = class { ['c' + 1]() { return 8; } };
var later = [1].map(() => 9);
target.paren = (function () { return 10; });
target[[0].map(() => 'x')[0]].y = function () { return 11; };
target.own = function ownTarget() { return 12; };
var holder = { value: function ownValue() { return 13; } };
var assigned;
assigned = function () { return 14; };
var { a = () => 15 } = {};
var b;
[b = () => 16] = [];

globalThis.defaultMethodNamesResult = [
  withClassDefault()[0],
  withParenDefault(),
  withNamedDefault(),
  new pick().p1(),
  parenFunction(),
  parenArrow(),
  ownName(),
  new target.cls().c1(),
  later[0],
  target.paren(),
  target.x.y(),
  target.own(),
  holder.value(),
  assigned(),
  a(),
  b(),
];
