'use strict';

// Parses lines such as `sku-1 x2 @12.50 #sale #new` into cart items.

const LINE_RE = /^(\S+)\s+x(\d+)\s+@(\d+(?:\.\d+)?)((?:\s+#\S+)*)\s*$/;

function parseLine(line, lineNumber = 1) {
  const trimmed = line.trim();
  if (trimmed === '' || trimmed.startsWith('//')) {
    return null;
  }
  const match = LINE_RE.exec(trimmed);
  if (!match) {
    throw new SyntaxError(`line ${lineNumber}: cannot parse "${trimmed}"`);
  }
  const [, sku, quantity, price, rawTags] = match;
  const tags = rawTags
    .split(/\s+/)
    .filter(Boolean)
    .map((tag) => tag.slice(1));
  return { sku, quantity: Number(quantity), price: Number(price), tags };
}

function parseList(text, { strict = false } = {}) {
  const items = [];
  const errors = [];
  text.split(/\r?\n/).forEach((line, index) => {
    try {
      const item = parseLine(line, index + 1);
      if (item) items.push(item);
    } catch (error) {
      if (strict) throw error;
      errors.push(error.message);
    }
  });
  return { items, errors };
}

function* chunk(items, size) {
  for (let i = 0; i < items.length; i += size) {
    yield items.slice(i, i + size);
  }
}

async function loadList(read, path) {
  const text = await read(path);
  return parseList(text || '');
}

module.exports = { parseLine, parseList, chunk, loadList };
