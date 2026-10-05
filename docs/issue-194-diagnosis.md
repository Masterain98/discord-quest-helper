# Issue #194: persistent Discord black screen after CDP game simulation

Diagnosed on 2026-10-05 against the affected local Windows Discord Stable client.

## Issue information

[Issue #194](https://github.com/Masterain98/discord-quest-helper/issues/194)
was opened by `manitsingh` at 2026-10-05 11:09:36 UTC. At retrieval it was open,
labelled `bug`, with no comments, assignees or milestone. The reporter used
Windows 11, Discord Stable and the MSI release described as `0.10.07`.
The attachment confirms application version `0.10.7`.

The reported sequence was process simulation, closing the simulated game,
then CDP simulation. Discord restarted into a persistent black screen;
restarting, clearing caches and changing GPU options had not restored it.

[The attached log](https://github.com/user-attachments/files/33055097/dqh-logs-2026-10-05T09-06-48.json)
contains 18 entries, from a session beginning 09:01:23 UTC to an export at
09:06:47 UTC. It records successful CDP authentication and client build 627798,
but contains no black-screen exception or game-spoof diagnostic. It alone
cannot prove the reporter's persisted state matches the local client.

The complete public issue body, metadata, comments array and timeline are
archived locally in `deliverables/issue-194/issue.json` and `timeline.json`.

## Confirmed local cause

1. Discord's translation exports synthesize functions for arbitrary method
   names. Matching `getDetectableGame`, `getGameByExecutable` and `findGame`
   therefore selected a translation module before the actual game database.
   Property-descriptor checks alone also accepted these translation exports.
2. Its `games` member was a translation function returning `{locale, ast}`.
   `detectableGamesPayload()` called it and used `Object.values()` as games.
3. Start and Stop dispatched these values as `GAMES_DATABASE_UPDATE`.
   Discord's handler inserts a normalized game in its map before calling
   `game.name.toLowerCase()`. The exception is caught by the helper, but the
   incomplete row remains and can be persisted by subsequent store updates.
4. The local `GameStore` cache contained 24,550 records, with a final row
   containing only `executables`, `aliases` and `thirdPartySkus`. It had neither
   `id` nor `name`.
5. On startup, `GameStore.initialize()` replays that row and throws before
   React mounts. The live stack was:

   ```text
   TypeError: Cannot read properties of undefined (reading 'toLowerCase')
       at j (web.24a0dd4254453b09.js:146:800566)
       at W.initialize (web.24a0dd4254453b09.js:146:801485)
       at W.initializeIfNeeded (...)
   ```

Reinstalling binaries preserves the Discord user profile and its game cache,
so it does not remove this poisoned row. Clearing unrelated asset or GPU caches
does not repair `GameStore` either.

The native-utils discovery had the same translation-decoy problem. Live
inspection found the real wrapper did not claim an own `getRunningGames`
property, while the translation exports did, matching the HTTP-facade exclusion
already used by this repository.

## Code repair

- Recognize the game database only when its `games` getter returns an Array;
  never call a `games` function or convert arbitrary objects into catalogue rows.
- Exclude translation exports from native-utils discovery and bump the bridge
  initialization version to invalidate previous discovery results.
- Validate the complete catalogue before dispatching during either Start or
  Stop, including legacy cached payloads. Reject malformed data before Discord
  can persist it.
- Convert `thirdPartySkus` to the event's `third_party_skus` field. Previously,
  redispatching normalized store rows could lose distributor identity.

## Recovery for already affected clients

The preventive code change does not erase existing poisoned caches. The
recovery tool uses the browser storage accessor from a temporary same-origin
iframe, so it works even when Discord's webpack initialization has failed and
the main page has removed `window.localStorage`.

Completely exit Discord and launch its installed executable with
`--remote-debugging-port=9223`. A black main window is sufficient; the helper
does not require a fully initialized Discord UI. From this checkout with
Node.js 22 or newer:

```text
node scripts/repair-discord-game-cache.mjs
node scripts/repair-discord-game-cache.mjs --apply
```

The first command only inspects. The second saves the original game cache in
`deliverables/discord-game-cache-<timestamp>.json`, verifies the cache has not
changed since that backup, and removes only records missing a string `id` or
`name`. It preserves the remaining catalogue, ETag, blocklists and persistence
version. Restart Discord after repair. Other storage entries are not read or
written. For another debugging port, append `--port=<port>`.

The affected local cache was backed up and exactly one invalid record was
removed. Reloading restored the Friends page with seven React root children.
A subsequent complete process restart still reached Friends. Discord then
refreshed the catalogue to 24,560 valid records. The recovery tool independently
reported zero invalid rows.

## Validation and limits

- Live comparison of the original and repaired module-discovery scripts:
  original selected `games: function`; repaired selected actual `GameStore`
  with an Array containing 24,560 records.
- The repaired payload builder accepted that real catalogue and retained every
  `thirdPartySkus` value in its API-compatible field.
- `pnpm test`: 158 tests passed, including translation-decoy discovery,
  persistence-before-exception ordering, invalid Start/Stop payloads, legacy
  cleanup, third-party identity and targeted recovery with concurrent-change
  protection.
- `cargo test -p discord-quest-helper --lib cdp_quest::tests -- --test-threads=1`:
  26 passed, one existing live game-spoof test ignored.
- `pnpm run build`: passed. The existing chunk-size advisory remains.
- Targeted Rust formatting and whitespace checks passed.

No quest progress requests were deliberately sent for validation. A full live
game-spoof cycle and the reporter's machine remain unverified. An installed old
release still contains the defect until replaced by a build containing the repair.
