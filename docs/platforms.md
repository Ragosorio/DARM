# Platforms

Where the DARM reader runs, how it is packaged, and — separately — what has
actually been built and run. Last updated 2026-11-16.

The reader is one web application (`apps/reader`). It ships in two forms:

- **Web / PWA**: the static build in `apps/reader/dist`, served from any
  https origin.
- **Native shell**: the same build inside a Tauri 2 webview
  (`apps/desktop`), for macOS, Windows, Linux, Android and iOS.

## Words used in this document

| Word | Meaning |
|---|---|
| **verified** | Run on a real machine, with the outcome observed. The section "What was verified, and how" lists every such check with its command. |
| **built** | An artifact was produced, but not exercised beyond what is stated. |
| **configured** | Source and configuration exist; nothing was built or run. Expect to fix things on the first build. |
| **not implemented** | No code exists. |

Everything verified so far was verified on **one machine**: Apple Silicon,
macOS 26.5.2, Xcode 26.6, Rust 1.99.1, Node 22.23.4. No Windows, Linux,
Android or iOS build has ever been produced from this repository. The CI
workflow (`.github/workflows/ci.yml`) that would produce them has never been
executed.

## Support matrix

| Platform | Shell | Status | Built here | Run here |
|---|---|---|---|---|
| macOS (Apple Silicon) | Tauri 2, WKWebView | implemented | yes: `DARM.app`, ad-hoc signed | yes, see below |
| macOS (Intel / universal) | same | configured | no | no |
| Windows 10/11 | Tauri 2, WebView2 | configured | no | no |
| Linux (deb, rpm, AppImage) | Tauri 2, WebKitGTK 4.1 | configured | no | no |
| Android | Tauri 2, Android WebView | configured, **blocked** (no SDK/NDK); "Open with" **not implemented** | no | no |
| iOS / iPadOS | Tauri 2, WKWebView | configured, **blocked** (no Rust targets, XcodeGen, simulator runtime, signing team) | no | no |
| Web: Chromium (Chrome, Edge) | none | implemented | yes (`vite build`) | service worker only, on localhost |
| Web: Safari | none | implemented | same build | service worker only, in Playwright's WebKit, on localhost |
| Web: Firefox | none | implemented | same build | service worker only, on localhost |

### Minimum versions

These follow from the features the reader's code uses (WebAssembly, ES2022
modules, CSS `color-mix()`, `OffscreenCanvas`). **None of the lower bounds has
been tested**; the only engine versions that have run the reader are listed in
the verification section.

| Platform | Configured or derived minimum | Where it is set |
|---|---|---|
| macOS | 13.3 (ships WebKit 16.4) | `bundle.macOS.minimumSystemVersion` |
| Windows | 10 or 11 with the WebView2 Evergreen runtime (the installer downloads it if missing) | Tauri default |
| Linux | a distribution with WebKitGTK 4.1 (Ubuntu 22.04+, Debian 12+, Fedora 37+) | Tauri 2 requirement |
| Android | 7.0 (API 24), with Android System WebView 111+ | `bundle.android.minSdkVersion` |
| iOS / iPadOS | 16.4 | `bundle.iOS.minimumSystemVersion` |
| Chrome / Edge | 111 | derived |
| Safari | 16.4 | derived |
| Firefox | 113 | derived |

WebAssembly is required everywhere; without it the reader shows an error and
opens nothing. WebGL 2 is optional: the reader falls back to a Canvas 2D
renderer when it is unavailable (common in WebKitGTK without GPU drivers and in
virtual machines).

## How a book reaches the app

| Route | macOS | Windows | Linux | Android | iOS | Web / PWA |
|---|---|---|---|---|---|---|
| Double-click a `.darm` file | implemented; the association is declared in `Info.plist` (**verified**) but a Finder double-click was **not** tried, because the app is not installed in `/Applications` | configured | configured | n/a | n/a | Chromium desktop only, and only after the PWA is installed (`file_handlers`); **not verified** |
| "Open with" / `open -a` | **verified** (`open -a DARM.app book.darm`, cold and into a running instance) | configured (Explorer passes the path as an argument) | configured (`.desktop` `MimeType`) | **not implemented** (see Android) | configured (document type + `Info.ios.plist`) | same as above |
| Command-line argument | **verified** (`DARM.app/Contents/MacOS/darm-desktop book.darm`) | configured | configured | n/a | n/a | n/a |
| Second launch forwards to the running app | **verified** (single-instance plugin) | configured | configured | n/a | n/a | `launch_handler: focus-existing`, **not verified** |
| File → Open… (Cmd/Ctrl+O), native dialog | implemented, **not verified** | configured | configured | no menu | no menu | n/a |
| "Add a book" button (`<input type="file">`) | reader feature; **not verified** inside the shell | configured | configured | configured | configured | reader feature, covered by the reader's own tests |
| Drag and drop onto the window | reader feature; the shell leaves HTML5 drops alone (`dragDropEnabled: false`); **not verified** inside the shell | configured | configured | n/a | n/a | reader feature, covered by the reader's own tests |
| Share sheet / "Open in DARM" | n/a | n/a | n/a | **not implemented** | configured | no (`share_target` is not declared) |

