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

Source: OpenSCQ30 (`lib/src/devices/soundcore/`, read on 2026-11-22). The count is the number of its 41 models
with the module. A model-specific module (for example `a3947_sound_modes`) counts for the shared feature.

### Where the controls are

Android has 4 Soundcore screens: `SoundcoreSettingsActivity` (noise control details, gestures, the cycle, the
switches, auto power off, volume limit, firmware), `SoundcoreButtonsActivity`, `SoundcoreEqActivity` and
`SoundcoreDualActivity`. They use the same row, card and sheet parts as the OPPO screens, so they look the same.
The home screen keeps its rows: noise control, low latency (gaming mode), high-quality audio (LDAC), the equalizer,
dual connection and earbud settings open the Soundcore screens.
Why not the OPPO screens: they read OPPO model data (the `models.json` gesture tables, `EqCodec`, the `0x8100`
bitmap). The Soundcore driver gives its own data (`state_json`), so a screen per brand is smaller than one screen
with two data models. A third brand with the same features can take these screens over.

The table names each feature's control:

| Feature | Soundcore module | Models | QuickBuds control | Gap to close |
|---|---|---|---|---|
| Battery | `single_battery`, `dual_battery`, `case_battery_level` and the `_custom` variants | 41 | Battery rings, widgets, notification | Some models give a level from 0 to 5, not a percent. The rings and the widgets show steps. |
| Firmware | `serial_number_and_(dual_)firmware_version` | 36 | Earbud settings, firmware row | Add the serial number. A firmware for each bud. |
| Noise control | `sound_modes` and the model-specific ones | 24 | Noise control on the home screen | Modes Normal, Transparency, Noise cancelling. Variants: Transport, Indoor, Outdoor, Custom, Adaptive. Transparency has Fully and Vocal. A custom level is a new slider under the modes. |
| Equalizer | `equalizer`, `equalizer_tws`, `equalizer_with_drc` and the model-specific ones | 35 | EQ screen | 8 bands (100 Hz to 12.8 kHz). The driver gives the band frequencies, the dB range and the presets. HearID is a later item. |
| Gestures | `button_configuration` | 26 | Gesture screen | Gestures: single, double, triple, long press, slide up and down, double and long press on a call. The actions come from the driver's table. |
| Reset the gestures | `reset_button_configuration` | 21 | Gesture screen | A new "Reset" button. |
| Touch controls on or off | `disable_all_buttons`, `touch_lock`, `d1301_tap_controls_disabled` | 5 | Gesture screen | A new switch at the top. |
| Noise cycle of a gesture | `ambient_sound_mode_cycle` (and `_tws`) | 14 | Gesture screen, noise cycle | The modes in the cycle are switches, as on OPPO. |
| Dual connection | `dual_connections` | 17 | Dual connection screen | The switch and the device list exist. |
| High-quality audio | `ldac`, `d1402_ldac_and_three_d_sound` | 13 | High-quality audio switch | LDAC only. |
| Low latency | `gaming_mode` and the model-specific one | 7 | Low latency switch | None. |
| Wear detection | `wearing_detection` | 9 | Earbud settings | None. |
| 3D audio | `spatial_audio`, `surround_sound` | 7 | 3D audio switch and type | The spatial modes map to the 3D audio type. |
| Tap sensitivity | `a3953_press_sensitivity` | 1 | Tap sensitivity slider | None. |
| Personalized noise cancelling | `a3955_sound_modes` | 1 | Noise control screen | A switch only: the buds keep their own ear canal result. |
| Each bud connected | `tws_status` | 24 | No control | It feeds the battery and wear display. |

### New hand-made controls

| Feature | Soundcore module | Models | Place |
|---|---|---|---|
| Auto power off | `auto_power_off` | 21 | Earbud settings: a choice of times. |
| Touch tone | `touch_tone` | 18 | Earbud settings: a switch. |
| Prompts and tones | `low_battery_prompt`, `voice_prompt`, `ambient_sound_mode_voice_prompt`, `wearing_tone`, `battery_alert` | 17 | Earbud settings: one "Prompts and tones" group, one switch each. OPPO's alert volume moves into this group. |
| Volume limit | `limit_high_volume` | 9 | A new screen: the switch, the dB limit, the check interval. |
| Pause when a bud comes out | `auto_play_pause` | 3 | Earbud settings: a switch. The buds pause; this is not the phone's Smart pause. |
| Sound leak compensation | `sound_leak_compensation` | 3 | EQ screen: a switch. |

### Later or never

One model each, or not about earbud control. They wait until owners ask:
volume and volume balance (A3116, A3876), side tone and Dolby (A3062), immersive experience (A3955), easy chat,
air pressure, case language and the case screen (A3954), lights (A3876), the sleep features and alarms (D1301),
the wake word (D1402), power off (A3116).

### OPPO features that Soundcore does not have

The Soundcore driver does not list them, so their controls stay hidden: golden sound and the hearing profile, the fit
test, find my earbuds, game sound type, head gestures, the bass engine and dynamic
sound, voice boost, auto-answer, find phone, game EQ, the codec picker, power saving and the alert volume.

## Strings

OpenSCQ30 has labels in 19 locales (`lib/i18n/`). Its owner allows copies. A copied label keeps the GPL note of
the source. The 7 locales it does not have need new text.
