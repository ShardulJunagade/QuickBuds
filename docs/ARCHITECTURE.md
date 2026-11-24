# Architecture

QuickBuds has four parts. This file shows how they connect and why. The wire format is in
[OPPO-PROTOCOL.md](./OPPO-PROTOCOL.md). The setting engine is in [FEATURE-SYNC.md](./FEATURE-SYNC.md).

## The parts

| Part | Folder | Language | Role |
|---|---|---|---|
| Core | `core/` | Rust | Frames, typed messages, commands, the setting engine, the model data and the OPPO driver. No I/O. One dependency: `serde_json`. |
| Android app | `app/` | Kotlin | Phone UI, Bluetooth link, service, widgets, tiles, notification. |
| Desktop app | `desktop-tauri/` | Rust, web UI | Windows and Linux. The link thread (`desktop-tauri/src/session.rs`) runs the core's driver. The Tauri window shows the UI from `desktop-tauri/ui/`. |
| Report Worker | `report-worker/` | JavaScript | Cloudflare Worker: problem reports, the daily count, the update check, the Discord digest. |

The core has no Bluetooth code. Each platform shell owns the RFCOMM socket and passes bytes in and out.
This keeps the core testable without a device.

The OPPO driver (`core/src/oppo/driver.rs`) holds the whole protocol of one link: the init sequence, the replies and
pushes, the UI's commands and the setting engine. The shell gives it three inputs: the bytes it reads
(`on_bytes`), a UI command (`command`) and the clock (`tick`). The driver returns a list of `Effect`s: frames to
write, log lines, a changed `Snapshot`, settings to store. A wait between two frames is a step in the driver's
queue, so the driver never blocks. The shell calls `tick` at `next_wake` or sooner. Why: the same driver can run
behind a JNI call on Android, and every brand gets a driver of this shape (docs/BRANDS.md).
The driver runs one UI command at a time. Commands that arrive during a command's waits go in one batch, and only
the last of several quick EQ picks is sent (`core/src/oppo/eq_logic.rs`).

The Android app runs its own Kotlin copy of the rules (`app/.../protocol/`: `Wire.kt`, `Commands.kt`, `Messages.kt`,
`FeatureSync.kt`). A protocol change updates both copies and OPPO-PROTOCOL.md.
The app also carries the core as a native library (`core-jni/`, `libbuds_core_jni.so`) in shadow mode
(`protocol/RustShadow.kt`). The read thread gives the core the same bytes as the Kotlin layer. The core's frames go
nowhere. 2 s after the last bytes, the app compares the core's shared state (`core/src/model.rs`) with its own:
the shared state, the EQ, the switch values, the gestures, the devices and every other value the Kotlin layer keeps.
A new difference goes to the log as one `RUST DIFF:` line. 10 s after the link comes up, the app also compares the
commands of the init sequence: the core's would-be frames against its own (`RUST TX DIFF:`).
Why: 90 % of the users are on Android, and the Kotlin layer knows more than the core. The report logs show each gap
on real buds before the core takes over, and users see no change. A panic in the core is caught in the bridge.
Driver mode (the default; Dev tools › Rust engine turns it off, read on each connect): the core drives the link
(`protocol/RustDriver.kt`). `BudsConnectionManager` stays the shell: it owns the socket, gives the core the bytes and
the UI commands (JSON), writes the core's frames and maps the core's state JSON to its fields and listeners
(`applyRust`). All core calls run on one thread. The core reads the battery every 5 min, reads it on the lid-close
push, and gives find phone as an effect. Shadow mode is off while driver mode runs.
Why a switch: the Kotlin layer stays as the fallback while beta users test the core on their buds.
The connect picks the brand first (`ConnectPolicy.brand`: the service list, then the name). Soundcore buds always
run in driver mode, with the Soundcore driver (`core/src/soundcore/driver.rs`), because no Kotlin copy exists.
The connect saves the brand (`protocol/Soundcore.kt`). On a Soundcore link, `ModelCatalog.current` is null, and
`Rows`, `AncModes`, reports and the model test read the Soundcore state instead. The Soundcore driver keeps the raw
state packet and gives every setting as one JSON object (`state_json`, the `sc` key of the bridge state).
Each model is a table in `core/src/soundcore/models.rs`: the state packet field by field, the buttons, the sound
modes, the equalizer write and the settings. The driver adds the field lengths up for the offsets.
Why a table: OpenSCQ30 has 36 model files with the same parts in other orders; one generic driver and a table per
model keep each model to about 20 lines, and a test parses every recorded packet (`demo.rs`) with its table.
`applyRust` saves it and calls `onSoundcore`. The 4 Soundcore screens (`ui/Soundcore*Activity.kt`) repaint from it
and write with `soundcoreSet` (bridge command `sc`). Wire format: docs/SOUNDCORE-PROTOCOL.md.
The desktop app calls the core directly (`desktop-tauri/src/main.rs` re-exports `cmd`, `msg`, `wire` and the OPPO model data in `core/src/oppo/`).