The web cannot register a file type universally. `file_handlers` is honoured
only by Chromium-based desktop browsers (Chrome, Edge) for an *installed* PWA;
Safari, Firefox and every mobile browser ignore it. Everywhere else the file
picker and drag-and-drop are the only routes.

## What was verified, and how

All on the machine described above, on 2026-11-16.

### Desktop shell on macOS

Built with `scripts/build-desktop.sh --skip-reader --skip-install -- --bundles app`
(only the `.app` bundle; no `.dmg` was produced). The checks were run twice:
first against a small stub page that uses the reader's real glue module
(`apps/reader/src/shell/tauri.ts`), then against the real reader build.

| Check | How | Outcome |
|---|---|---|
| The bundle exists and is consistent | `codesign --verify --deep --strict DARM.app` | passes; signature is ad-hoc (`flags=adhoc,runtime`), no team identifier |
| `.darm` document type | `plutil -p DARM.app/Contents/Info.plist` | `CFBundleDocumentTypes` (extension `darm`, role Viewer, rank Owner, `org.darm.book`) and `UTExportedTypeDeclarations` (`application/vnd.darm`, conforms to `public.data`) are present |
| Launch with a file | `open -n --stderr log --env DARM_SHELL_TRACE=1 -a DARM.app apps/reader/tests/fixtures/kitchen-sink.darm` | the shell logs `accepted from the operating system: … (71498 bytes)`; the webview drains the queue, reads the bytes and releases the token |
| A second file reaches the same instance | `open -a DARM.app other.darm` | same process id logs the second file |
| Bytes arrive intact in JavaScript | stub computes a hash of the received bytes and reports it | equal to the hash computed from the file on disk, for 71 KiB, 241 KiB and a 9 MiB file read as 4 MiB + 4 MiB + 1 MiB chunks |
| Second launch forwards its arguments | run the bundle's binary directly with paths while the app is running | exits 0; the running instance logs `a second launch forwarded 5 candidate path(s)` |
| The registry refuses what it should | same run: a `.txt` file, a directory named `x.darm`, a symlink `x.darm` → `/etc/hosts`, a missing file | each is logged as rejected with its reason; only the real `.darm` is accepted |
| Cold start with a path argument | `DARM.app/Contents/MacOS/darm-desktop book.darm notes.txt` | book accepted, `notes.txt` rejected |
| The webview has no other powers | stub invokes `plugin:fs\|read_file`, `plugin:dialog\|open`, `plugin:opener\|open_url` | all three refused by Tauri's access control |
| CSP is enforced | stub fetches another origin; compiles a WebAssembly module | fetch blocked (the local test server sees no request); WebAssembly compiles |
| External links leave the app | stub calls `window.open(url, "_blank", "noopener,noreferrer")` | `window.open` returns `null`; the URL is requested by the system browser; no second webview appears |
| The webview cannot be navigated away | stub sets `location.href` to another origin | the shell logs `blocked navigation away from the reader`; the page is still running afterwards |
| The real reader opens a book handed over by the OS | real build, with the change to `src/main.ts` described under "Wiring" applied at build time | Kitchen Sink (12 pages) and Atlas de mareas (60 pages) open, rendered with WebGL 2; confirmed in a window capture |
| Unit tests | `cargo test --release --features tauri/custom-protocol` in `apps/desktop/src-tauri` | 27 pass (21 registry, 6 URL policy) |
| Lints | `cargo fmt --check`, `cargo clippy --release --features tauri/custom-protocol --all-targets -- -D warnings` | clean |

Not verified on macOS: Finder double-click, File → Open…, drag and drop into
the shell, the "Add a book" picker inside the shell, the `.dmg`, an Intel or
universal build, `tauri dev`, any real signing identity, notarization.

