const { test } = require('node:test');
const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const { validateNpmVersion } = require('./validate-npm-version.cjs');

test('accepts stable, beta, easy including existing release inputs', () => {
  for (const value of [
    '0.0.0',
    '1.2.3',
    '0.1.44-easy.1',
    '0.1.44-easy.64',
    '1.2.3-beta.0',
    '12.34.56-beta.100',
  ])
    assert.equal(validateNpmVersion(value), true, value);
});
test('rejects malformed, unsafe and noncanonical version inputs', () => {
  for (const value of [
    undefined,
    null,
    '',
    'v1.2.3',
    '1.2',
    '01.2.3',
    '1.02.3',
    '1.2.03',
    '1.2.3-easy.01',
    '1.2.3-beta.00',
    '1.2.3-easy.-1',
    '1.2.3-rc.1',
    '1.2.3+meta',
    ' 1.2.3',
    '1.2.3\n',
    '1.2.3\r\n',
    '1.2.3;echo nope',
  ])
    assert.equal(validateNpmVersion(value), false, String(value));
});
test('the workflow CLI entry returns nonzero on invalid input without publishing', () => {
  for (const [value, code] of [
    ['0.1.44-easy.1', 0],
    ['1.2.3', 0],
    ['1.2.3-beta.1', 0],
    ['bad', 1],
  ]) {
    const result = spawnSync(
      process.execPath,
      [require.resolve('./validate-npm-version.cjs')],
      { env: { ...process.env, VERSION: value } }
    );
    assert.equal(result.status, code);
  }
});
