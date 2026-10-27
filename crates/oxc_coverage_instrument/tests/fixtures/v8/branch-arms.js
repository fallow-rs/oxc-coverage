function pick(flag) {
  return flag ? 'a' : 'b';
}
function always(flag) {
  return flag ? 'a' : 'b';
}
function either(a, b) {
  return a || b;
}
function fallback(a, b) {
  return a ?? b;
}
function greet(name = 'world') {
  return name;
}
function guard(x) {
  if (x > 0) {
    x = 1;
  }
  return x;
}
function kind(x) {
  switch (x) {
    case 1:
      return 'one';
    default:
      return 'other';
  }
}
function only(x) {
  switch (x) {
    case 1:
      return 'one';
    default:
      return 'other';
  }
}
function nested(a, b, c) {
  return a || (b && c);
}
for (let i = 0; i < 4; i++) {
  pick(i < 3);
  always(true);
  either(0, i);
  fallback(i < 1 ? null : 1, 2);
  greet(i < 1 ? undefined : 'x');
  guard(i - 2);
  kind(i < 3 ? 1 : 2);
  only(1);
  nested(i < 1, i < 3, 0);
}
