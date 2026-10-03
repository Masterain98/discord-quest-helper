# Linux AppImage compatibility

## Window becomes gray after login

Issue #188's follow-up on October 3, 2026 reports that the test build renders the initial window, completes CDP login, and retrieves quests, then becomes gray with:

```text
GStreamer element autoaudiosink not found. Please install it
GLib-GObject-WARNING: invalid (NULL) pointer instance
```

Quest cards autoplay muted MP4 reward previews. WebKitGTK still needs GStreamer element factories when it initializes this media pipeline. The tested AppImage from Actions run `37097666406` contains GStreamer shared libraries but no plugin modules or plugin scanner. Thus an initially working login screen does not establish that the media path works.

The Linux Tauri configuration enables `bundle.linux.appimage.bundleMediaFramework`. All Linux build workflows install the base, good, bad, and libav plugin packages before bundling. Tauri's GStreamer bundler copies the plugins and scanner and adds an AppRun hook to select these bundled plugins rather than mixing them with a newer host installation. The package audit requires the audio autodetection, app, core, MP4 demuxer, libav, and playback modules, the scanner, and the hook. Manual test builds run this audit before staging their AppImage.

Rebuild the AppImage to apply this fix; existing downloads do not change. Installing host plugins alone may not repair an AppImage that loads an older bundled GStreamer core. The changed payload must still be tested after login on the affected CachyOS/AMD system.

## EGL failure before the first window renders

The original `EGL_BAD_PARAMETER` report describes a separate earlier failure. Keep the upgraded Tauri CLI and the package audit that rejects bundled `libwayland-client.so*`; AppImages must use the host's Wayland client alongside the host Mesa/EGL stack. The media fix preserves this check.

## Build and audit

On the Ubuntu build host, install the normal Tauri Linux build dependencies and:

```bash
sudo apt install gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad gstreamer1.0-libav
pnpm run tauri:build --bundles deb,appimage
node scripts/audit-packaged-identity.mjs --platform linux \
  --artifact "target/release/bundle/appimage/Discord Quest Helper_0.10.7_amd64.AppImage"
```

See the [Tauri AppImage media documentation](https://tauri.app/distribute/appimage/#gstreamer-media-framework-support) and [Issue #188](https://github.com/Masterain98/discord-quest-helper/issues/188#issuecomment-5967091575).
