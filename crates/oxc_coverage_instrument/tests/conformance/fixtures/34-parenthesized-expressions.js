let total = 0;
const values = [1, 2, 3];
values.forEach((value) => (total += +value || 0));
const pair = (value, index) => (value, index);

const ratio = (total - 1) / values.length;
const wrapped = ((total));
const picked = total > 1 ? (total, ratio) : ((total - 1) * 2);
const nested = ((total > 2) ? ((ratio)) : (pair(1, 2)));
const settings = { scale: (ratio * 2), mode: ((total > 0) ? 'on' : 'off') };

function withDefault(limit = (total + 1)) {
  return limit;
}

class Holder {
  size = (total * 2);
}

const chosen = (total && ratio) || (wrapped ?? 0);
const ignoredInside = () => (/* istanbul ignore next */ total);
const handlers = {
  /* istanbul ignore next */
  run: (function () {
    return total;
  }),
};
globalThis.parenthesizedResult = [pair(1, 2), picked, nested, settings, withDefault(), new Holder().size, chosen, ignoredInside(), handlers.run()];
