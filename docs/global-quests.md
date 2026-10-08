# Global quest catalog

The Global Quests tab reads the community dataset documented at
https://discordquest.com/document/. This catalog is independent of the signed-in
account's executable quest list.

## Source and transport

- `https://api.discordquest.com/api/quests`: the complete quest array (the parser
  also accepts a `{ quests: [...] }` envelope).
- `https://api.discordquest.com/api/regions`: a `{ quests: [...] }` envelope
  containing country inclusion/exclusion and age-gate records.
- The datasets are requested concurrently using browser `fetch`, with omitted
  credentials, no Authorization header, no browser cache, and a 20-second timeout
  covering both the request and response body.
- The Pinia catalog store coalesces concurrent requests and caches successful
  data in memory for five minutes. Refresh bypasses this cache.
- Each dataset can succeed independently. Failed refreshes preserve its previous
  data and display a warning; missing restriction data stays unknown.

## Normalization and display

Quest IDs remain strings. Distinct regional versions are retained even when their
names match. Only valid expiration timestamps later than the current time are
shown; future start times are labeled Upcoming. An active page updates its clock
every second. Unfinished rows come first, then completed rows. Each group sorts
by descending start time, with ID as a stable tie-breaker.

Region records join by exact quest ID. Both the old country array and the newer
`{ include, exclude }` format are supported. UK normalizes to GB. Empty country
rules are global only when `is_global` is explicitly true. Invalid or missing rules
are unknown. Exclusions take precedence over inclusions. Country labels come from
`Intl.DisplayNames`; the country picker includes ISO 3166-1 countries, including
those with no country-specific quests.

`show_age_gate: true` is displayed as 18+, `false` as No age restriction marked,
and a missing value as Age unknown in the filters. Only the 18+ age badge is
shown in rows and details; other age states add no label. Community labels do not guarantee account
eligibility. Replacement IDs do not cause different quest rows or completion
statuses to be merged.

Reward types 1 and 2 are in-game rewards, 3 is avatar decoration, 4 (or an explicit
orb quantity) is Orbs, and 5 is Nitro. Unknown types remain Other / unknown.
Every task and reward contributes to filtering. Selected values within a group
use OR; different groups, country availability and name search use AND.

## Account completion

The existing authenticated quest reader supplies completion status, without
passing account credentials to the community service. Only an exact quest-ID
match with `completed_at` or `claimed_at` marks a row completed. An explicitly
different `user_status.user_id` is excluded. Completed rows remain browsable and
filterable, with muted styling and a green completion badge. The compact row
omits the separate game-name line. SVG country flags are bundled locally from
`country-flag-icons` with its MIT license; global availability uses a globe,
excluded countries have a red slash, and unknown regions display Unknown.

Account changes synchronously invalidate cached account quests and in-flight
requests from the previous account. Public catalog data and filters survive Tab
switches and do not enter execution queues. In CDP mode, details navigate the
Discord client to `/quest-home#{id}` through the existing `navigateDiscordSpa`
command. Other modes open `https://discord.com/quests/{id}` through the existing
shell plugin. A failed CDP navigation reports an error without opening a browser.
The source link in the page header opens https://discordquest.com/.

## Validation

The catalog parser, transport, store and account-loading tests cover legacy and
current region shapes, exclusions, unknown rules, multi-value filtering, expiry,
request timeout/body timeout, partial failure, cache/coalescing, completion and
account changes with late responses. Run `pnpm test`, `pnpm build`, and
`pnpm i18n:check`. Desktop smoke validation requires the normal Tauri host; a
standalone browser needs mocks for the app's existing native IPC calls.
