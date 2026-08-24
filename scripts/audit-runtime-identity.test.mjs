import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

import {
  commandSummary,
  containsProductToken,
  fingerprintSummary,
  inspected,
  redactPath,
} from './audit-runtime-identity.mjs';

test('product token matching is case insensitive and avoids neutral names', () => {
  assert.equal(containsProductToken('/opt/DiscordQuestHelper/bin'), true);
  assert.equal(containsProductToken('discord-cdp-launcher'), true);
  assert.equal(containsProductToken('/opt/meridian/waybridge'), false);
});

test('home paths are redacted without changing unrelated paths', () => {
  assert.equal(redactPath('/Users/alice/Library/App', '/Users/alice'), '$HOME/Library/App');
  assert.equal(redactPath('/opt/runtime', '/Users/alice'), '/opt/runtime');
});

test('audited values state whether they were observed or read from a package', () => {
  assert.equal(inspected('meridian').source, 'observed');
  assert.equal(inspected('meridian', 'package').source, 'package');
});

test('command summaries never retain credentials or raw arguments', () => {
  const command = '/opt/meridian --authorization super-secret --cookie session-secret';
  const summary = commandSummary(command);

  assert.equal(summary.value, null);
  assert.equal(summary.containsProductToken, false);
  assert.equal(JSON.stringify(summary).includes('super-secret'), false);
  assert.equal(JSON.stringify(summary).includes('session-secret'), false);
});

test('fingerprint summary never includes the raw fingerprint', () => {
  const directory = mkdtempSync(join(tmpdir(), 'identity-audit-'));
  const snapshot = join(directory, 'snapshot.json');
  writeFileSync(snapshot, JSON.stringify({ native: { executableFingerprint: 'private-value' } }));
  const summary = fingerprintSummary(snapshot);

  assert.equal(summary.status, 'available');
  assert.equal(summary.length, 13);
  assert.equal(summary.containsProductToken, false);
  assert.equal(JSON.stringify(summary).includes('private-value'), false);
});

test('missing fingerprint is unavailable rather than clean', () => {
  const directory = mkdtempSync(join(tmpdir(), 'identity-audit-'));
  const snapshot = join(directory, 'snapshot.json');
  writeFileSync(snapshot, JSON.stringify({ games: [] }));

  assert.deepEqual(fingerprintSummary(snapshot), {
    status: 'unavailable',
    reason: 'no native executable fingerprint was present',
  });
});

test('an explicit unavailable probe is not hashed as a clean fingerprint', () => {
  const directory = mkdtempSync(join(tmpdir(), 'identity-audit-'));
  const snapshot = join(directory, 'snapshot.json');
  writeFileSync(snapshot, JSON.stringify({ native_diagnostics: [{ fingerprint: '<unavailable>' }] }));

  assert.equal(fingerprintSummary(snapshot).status, 'unavailable');
});
