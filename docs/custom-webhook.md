# Custom Webhook

Custom Webhook sends a versioned JSON message to one user-configured endpoint.
It is disabled by default and follows the notification event switches.

## Connection and response contract

Endpoints may use HTTP or HTTPS, public or private hosts, loopback addresses,
custom ports and query parameters. Relative URLs, other protocols, URL user
information and fragments are rejected. HTTPS uses normal certificate validation.
Requests do not follow redirects and are never retried, including reqwest
protocol-level retries. The HTTP timeout is 10 seconds; the notification service
retains its independent 15-second delivery deadline.

Each delivery uses POST with `Content-Type: application/json`. When configured,
the separate token adds `Authorization: Bearer <token>`. The endpoint and token
are stored in the system credential store, never in the settings JSON or returned
settings. Empty password inputs keep saved credentials. **Remove token** removes
just the optional token when saved; **Clear configuration** removes the channel.

Any status from 200 through 299 confirms successful acceptance. Status 202 is
reported as Submitted; the other successful statuses are reported as Sent.
The response may be empty or non-JSON; its body is never inspected.
401, 403, 404 and 429 use the existing credential, permission, destination and
rate-limit errors. All other statuses, including redirects, are rejected.
Errors and application logs do not include request URLs, tokens, message bodies
or response bodies. Success confirms HTTP acceptance by the receiver, not
downstream delivery.

## JSON schema version 1

All the following keys are always present:

| Key | Type | Meaning |
| --- | --- | --- |
| `schema_version` | integer | Currently `1` |
| `event_id` | string | Unique execution-event ID; distinct from a Discord quest ID |
| `event_type` | string | `quest_started`, `quest_completed` or `queue_completed` |
| `timestamp` | string | Message creation time, UTC RFC3339 with milliseconds |
| `locale` | string | Requested app language; unsupported languages use English default text |
| `is_test` | boolean | Whether this is a settings test |
| `title` | string | Rendered title; may be empty |
| `content` | string | Rendered content; may be empty |
| `quest` | object or null | Task data for task events; null for queue events |
| `queue` | object or null | Queue statistics for queue events; null for task events |

`quest` contains `name` (string), `reward_image` (string or null) and
`estimated_complete_at` (UTC RFC3339 string or null). The estimate is only
applicable to task-start events; missing or invalid estimates become null.
Task names and image sources preserve the event's original values. In
particular, `/icons/orbs.png` identifies an app-bundled image, not a public image
URL on the receiver's host. The adapter does not download or embed images.

`queue` contains `succeeded`, `failed` and `skipped`, all non-negative integers.

Task started:

```json
{
  "schema_version": 1,
  "event_id": "execution-uuid:started",
  "event_type": "quest_started",
  "timestamp": "2026-10-31T08:00:00.000Z",
  "locale": "en",
  "is_test": false,
  "title": "Task started",
  "content": "Example task\nEstimated completion: 2026-10-31T08:05:00.000Z",
  "quest": {
    "name": "Example task",
    "reward_image": "/icons/orbs.png",
    "estimated_complete_at": "2026-10-31T08:05:00.000Z"
  },
  "queue": null
}
```

Task completed:

```json
{
  "schema_version": 1,
  "event_id": "execution-uuid:completed",
  "event_type": "quest_completed",
  "timestamp": "2026-10-31T08:05:00.000Z",
  "locale": "en",
  "is_test": false,
  "title": "Task completed",
  "content": "Example task",
  "quest": {
    "name": "Example task",
    "reward_image": null,
    "estimated_complete_at": null
  },
  "queue": null
}
```

Queue completed:

```json
{
  "schema_version": 1,
  "event_id": "queue-uuid:completed",
  "event_type": "queue_completed",
  "timestamp": "2026-10-31T08:10:00.000Z",
  "locale": "en",
  "is_test": false,
  "title": "Queue processing completed",
  "content": "Succeeded: 2; failed: 1; skipped: 0",
  "quest": null,
  "queue": { "succeeded": 2, "failed": 1, "skipped": 0 }
}
```

## Templates

The three events each have independent title and content templates. Fixed text,
spacing, newlines, Unicode and explicit empty strings are preserved. No footer,
timestamp or extra text is appended. The surrounding JSON structure is fixed;
JSON serialization handles quotes, line breaks and backslashes safely.

| Placeholder | Applies to | Value |
| --- | --- | --- |
| `{quest-name}` | Task events | Original task name |
| `{event-type}` | All events | Stable event type identifier |
| `{event-id}` | All events | Execution-event ID |
| `{timestamp}` | All events | Same UTC timestamp as the JSON envelope |
| `{estimated-complete-at}` | Task started | Same normalized estimate as `quest` |
| `{reward-image}` | Task events | Original image source |
| `{succeeded}` | Queue completed | Decimal success count |
| `{failed}` | Queue completed | Decimal failure count |
| `{skipped}` | Queue completed | Decimal skip count |

Unavailable values become empty strings. Unknown named placeholders remain
unchanged, and the editor shows a non-blocking warning. Double braces escape a
placeholder: `{{quest-name}}` sends literal `{quest-name}`. Replacement runs once,
so braces within a substituted task name never trigger another replacement.
There are no expressions, scripts or conditionals.

Unmodified templates use the current language's defaults in all 16 supported
languages. Customizations remain as written across language changes.
**Restore default** removes one override; deleting its input instead saves an
explicit empty string. Configuration remains version 2: only overrides are
stored in `values`. Template defaults are read as raw translation strings and
are never passed through the regular vue-i18n interpolation engine.

Tests use the current unsaved draft and never save it. The event chooser is
available without debug mode and supports all three events. Calls without an
explicit event use a localized sample task-completed event. Each test receives
an independent event ID and `is_test: true`. Typed credentials are cleared after
testing, saving and closing the editor, consistent with the other channels.