`apps/reader/src/main.ts` now contains the wiring described under "Wiring the
reader to the shell", so a build from the repository opens files handed over
by the OS. The `.app` that was launched for the checks above was built with
that same change applied at build time; the app has not been rebuilt and
launched again since the change was committed to `main.ts`.

### Web / PWA

| Check | How | Outcome |
|---|---|---|
| Manifest is valid JSON and its icons exist | `node` | ok |
| Service worker parses | `node --check apps/reader/public/sw.js` | ok |
| Precache, runtime rules, bypass rules, cache replacement | scripted in Chrome 157, Firefox 159 and Playwright WebKit 27.2 against a synthetic site on `http://127.0.0.1` | first visit caches every listed file; offline reload works, including `index.html?query`; `/__darm/*`, `*.darm`, Range, POST and unlisted URLs are neither served from nor written to the cache; a new `precache.json` produces a new cache and removes the old one |
| The real reader's shell offline | same three engines, real `vite build` output served from a scratch copy on `http://127.0.0.1` | after one online visit and with the server stopped, the library screen renders and the WebAssembly core compiles; all 42 precache entries were cached |
| Offline reload followed by opening a book | `apps/reader/e2e/offline.spec.ts`, against `vite preview` on `http://127.0.0.1:4173` | covered by the reader's Playwright suite, **on localhost only** |

Offline behaviour has never been verified on a deployed https origin. Not
verified at all: installing the PWA, `file_handlers`, `launch_handler`, the
maskable icons on a real launcher, Safari "Add to Dock", iOS "Add to Home
Screen".

## Desktop shell

Source: `apps/desktop`. Versions (pinned; lockfiles committed):

| Component | Version |
|---|---|
| `@tauri-apps/cli` (npm) | 2.12.5 |
| `tauri` | 2.12.6 |
| `tauri-build` | 2.7.5 |
| `tauri-plugin-single-instance` | 2.5.5 |
| `tauri-plugin-dialog` | 2.8.4 |
| `tauri-plugin-opener` | 2.7.3 |
| resolved: `wry` 0.57.2, `tao` 0.37.2 | |

### Security model

The webview must never be able to read an arbitrary path.

- **File registry** (`src-tauri/src/registry.rs`). Files that the operating
  system or the native Open dialog hands to the app are registered as
  `random token → open file handle`. Only regular files whose *canonical* path
  ends in `.darm` are accepted. Tokens are 128 bits from the OS random
  generator. At most 64 files are held at once.
- **Three commands**, and nothing else, are granted to the webview
  (`src-tauri/capabilities/main.json`): `darm_take_pending`,
  `darm_read_chunk(token, offset, len)` (at most 8 MiB, bounds-checked, binary
  response) and `darm_release(token)`, plus listening to events. The `fs`,
  `dialog` and `opener` plugins are compiled in but none of their commands is
  granted; dialog and opener are used from Rust only.
- **Navigation**: the main webview may only load the app's own origin.
  `window.open` never creates a webview: `http:`, `https:` and `mailto:` URLs
  are handed to the system, everything else is dropped.
- **Single instance**: on macOS and Linux the plugin listens on a local socket
  (`/tmp/org_darm_reader_si.sock` on macOS). Another local user can ask the
  running app to *register* a path; the registry rules above still apply and
  the bytes only ever go to the reader's own window.

### Content-Security-Policy

Set in `tauri.conf.json` (`app.security.csp`):

```
default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline';
img-src 'self' blob: data:; media-src 'self' blob: https:; font-src 'self' data: blob:;
connect-src 'self' ipc: http://ipc.localhost; worker-src 'self' blob:;
object-src 'none'; base-uri 'none'; form-action 'none'
```

- `connect-src` lists both IPC origins because Tauri uses a different one per
  platform: the custom scheme `ipc://localhost` on macOS, Linux and iOS, and
  `http://ipc.localhost` on Windows, where WebView2 cannot serve custom
  schemes the same way. (Android uses the `http://` form for its protocols
  too, but sends IPC through `postMessage`, which CSP does not govern.)
  `'self'` is `tauri://localhost` on macOS, Linux and iOS and
  `http://tauri.localhost` on Windows and Android. Only the macOS case has
  been run.
- `dangerousDisableAssetCspModification: ["style-src"]` stops Tauri from
  adding a nonce to `style-src`. With a nonce present, browsers ignore
  `'unsafe-inline'`, which would break the reader's inline style attributes.
  Script handling is left to Tauri.
