# Adding a notification channel

Notification channels are compiled into the app. A normal channel PR adds one
adapter, one entry in `notifications/channels/mod.rs`, translations and tests.
It should not change quest execution, the dispatcher, another adapter, or the
settings page. Changes to the shared interface need a separate compatibility
explanation and regression coverage.

## Boundaries

- `channel`: the `NotificationChannel` interface, descriptors and registry.
- `channels`: adapter-specific validation, formatting and delivery. `system`
  owns Windows/macOS/Linux notification integration.
- `message` / `model`: channel-independent event content and localization.
- `config` / `secrets` / `atomic`: versioned settings and credential transactions.
- `service`: filtering, deduplication, independent delivery and result recording.
- `mod`: Tauri commands and the native result-event bridge.

The interface is deliberately small:

```rust
trait NotificationChannel: Send + Sync {
    fn descriptor(&self) -> Descriptor;
    fn validate(&self, config: &ResolvedConfig) -> Result<()>;
    fn send<'a>(&'a self, config: &'a ResolvedConfig, message: &'a Message)
        -> BoxFuture<'a, Result<Receipt>>;
}
```

Each adapter receives only its resolved configuration and message. Prefer a
small typed adapter configuration (as in Telegram and Discord), borrowing
credential strings from `ResolvedConfig`. The registry supplies the common HTTP
client through the adapter constructor. It initializes lazily so TLS/proxy setup
errors cannot prevent application startup. It has a 10-second timeout and disables
redirects. Delivery has a separate 15-second total deadline, with no retry.
Keep validation fast and free of I/O; asynchronous work belongs in `send`.

## Minimal adapter example

This complete example can be placed in `channels/example.rs`. Its endpoint is
intentionally fictional; replace the protocol, response validation and host with
the provider's real documented contract before proposing a production adapter.

```rust
use super::super::{
    channel::{Descriptor, Field, InputKind, NotificationChannel, Receipt, ResolvedConfig},
    error::{Code, Error, Result},
    http,
    message::Message,
};
use futures_util::future::BoxFuture;

pub(super) struct ExampleChannel {
    client: http::Client,
}
impl ExampleChannel {
    pub fn new(client: http::Client) -> Self { Self { client } }
}
struct ExampleConfig<'a> { token: &'a str, recipient: &'a str }
impl<'a> ExampleConfig<'a> {
    fn parse(config: &'a ResolvedConfig) -> Result<Self> {
        let token = config.get("token");
        let recipient = config.get("recipient");
        if token.is_empty() { return Err(Error::new(Code::InvalidConfig).at("token")); }
        if recipient.is_empty() {
            return Err(Error::new(Code::InvalidConfig).at("recipient"));
        }
        Ok(Self { token, recipient })
    }
}
impl NotificationChannel for ExampleChannel {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: "example",
            name_key: "notifications.example",
            description_key: None,
            icon: None,
            enabled_by_default: false,
            fields: &[
                Field { id: "token", label_key: "notifications.example_token",
                    kind: InputKind::Password, required: true, placeholder: None },
                Field { id: "recipient", label_key: "notifications.example_recipient",
                    kind: InputKind::Text, required: true, placeholder: None },
            ],
            help_links: &[],
        }
    }
    fn validate(&self, config: &ResolvedConfig) -> Result<()> {
        ExampleConfig::parse(config).map(|_| ())
    }
    fn send<'a>(&'a self, config: &'a ResolvedConfig, message: &'a Message)
        -> BoxFuture<'a, Result<Receipt>>
    {
        Box::pin(async move {
            let config = ExampleConfig::parse(config)?;
            let request = self.client.get()?.post("https://push.example.invalid/messages")
                .bearer_auth(config.token)
                .json(&serde_json::json!({
                    "recipient": config.recipient,
                    "title": message.title,
                    "body": message.plain_body(),
                }));
            let response = http::json(http::submit(request).await?).await?;
            if response["success"] != true { return Err(Code::Rejected.into()); }
            Ok(Receipt::Sent)
        })
    }
}
```

Declare `mod example;` and add
`Arc::new(example::ExampleChannel::new(client.clone()))` to the built-in list
before its final use of `client`. List order is settings-page order. IDs must be
unique, stable lowercase identifiers containing only letters, digits and `_`.
Field IDs must also be unique within a channel. Keep `enabled_by_default: false`
for new channels. Do not rename released IDs without a configuration migration.

## Fields, credentials and UI

