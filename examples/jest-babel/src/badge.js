'use strict';

// A small view in JSX, in a `.js` file. Babel parses the JSX with the `jsx`
// parser plugin of @babel/preset-react. `h` builds plain objects, so the
// example needs no UI library.

const h = (type, props, ...children) => ({ type, props: props || {}, children });

function Badge({ count, label = 'items' }) {
  if (count === 0) {
    return <span className="badge empty">no {label}</span>;
  }
  const tone = count > 99 ? 'many' : 'some';
  return (
    <span className={`badge ${tone}`}>
      {count > 99 ? '99+' : count} {label}
    </span>
  );
}

const BadgeList = ({ counts }) => <ul>{counts.map((count) => <li key={count}>{Badge({ count })}</li>)}</ul>;

module.exports = { h, Badge, BadgeList };