- The reader's own build also injects the same policy as a `<meta>` tag
  (`apps/reader/vite.config.ts`). Both apply; they are identical.
- `devCsp` (used by `tauri dev`) additionally allows the Vite dev server's
  websocket and inline scripts. `tauri dev` has **not** been run.

### Wiring the reader to the shell

`apps/reader/src/shell/tauri.ts` exports `initNativeShell(open)`. It resolves
to `false` in a browser. Inside the shell it drains the file queue, reads each
file in 4 MiB chunks, builds a `File` and calls `open(file)`, then releases
the token. `apps/reader/src/main.ts` calls it:

```ts
import { initNativeShell } from "./shell/tauri";
app.start().then(() => initNativeShell((file) => app.importFiles([file]))).catch((e) => {
```

It has to run after `app.start()` resolves: `start()` ends by showing the
library, which would close a book opened earlier.

### Diagnostics

The shell logs one line per file it accepts or rejects to stderr.
`DARM_SHELL_LOG=<path>` also appends to a file (the only way to see the log of
a release build on Windows). `DARM_SHELL_TRACE=1` adds one line per webview
command, numbers only. On macOS:

```sh
open -g -n --stderr /tmp/darm.log --env DARM_SHELL_TRACE=1 -a path/to/DARM.app book.darm
```

(`-g` keeps the app in the background; without it the new window takes
keyboard focus.)

## Building, signing and packaging

All scripts run from any directory and refuse to start with less than 2 GiB
of free disk space (`DARM_MIN_FREE_GIB` overrides). A release build of the
shell needs about 1.3 GiB in `apps/desktop/src-tauri/target`.

```sh
scripts/build-reader.sh                  # wasm core + npm ci + vite build -> apps/reader/dist
scripts/build-desktop.sh                 # the above + tauri build for this platform
scripts/build-desktop.sh --skip-reader -- --bundles app     # reuse dist; macOS .app only
```

Output: `apps/desktop/src-tauri/target/release/bundle/`. Nothing below about
signing has been exercised; the environment variable names are the ones Tauri
2 documents.

### macOS

Prerequisites: Xcode, Rust, Node, `wasm-bindgen-cli` 0.2.130.

```sh
scripts/build-desktop.sh -- --bundles app,dmg
# universal binary (needs: rustup target add x86_64-apple-darwin aarch64-apple-darwin)
scripts/build-desktop.sh -- --target universal-apple-darwin --bundles app,dmg
```

By default the bundle gets an **ad-hoc** signature
(`bundle.macOS.signingIdentity` is `"-"`). That is enough to run on the
machine that built it; downloaded to another Mac, Gatekeeper will refuse it.
For distribution you need an Apple Developer account and a *Developer ID
Application* certificate:

```sh
export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"   # overrides the "-" in the config
# notarization, either with an Apple ID…
export APPLE_ID="you@example.com" APPLE_PASSWORD="app-specific-password" APPLE_TEAM_ID="TEAMID"
# …or with an App Store Connect API key
export APPLE_API_ISSUER="issuer-uuid" APPLE_API_KEY="KEYID" APPLE_API_KEY_PATH="/path/AuthKey_KEYID.p8"
scripts/build-desktop.sh -- --bundles app,dmg      # signs, notarizes and staples

codesign --verify --deep --strict --verbose=2 DARM.app
spctl --assess --type execute --verbose DARM.app
xcrun stapler validate DARM.app
```

In CI, import the certificate with `APPLE_CERTIFICATE` (base64 `.p12`),
`APPLE_CERTIFICATE_PASSWORD` and `KEYCHAIN_PASSWORD`. By hand, the equivalent
is `codesign --force --options runtime --timestamp --sign "$APPLE_SIGNING_IDENTITY" DARM.app`,
`xcrun notarytool submit DARM.dmg --wait …`, `xcrun stapler staple DARM.dmg`.

### Windows — configured, never built

Build on Windows (Rust MSVC toolchain, Node, Git Bash for the scripts):

```sh
scripts/build-desktop.sh -- --bundles msi,nsis
```

Tauri downloads WiX (MSI) and NSIS on first use. The `.darm` association is
written by both installers from `bundle.fileAssociations`. Authenticode:
either set `bundle.windows.certificateThumbprint`, `digestAlgorithm`
(`"sha256"`) and `timestampUrl` in `tauri.conf.json` with the certificate in
the Windows certificate store, or set `bundle.windows.signCommand` for a
cloud or hardware key (Azure Trusted Signing, an HSM). By hand:

```bat
signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /sha1 <thumbprint> DARM_0.1.0_x64-setup.exe
signtool verify /pa /v DARM_0.1.0_x64-setup.exe
```

Unsigned installers trigger SmartScreen warnings.

### Linux — configured, never built

```sh
sudo apt-get install -y libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
scripts/build-desktop.sh -- --bundles deb,rpm,appimage
```

The `.deb` and `.rpm` install `linux/org.darm.reader.xml` into
`/usr/share/mime/packages`, which teaches the desktop that `*.darm` is
`application/vnd.darm`; the generated `.desktop` file declares that MIME type.
An AppImage cannot register a MIME type by itself: without a helper such as
AppImageLauncher, "Open with" has to be set by hand. Signing: AppImage with
`SIGN=1 SIGN_KEY=<gpg key id> APPIMAGETOOL_SIGN_PASSPHRASE=…`; RPM with
`TAURI_SIGNING_RPM_KEY` and `TAURI_SIGNING_RPM_KEY_PASSPHRASE`; a `.deb` is
normally signed at the repository level (`Release.gpg`), not per package.

### Android — configured, not built, not validated

**Blocker on the machine used so far**: no Android SDK or NDK
(`~/Library/Android` does not exist, `ANDROID_HOME` and `NDK_HOME` are unset,
no Rust Android targets). `npx tauri android init --skip-targets-install --ci`
was run and stops with: *"Android SDK not found. Make sure the SDK and NDK are
installed and the ANDROID_HOME and NDK_HOME environment variables are set."*
A JDK 17 is present. Nothing was installed.

Steps (from the Tauri 2 prerequisites; untested here beyond the failed `init`):

```sh
# Android Studio > SDK Manager: Android SDK Platform, Platform-Tools,
# NDK (Side by side), Build-Tools, Command-line Tools
export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"
export ANDROID_HOME="$HOME/Library/Android/sdk"
export NDK_HOME="$ANDROID_HOME/ndk/$(ls -1 "$ANDROID_HOME/ndk")"
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android

scripts/build-reader.sh
cd apps/desktop
npx tauri android init                 # generates src-tauri/gen/android
npx tauri android dev                  # emulator or device
npx tauri android build --apk          # or --aab for Google Play
```

What the configuration already provides: `bundle.fileAssociations` (Tauri
turns it into `VIEW`/`SEND` intent filters for `application/vnd.darm` when it
generates the manifest), `minSdkVersion` 24, launcher icons in
`src-tauri/icons/android`.

What is missing:

- **"Open with" and the share sheet do not work yet.** Android hands the app a
  `content://` URI, not a path. The shell receives it (`RunEvent::Opened`) and
  logs that it is ignored. Resolving it (for example by opening the URI
  through `tauri-plugin-fs` and registering the resulting handle) is not
  written. Until then the only route on Android is the reader's own file
  picker.
- External links: Tauri's `window.open` hook is not available on mobile, so
  what happens to an external link on Android is unknown.
- Tauri's IPC on Android does not use the custom protocol; binary chunks may
  arrive as JSON arrays. The glue accepts that, but large books will import
  slowly.
- Signing: create a keystore
  (`keytool -genkey -v -keystore ~/upload-keystore.jks -keyalg RSA -keysize 2048 -validity 10000 -alias upload`),
  write `src-tauri/gen/android/keystore.properties` (`password`, `keyAlias`,
  `storeFile`) and add the `signingConfigs` block to
  `src-tauri/gen/android/app/build.gradle.kts` as described in the Tauri
  Android signing guide. `src-tauri/gen/` is git-ignored in this repository,
  so those edits must be re-applied after every `android init` (or the ignore
  rule changed). Publishing needs a Google Play developer account.

### iOS / iPadOS — configured, not built, not validated

State of the machine used so far: `xcodebuild -version` works (Xcode 26.6),
`xcodebuild -license check` and `xcodebuild -checkFirstLaunchStatus` both
exit 0, and the iOS 26.5 SDK is installed. **Blockers**:

- no Rust iOS targets (`rustup target list --installed` shows only macOS,
  Windows and wasm32);
- no XcodeGen and no libimobiledevice, which `tauri ios init` needs (it would
  install them with Homebrew, so it was deliberately not run);
