# Core API

`core/` (`buds-core`) is the portable protocol library. It holds one driver for each brand, the model data and
one brand-neutral feature model. The Android app and the desktop app are shells around it. A shell owns the socket,
the service and the UI. It never reads a brand's frames.

## The driver

A driver does no I/O. It never blocks and never reads a clock or a file.

| Input | Meaning |
|---|---|
| `connected(now, name)` | The shell opened the link. The init sequence starts. |
| `on_bytes(now, unix, bytes)` | Bytes from the socket. |
| `command(now, Cmd)` | A request from the UI. |
| `tick(now)` | The clock. The shell calls it at `next_wake()` or sooner. |
| `disconnected(now)` | The link is gone. |

Each input returns a list of effects: `Send(bytes)`, `Log`, `State` (the state changed), `Save(key, value)`, `Ready`.
A wait between two frames is a step in the driver's queue. The shell passes the user's settings in (`Settings`).
`core/src/oppo/driver.rs` is the first driver.

## The feature model

The app has no generic settings page. Each feature has its own hand-made control in a fixed place in the UI.
A brand maps its protocol onto these features. A feature that no screen has yet gets a hand-made control once.
After that, every brand that has the feature uses it.

`core/src/model.rs` holds the shared state (`Common`): connection, name, model id, battery levels with their
maximum, wear, noise control mode and modes, low latency, and the feature keys. The feature keys are the row keys
of `oppo/row_rules.rs`, the same as the Android `RowRules.kt`. A driver gives it with `common()`.

Rules:
- The driver lists the features of the connected buds. A control shows only when its feature is on that list.
- A feature has one id in one append-only list. A feature never changes its meaning.
- The widgets, the tiles and the notification read only the shared state: battery, wear, noise control,
  low latency, and the busy and failure state of the setting engine.
- A feature value is typed: a switch, a choice from a list, a level in a range, an EQ curve, or a list of devices.
  The driver gives the limits: the list of choices, the range and its step, the EQ band count and frequencies.
- Each new feature label needs all 26 locales (`scripts/check-locales.py`).

## Mapping: Soundcore

Source: OpenSCQ30 (`lib/src/devices/soundcore/`). Each model is a table in `core/src/soundcore/models.rs`.

### Where the controls are

Android has 4 Soundcore screens: `SoundcoreSettingsActivity` (noise control details, the cycle, the switches and
choices, auto power off, volume limit, firmware), `SoundcoreButtonsActivity`, `SoundcoreEqActivity` and
`SoundcoreDualActivity`. They use the same row, card and sheet parts as the OPPO screens, so they look the same.
The home screen keeps its rows: noise control, low latency (gaming mode), high-quality audio (LDAC), the equalizer,
dual connection and earbud settings open the Soundcore screens. Each row shows only when the model has its key.
Why not the OPPO screens: they read OPPO model data (the `models.json` gesture tables, `EqCodec`, the `0x8100`
bitmap). The Soundcore driver gives its own data (`state_json`): lists of sound mode fields, switches, choices and
buttons, each with its values and value names. The screens build their rows from these lists, so a new model needs
a table in the core and no screen code.

| Feature | Control |
|---|---|
| Battery, case battery | Battery rings, widgets, notification. Levels in steps (0 to 5, 1 to 10) or percent. |
| Firmware, serial | Earbud settings, About. |
| Noise control and its fields | Home noise control (the modes the model has); the fields in Earbud settings. |
| Equalizer | EQ screen: the model's presets and, on most models, a custom curve of 8 bands. |
| Gestures | Gesture screen: per group and side; reset; the noise control cycle in Earbud settings. |
| Dual connection | Dual connection screen. |
| LDAC, gaming mode | High-quality audio and low latency rows. |
| One-byte settings | Earbud settings: a switch or a choice each (prompts and tones, wear detection, auto play, leak compensation, surround, touch lock, touch controls, Dolby, side tone, press sensitivity, volume balance, case language, the D1301 sleep settings). |
| Auto power off, volume limit | Earbud settings. |

### Later

One model each, and more work than a byte: the A3876 lights, the A3954 case features, easy chat and air pressure,
spatial audio (A3953, A3954, D1202), the D1301 alarms, timer and post-sleep audio, the A3116 speaker. They wait
until owners ask.

### OPPO features that Soundcore does not have

The Soundcore driver does not list them, so their controls stay hidden: golden sound and the hearing profile, the fit
test, find my earbuds, game sound type, head gestures, the bass engine and dynamic
sound, voice boost, auto-answer, find phone, game EQ, the codec picker, power saving and the alert volume.

## Strings

OpenSCQ30 has labels in 19 locales (`lib/i18n/`). Its owner allows copies. A copied label keeps the GPL note of
the source. The 7 locales it does not have need new text.