Both apps decode a frame in one place. Android: `Decode.decode`, then `BudsConnectionManager.apply`. Desktop: `buds_core::msg::decode`, mapped to the session's `Event` in `core/src/oppo/protocol.rs`.

One setting does not go to the buds. Buds whose bitmap lacks `0xEF03` cannot make 3D audio; the phone does.
`app/.../bluetooth/PhoneSpatial.kt` reads the Android `Spatializer` and writes an OPLUS audio parameter. Only OPPO,
OnePlus and realme phones have that spatializer, so the row shows only there. Why: the switch on the buds plays
nothing on other phones (OPPO-PROTOCOL.md section 18). The desktop app hides the row.
Phone-made 3D audio and high-quality audio exclude each other, as on the buds: one turns the other off after a dialog.

One request goes the other way: the buds ask the phone to ring. realme buds with the `findPhone` flag send `0x0502 05`
on a gesture. `app/.../bluetooth/PhoneRinger.kt` plays the default ringtone on the alarm stream and posts a
notification with a Stop action. Why: the ring needs a phone, so the desktop app has no Find phone row.

A connect tries `079A`, then `1107` (`ConnectPolicy.connectUuids`, `desktop-tauri/src/bt.rs`). Plain SPP `1101` comes last,
and only when the buds' service list has neither. Why: some realme buds carry the protocol only on `1101`, while on
OnePlus and OPPO buds `1101` carries other frames (OPPO-PROTOCOL.md section 1).