Descriptors are the source of truth for frontend forms. Input kinds are `Text`,
`Password`, `Url` and `Numeric`. Numeric inputs remain strings, including long
recipient IDs. Password fields may contain URLs (Discord's webhook is a secret).
Every secret must be declared `Password`; regular fields are stored in JSON.
Use static translation keys in all 16 locales. Optional description/help links
are supported; icons are `monitor`, `send`, `bell`, or `None` for the fallback.

The command contract is `draft: { fields: Record<string, string> }`.
Unknown draft fields are rejected. Missing ordinary fields keep the saved value;
an explicit empty ordinary value clears it. Empty or absent passwords keep the
saved credential; use clear-channel to remove credentials. Required fields are
checked centrally, then adapter-specific validation runs. Return
`Error::new(Code::InvalidConfig).at("field_id")` for ordinary validation errors.
Never include the input value in errors. Success returns `Sent` or `Submitted`.

The settings response is `{ events, channels, results }`. Each channel combines
its descriptor, enabled/configured flags, ordinary `values` and
`secret_fields_set` (field names only). Drafts and resolved configurations cannot
be serialized back to the UI. Draft tests never save settings. Password drafts
are cleared after tests, successful saves and dialog disposal.

Version 2 stores per-channel values and credential references. Version 1 is
migrated atomically without rewriting credentials. Unknown channel entries are
retained but never executed. Unsupported file versions fail without resetting
settings. Updates stage credentials before replacing the file. Clearing commits
empty settings before best-effort vault cleanup, so missing or locked credentials
cannot prevent recovery; cleanup failures can leave unreferenced vault entries.
POSIX writes sync the parent directory after replacement. If that final sync fails,
the service reports a storage error, follows the replaced file in memory and keeps
both credential generations, avoiding dangling references after a crash.
Accepted deliveries lease their configuration until password resolution finishes;
password replacement/clearing retires old references after these reads, without
blocking settings edits or network delivery.
System artwork uses a 128-file / 32 MiB / 30-day cache budget. Recently used files
are protected for five minutes for native consumers, so bursts can temporarily
exceed the budget; the next preparation evicts old files and abandoned temporaries.
Downloads and image decoding run outside the cache lock; only cache lookups,
commits and maintenance are serialized.
The service owns all persistence and vault transactions; adapters must
not access those facilities or global task state.

## Tests and review

Put provider tests beside the adapter. Cover typed validation, escaped payloads,
required response confirmation and provider errors. Build requests without
sending them, and construct `reqwest::Response` from `http::Response` for response
tests. Helpers in `notifications::testing` provide resolved fields, sample events
and responses. Use fake credentials only; do not perform real account deliveries
in automated tests or print request URLs, bodies, secrets or raw network errors.

The shared fourth-channel fixture exercises URL/numeric fields, multiple secrets,
save/test/enable/dispatch, rollback, failure isolation and dynamic frontend forms.
`tests/fixtures/notification-channels.json` is asserted by Rust and consumed by
frontend tests, so changing the public schema requires updating both sides.

Run from the repository root:

```sh
pnpm test
pnpm i18n:check
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace
```

Check the rendered form in light/dark themes and a narrow window, including
required-field feedback, saved-password placeholders, keyboard operation,
save/clear and test previews. Existing CI runs Rust checks on Windows, macOS and
Linux. Record real native or provider delivery tests separately from mock-based
browser and automated tests.

The optional Windows smoke test uses the same localized generic test message as
the settings page. A standalone test has no UI language, so it defaults to English;
set `DQH_NOTIFICATION_TEST_LOCALE` to a supported locale (for example `zh-TW`)
when explicitly running `windows_system_smoke -- --ignored`. It submits a real
desktop notification and is skipped in normal automated test runs.

For a repeatable browser check, start `pnpm dev` and open
`/tests/browser/notification-channels.html` on that server. This development-only
harness mounts the actual settings component with the shared fourth-channel
fixture and a mock Tauri bridge. It cannot send real notifications.
With Playwright CLI available, replay its scenarios from PowerShell:

```powershell
npx --yes --package @playwright/cli playwright-cli -s=channels open http://localhost:5173/tests/browser/notification-channels.html
npx --yes --package @playwright/cli playwright-cli -s=channels run-code --filename=tests/browser/notification-channels.js
```

The replay checks generated fields, password cleanup, save/clear/enable,
field-level errors, required inputs, four preview types, keyboard focus, a narrow
dark-theme layout and the existing Telegram/Discord forms. It runs against the
development server; the harness is not included in the production build.
