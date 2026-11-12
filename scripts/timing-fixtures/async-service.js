// A small API service in the shape of a typical Node handler: validate the
// request, look up the key, read rows, aggregate them, and serialize the
// response. The fake database answers on the next macrotask, as a real
// driver does, so each handler awaits real I/O turns.
'use strict';

const tick = () => new Promise((resolve) => setImmediate(resolve));

const keys = new Map(Array.from({ length: 50 }, (_, i) => [`key_${i}`, { orgId: `org_${i % 7}`, scope: i % 3 ? 'read' : 'live' }]));

const rows = Array.from({ length: 400 }, (_, i) => ({
  file: `src/services/file-${i % 40}.ts`,
  name: `fn${i}`,
  calls: (i * 7919) % 5000,
  state: i % 5 === 0 ? 'never_called' : 'called',
}));

const db = {
  async findKey(key) {
    await tick();
    return keys.get(key) ?? null;
  },
  async rowsFor(orgId, repo) {
    await tick();
    const shift = orgId.length + repo.length;
    return rows.filter((row, index) => (index + shift) % 3 !== 0);
  },
  recordRead: (orgId) => tick().then(() => orgId),
};

function validate(request) {
  if (typeof request.repo !== 'string' || request.repo.length === 0) {
    throw new TypeError('repo is required');
  }
  const limit = Number(request.limit ?? 100);
  if (!Number.isInteger(limit) || limit < 1 || limit > 1000) {
    throw new RangeError('limit must be 1..1000');
  }
  return { repo: request.repo, limit, key: request.key };
}

async function authenticate(key) {
  const record = await db.findKey(key);
  if (record === null) {
    const error = new Error('unknown key');
    error.status = 401;
    throw error;
  }
  return record;
}

const summarize = (items) =>
  items.reduce(
    (acc, row) => {
      acc.calls += row.calls;
      acc[row.state] = (acc[row.state] ?? 0) + 1;
      return acc;
    },
    { calls: 0 },
  );

function groupByFile(items) {
  const groups = new Map();
  for (const row of items) {
    const list = groups.get(row.file);
    if (list) list.push(row);
    else groups.set(row.file, [row]);
  }
  return [...groups.entries()].map(([file, list]) => ({ file, functions: list.length, calls: list.reduce((a, r) => a + r.calls, 0) }));
}

async function handle(request) {
  const input = validate(request);
  const auth = await authenticate(input.key);
  const items = await db.rowsFor(auth.orgId, input.repo);
  const page = items.slice(0, input.limit);
  const body = { summary: summarize(items), files: groupByFile(page), functions: page.map((row) => ({ ...row, repoPath: row.file.replace(/^src\//, '') })) };
  return db.recordRead(auth.orgId).then(() => JSON.stringify(body));
}

async function respond(request) {
  try {
    const body = await handle(request);
    return { status: 200, length: body.length };
  } catch (error) {
    return { status: error.status ?? 400, length: String(error.message).length };
  }
}

async function runBatch(count) {
  let bytes = 0;
  const inFlight = 50;
  for (let start = 0; start < count; start += inFlight) {
    const batch = [];
    for (let i = start; i < Math.min(count, start + inFlight); i++) {
      const bad = i % 97 === 0;
      batch.push(respond({ repo: bad ? '' : `repo-${i % 9}`, limit: 50 + (i % 200), key: `key_${i % 60}` }));
    }
    for (const result of await Promise.all(batch)) bytes += result.length;
  }
  return bytes;
}

module.exports = { runBatch };
