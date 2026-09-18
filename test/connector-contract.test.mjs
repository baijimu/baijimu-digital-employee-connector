import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
const manifest = JSON.parse(fs.readFileSync(new URL('../connector.json', import.meta.url)));
const pkg = JSON.parse(fs.readFileSync(new URL('../package.json', import.meta.url)));
test('release identity and operation paths agree', () => {
  assert.equal(manifest.version, pkg.version);
  assert.equal(manifest.source.revision, `v${pkg.version}`);
  assert.equal(new Set(manifest.methods.map(m => m.name)).size, manifest.methods.length);
  for (const method of manifest.methods) assert.equal(method.path, `/invoke/${method.name}`);
  assert.equal(manifest.runtime.processOwnership, 'host');
  assert.equal(manifest.transport.baseUrl, manifest.management.baseUrl);
});
test('employee has an independent app identity', () => {
  assert.equal(manifest.appId, 'digital-employee-connector');
  assert.equal(manifest.source.repo, 'baijimu/baijimu-digital-employee-connector');
  assert(manifest.methods.some(m => m.name === 'startThread'));
});
