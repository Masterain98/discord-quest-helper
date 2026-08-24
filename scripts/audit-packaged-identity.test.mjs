import assert from 'node:assert/strict';
import test from 'node:test';

import {
  containsProductToken,
  IDENTITY,
  validateInternalName,
} from './audit-packaged-identity.mjs';

test('configured artifact identities satisfy the stable naming policy', () => {
  assert.equal(validateInternalName(IDENTITY.mainBinary, 'meridian'), true);
  assert.equal(validateInternalName(IDENTITY.bridgeBinary, 'waybridge'), true);
  assert.equal(validateInternalName(IDENTITY.runnerBuildBinary, 'stagecraft'), true);
});

test('product names and random-looking hex names fail internal validation', () => {
  assert.equal(validateInternalName('discord-quest-helper', 'discord-quest-helper'), false);
  assert.equal(validateInternalName('abcdef123456', 'abcdef123456'), false);
});

test('public identity remains allowed outside internal executable metadata', () => {
  assert.equal(IDENTITY.publicName, 'Discord Quest Helper');
  assert.equal(containsProductToken(IDENTITY.publicName), true);
});