A second link is a fallback. When another app holds `079A` (every try fails fast with `read ret -1`) and the buds' SDP
lists the Fast Pair Message Stream, `app/.../bluetooth/FastPairBattery.kt` opens that stream and passes its battery
messages (`protocol/FastPairStream.kt`) to the same listeners. A new connect or a Disconnect closes it. Why: on a
OnePlus phone the system holds `079A` and cannot be turned off (issue #18); the stream is the only other channel. A missing or `0` case level beside the normal link starts a 10 s BLE scan, at most once every 5 min
(`bluetooth/AdvertCaseScan.kt`, `protocol/FastPairAdvert.kt`). Why: Google Play Services holds the stream, but the
advertisement is free to read. Was: a short stream read beside the link. Is: the scan only. The desktop app has no fallback.
With no fresh scan level, a `0` case level shows the last real level of these buds (kept per address). The core's driver keeps the same rule (`Settings::case_level`); the desktop keeps the level per address in `settings.json`. The Android app gives the core the scanned level too (`case_stand_in`), with the same 5 min rule. Why: the buds
report `0` while no bud sits in the open case (issue #47).

## The model data

`app/src/main/assets/models.json` lists every model. Each entry has the id, the name, the commands the buds accept,
and per-model flags (for example `seqless`, `gameConfirm`, `noFind`). Code reads an entry only through
`protocol/ModelProfile.kt` and its Rust twin `core/src/oppo/profile.rs`. A new key goes there once. Code never
tests a model name. A new model needs only a new entry ([ADD-A-MODEL.md](./ADD-A-MODEL.md)). The Worker imports the
same file to accept a report only for a known model.

## From a button to the buds and back

1. The user taps a control. The UI calls `setFeature(id, on)` on the connection manager (Android) or sends a
   `Cmd` to the link thread (desktop). The screens, the widgets and the tiles take this one path. The manager picks
   the confirmation, the restart and the coupled switch (3D audio and high-quality audio).
2. The engine in `FeatureSync` takes the operation. It shows "Applying" and sends the first write.
   The control does not move.
3. The buds ack each write with a status. The next write goes out after status 0.
4. The engine confirms the setting by one of three ways: a status read, a push from the buds, or the ack alone.
   The model entry in `models.json` can change the way.
5. The buds' report decides. The engine emits `changed`, and the UI shows the new value.
   A refusal or a missing answer shows a toast that names the setting.

The buds are the only source of truth. The app is a remote control.

## Main flows

### Connect (Android)

1. `KeepAliveReceiver` sees an audio profile connect. `BudsDevice` picks the paired device that is the buds: a model
   picked by hand, else earbuds connected now, else the address saved last. A smart band or a watch is not earbuds.
2. `BudsConnectionManager.connect` opens the RFCOMM socket with the OPPO SPP UUID.
3. `onLinkUp` starts the init sequence on its own thread: handshake, product id, broadcast codes, then the
   notification registration.
4. Reads follow, and only for commands that the model lists (`Capabilities`): status, ANC, battery, wearing, gestures,
   spatial type, codec, game sound. Models with the batch command get these reads in one frame.
5. Replies arrive as frames. `Decode` and `Messages` turn them into typed values. The manager tells its listeners:
   `BudsService` (notification, chip), the activities, the widgets and the tiles.
   The Android manager reads the battery again every 5 min while the link is up, and when the lid closes.
   Some buds push no case change (issue #47). The desktop does not do this yet.
   A connect clears the stored wear state. The wear query or the next wear event sets it, because some buds
   (T300) have no wear query and report wear only on a change.
6. A link that drops without our request reconnects with growing delays. The service parks when the link is idle.
   A locked screen puts the service to sleep: it closes the link and keeps the last levels. The unlock reconnects.
   Why: a held link costs battery. Smart pause keeps the link up. Sleep, wake and park go to the report log.

The desktop link thread (`desktop-tauri/src/session.rs`) runs the core's driver, which has the same sequence and the same rules.

### A setting change

See "From a button to the buds and back". A write that restarts the link (Hi-Res, power saving, codec)
waits for the first report after the reconnect.

### A problem report

1. The user fills the form (`ReportActivity` on Android, the Feedback page on desktop).
2. `ProblemReport` builds the log: a header, the packet log and the app's own warnings. It removes Bluetooth
   addresses and device names before the log leaves the phone.
3. The app posts JSON to `/report` on the Worker.
4. The Worker checks the model, the description and the log, and limits one sender to 5 reports an hour.
   It stores the report in D1.
5. A cron runs every 15 minutes. It puts each new report on a queue. The queue consumer posts one Discord message
   for each report.
6. The developer reads the report on the Worker page with the read key and sets its status in D1.

### The desktop tray panel

A click on the tray icon opens the quick panel (`desktop-tauri/src/tray.rs`, `main.rs`). On Linux a layer-shell
compositor pins it to a screen corner. App settings › Panel corner picks the corner, or Automatic: the half of the
screen with the click point the tray host sent. A host that sends no point (0, 0, or a right click) gets the bottom
right. Why the setting: some tray hosts never send a point, and the bar can sit on any edge (issues #46, report #232).
Each click goes to the log as `TRAY CLICK` with the point or "no position".

### A crash report

Android: an uncaught-exception handler (`QuickBudsApp.kt`) writes the stack trace and the log. The next start shows it once
with Send. Desktop (`desktop-tauri/src/crash.rs`): each log line also goes to `log.txt` in the settings folder. A panic writes
its message to `crash.txt`. On Linux a fatal signal writes its name there too, with `write` only. At the next start a
non-empty `crash.txt` and the old `log.txt` become `crash-<time>.txt`. The main window opens, also after a `--tray` start,
and offers Send. The report goes to the Worker with `crash` set, so the model check does not apply.
Why: the desktop log lives in memory, and a crash lost it. A log file and a marker survive every crash. A clean exit, a
logout or a kill leaves `crash.txt` empty, so they show no dialog.

### The daily count and replies

Once a day the app posts its platform, version and random install ID to `/ping`. The Worker counts the install.
If the developer wrote a reply to a report of that install, the answer carries it. The app stores it, shows it once,
and sends its id in `ack` on the next ping. The Worker marks the reply as seen only then.
The user can turn the count off in Settings.

### The update check

1. The app asks `/latest` on the Worker, not GitHub. This saves the 60 requests an hour that GitHub gives one IP.
2. The Worker answers from the edge cache for 5 minutes. On a miss it asks GitHub and keeps the fields the apps need.
   It stores the last good answer in KV, and writes KV only when the release changes.
3. If the Worker fails, the app asks GitHub directly.
4. Android needs the `.apk` asset in the release. It downloads the file and opens the system installer through a FileProvider. The user confirms.
5. Desktop picks the file for its system (`-windows-x64.zip` or `-linux-x64.AppImage`),
   checks `SHA256SUMS` and replaces its own program. On Linux only an AppImage replaces itself. A Linux build that is
   not an AppImage, or a program in a folder it cannot write to, opens the release page instead.
6. Beta channel (switch "Beta versions", default off). A beta is a GitHub pre-release with a tag like
   `v6.4.0-beta.1`. With the switch on, the app asks the GitHub release list and takes the highest version,
   pre-releases included. The Worker `/latest` answers only stable releases, so the beta path skips it.
   The order is `6.3.0` < `6.4.0-beta.1` < `6.4.0-beta.2` < `6.4.0`.
7. Back to stable. Android blocks a lower `versionCode`, so a beta install stays until the next stable release.
   `versionCode` comes from `versionName` (`app/build.gradle.kts`): a beta gets a code below its own stable.
   Desktop has no such block: with the switch off, a beta build offers the latest stable release at once.

## Rules that keep the parts in step

- A protocol change updates `core/`, the Kotlin copy and OPPO-PROTOCOL.md together.
- Docs change in the same commit as the code.
- Which rows show, and which ids `0x010D` asks, is one rule set: `protocol/RowRules.kt` and its port
  `core/src/oppo/row_rules.rs`. Every screen, widget and tile asks it. Both run `testdata/row_fixtures.json`.
- A new user-visible string needs all 26 locales. `scripts/check-locales.py` checks this.
- A change to `models.json` needs `npx wrangler deploy` in `report-worker/`. Else the Worker rejects reports for the new model.