- no simulator runtime: `xcrun simctl list runtimes` is empty, so no
  simulator can boot (the runtime is a multi-gigabyte download);
- no Apple development team or provisioning profile.

CocoaPods 1.17.0 is present. Nothing was installed.

Steps (untested here):

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
brew install cocoapods xcodegen libimobiledevice
xcodebuild -downloadPlatform iOS            # simulator runtime; or Xcode > Settings > Components
export APPLE_DEVELOPMENT_TEAM="TEAMID"      # or bundle.iOS.developmentTeam in tauri.conf.json

scripts/build-reader.sh
cd apps/desktop
npx tauri ios init                          # generates src-tauri/gen/apple
npx tauri ios dev "iPhone 17"
npx tauri ios build --export-method app-store-connect     # or release-testing, debugging
```

What the configuration already provides: `bundle.fileAssociations` (document
type `org.darm.book`, exported with the `.darm` extension and
`application/vnd.darm`), `src-tauri/Info.ios.plist` (merged into the generated
`Info.plist`; sets `LSSupportsOpeningDocumentsInPlace` to `false` so that
"Open in DARM" and the share sheet give the app a private copy with a plain
file URL), deployment target 16.4, icons in `src-tauri/icons/ios`. The shell's
`RunEvent::Opened` handler is compiled for iOS and feeds the same registry.
The reader already pads for `env(safe-area-inset-*)`.

Known gaps, all unverified: copies that iOS places in the app's
`Documents/Inbox` are never deleted; external links (no `window.open` hook on
mobile); the export-compliance answer (`ITSAppUsesNonExemptEncryption`) is
left to the publisher because DARM opens encrypted books. Signing: automatic
with an Apple account signed in to Xcode; in CI either `APPLE_API_ISSUER`,
`APPLE_API_KEY`, `APPLE_API_KEY_PATH` (automatic) or `IOS_CERTIFICATE`,
`IOS_CERTIFICATE_PASSWORD`, `IOS_MOBILE_PROVISION` (manual, base64). An Apple
Developer Program membership is required to run on a device or publish.

## Web / PWA

`apps/reader/public` holds the static files of the installable web app.

- `manifest.webmanifest`: name, standalone display, colours matching the
  reader's light theme (`#e4dfd5`), icons (192, 512, maskable 192/512, SVG),
  `file_handlers` for `.darm` and `launch_handler: focus-existing`. The reader
  consumes `window.launchQueue`.
- `sw.js`: precaches the whole shell on install from `precache.json` (written
  by the production build), then serves hashed `assets/` cache-first and
  navigations network-first with the cached `index.html` as fallback. It
  never touches `/__darm/*`, `*.darm`, Range requests, non-GET or cross-origin
  requests, and never stores anything that is not in the precache list. The
  cache is named after a hash of the list; a new build replaces it.
- The reader registers the worker only in production builds in a secure
  context, and not for `?book=` previews.

Deploy `apps/reader/dist` to any static https host. The server must send
`application/wasm` for `.wasm` and should send `Cache-Control: no-cache` for
`index.html`, `sw.js` and `precache.json`.

Limitations: no universal file association (see above); iOS Safari may evict
site storage, including the library, after weeks without use unless the app is
added to the Home Screen; Firefox desktop cannot install PWAs.

## Icons

One mark (an open book with a turning leaf) in three SVG sources:
`apps/reader/public/icon.svg`, `apps/reader/public/icon-maskable.svg` and
`apps/desktop/icon-source/app-icon.svg` (Apple's 1024 grid).
`apps/desktop/icon-source/rasterize.sh` regenerates every PNG and the platform
set in `apps/desktop/src-tauri/icons` (needs Google Chrome and macOS `sips`).

## Continuous integration

`.github/workflows/ci.yml` defines: `rust` (fmt, clippy, tests on Ubuntu,
macOS, Windows), `wasm`, `reader` (typecheck, vitest, build, Playwright),
`examples`, `desktop` (Tauri bundles on macOS, Windows, Ubuntu 22.04, uploaded
unsigned), `audit` (non-blocking) and `fuzz-smoke` (60 s per target on
nightly).

**It has never been executed.** The YAML parses, and the `examples` step's
script was run locally (14 books compile and verify). Known risks for the
first run: the visual baselines in `apps/reader/e2e/snapshots` were recorded
on macOS and the `reader` job runs on Ubuntu; the Windows and Linux desktop
jobs will be the first builds of the shell on those systems.
