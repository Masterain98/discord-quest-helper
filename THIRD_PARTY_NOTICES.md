# Third-Party Notices

This project is MIT-licensed. It bundles and uses the following third-party
software and services, which remain the property of their respective owners.

## proton-confgen (bundled binary)

- **License:** GPL-3.0. The full license text is in
  [`tools/proton-confgen/LICENSE`](tools/proton-confgen/LICENSE) and the
  complete source code is in [`tools/proton-confgen/`](tools/proton-confgen/).
- **What it does:** a command-line generator that logs into a ProtonVPN
  account (SRP) and produces a WireGuard profile for the free server pool.
- **How it ships:** a Windows build is vendored at
  `src-tauri/binaries/proton-confgen.exe` and embedded into the app. At runtime
  the helper verifies the binary's SHA-256 before installing and executing it.
  Your ProtonVPN credentials are passed to it locally through stdin; they are
  never sent anywhere except the ProtonVPN API by the generator itself.

## WireSock VPN Client (not bundled)

- WireSock Secure Connect / WireSock CLI is proprietary software by NTKERNEL.
  It is **not** distributed with this app. The helper can install it for you
  through `winget` (with a UAC prompt) or link to the vendor's download page,
  and only launches a locally installed copy with the configuration you
  provide.

## External services (runtime requests, no bundled code)

- **Discord** — authentication, quests, CDP, and RPC traffic.
- **discordquest.com** — the Global Quests public catalog.
- **ProtonVPN API** — only when you use the optional Proton profile generator.
- **GitHub** — release/update checks.
