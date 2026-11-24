# SOUNDCORE-PROTOCOL.md — the Soundcore wire format

This is the RFCOMM protocol of Anker Soundcore earbuds and headphones, as QuickBuds speaks it. The code is in
`core/src/soundcore/`:

- `packet.rs`: the framing.
- `msg.rs`: the pushes every model shares.
- `models.rs`: one table per model: the state packet field by field, and the model's features.
- `presets.rs`, `demo.rs`: the equalizer presets and a recorded state packet per model. Scripts in `local/tools/`
  make both from OpenSCQ30; do not edit them by hand.
- `driver.rs`: the state, every write and the JSON for the apps.

The Android app runs it through `core-jni/` (`sc_command`, `sc_state_json`).

The source is [OpenSCQ30](https://github.com/Oppzippy/OpenSCQ30) (`lib/src/devices/soundcore/`, commit `c941bc4`),
GPL-3.0-or-later. Its owner agreed to the port. The ported code carries the note "Copyright 2026 Kyle Scheuing,
GPL-3.0-or-later".

**Evidence tags:**
- `[OPENSCQ30]`: OpenSCQ30's code or its tests.
- `[RECORDED]`: a state packet from a real device in OpenSCQ30's tests or its device faker.
- `[TESTED]`: a tester's log. The developer owns no Soundcore buds.
- `[GUESS]`: unverified.

**Models:** every OpenSCQ30 model except the A3116 (Motion+, a speaker): A3004, A3005, A3027, A3028, A3029,
A3030, A3031, A3033, A3035, A3040, A3062, A3330, A3388, A3876, A3909, A3926, A3930, A3931, A3933, A3935, A3936,
A3939, A3944, A3945, A3947, A3948, A3949, A3951, A3952, A3953, A3954, A3955, A3957, A3959, A3968, D1101, D1202,
D1202C, D1301, D1402. The names are in `app/src/main/assets/models.json` (`soundcore`). A Space A40 (A3936) is
`[TESTED]`; the others are `[OPENSCQ30]` `[RECORDED]`.

## 1. Connect

- Brand pick (`ConnectPolicy.brand`): the buds list a service UUID that starts with
  `0cf12d31-fac3-4553-bd80-d6832e7`, or their name starts with "soundcore". Else the OPPO driver runs `[OPENSCQ30]`.
  The last 5 hex digits of the vendor UUID name the model, for example `b3936` or `d1101`.
- Service: the vendor UUIDs the buds list. Only buds without one get plain SPP `00001101-0000-1000-8000-00805f9b34fb`
  `[OPENSCQ30]`. A Space A40 with the vendor service gives no state on SPP `[TESTED]`. The OPPO channel 15 fallback is off.
- The driver sends `01 01` (request state). No answer in 1.5 s: it sends it once more.
- The model: each table's serial number field is tried; the serial starts with the model number, for example
  `3936`. The tables of the models without a serial in their state (A3031, A3909, A3926, A3930, A3931, A3951)
  match the Bluetooth name instead. No match, or a packet shorter than the table: a `NOTE` log line and no state.
- Dual connection on: the driver asks for the device list (`0b 01`).
- Android saves the brand only when the driver knows the model (its `Ready`); an OPPO link clears it.
  A Soundcore device counts as the buds for the auto-connect only with the name of a known model.

## 2. Framing

| Part | Bytes |
|---|---|
| Direction | phone to buds `08 EE 00 00 00`; buds to phone `09 FF 00 00 01` |
| Command | 2 bytes, for example `06 81` |
| Length | u16 little-endian, the whole frame |
| Body | the payload |
| Checksum | 1 byte: the sum of all bytes before it, mod 256 |

The reader joins reads into frames and skips bytes up to the next header. A bad checksum discards the frame
(`DISCARDED RX` in the log) `[OPENSCQ30]`. The log has whole frames, `TX: 08 EE …` and `RX: 09 FF …`, with the
command in brackets. The report Worker accepts a log that holds an `RX: 09 FF` line.

The buds answer each write with a packet of the same command: empty, or with the written bytes (`0b 83`, `0b 84`).
The driver changes its copy of the state when it sends the write, as OpenSCQ30 does after the answer. A write that
the buds refuse is not seen `[GUESS]`.

## 3. Pushes

| Command | Body | Meaning |
|---|---|---|
| `01 01` | the state packet (section 4) | answer to the request; replaces the whole state |
| `01 02` | host, both | TWS status: both buds connected to each other |
| `01 03` | 1 level (headphones), or left, right (1 or 2 more bytes on newer firmware; a Space A40 sends 3) | battery level |
| `01 04` | 1 or 2 bytes, 1 = charging | charging |
| `01 05` | 1 or 2 firmware versions ("04.19"), 16-byte serial | firmware |
| `01 10` | 1 byte | voice prompt changed |
| `01 11` | 1 byte | gaming mode changed on the buds |
| `01 7F` | 1 byte | LDAC |
| `06 01` | the model's full sound mode group | sound modes; another length is refused, as OpenSCQ30 does |
| `0b 01` | total, index, devices | dual connection devices (section 6) |

## 4. The state packet

Each model's table in `models.rs` lists the fields in order with their lengths. The driver adds the lengths up for
the offsets, so no offset is written by hand. Field keys:

| Key | Bytes | Content |
|---|---|---|
| `tws` | 2 | host, both buds connected |
| `battery` | 1 to 4 | single: level, charging; dual: left, right, left charging, right charging; or the levels only |
| `fw`, `serial` | 5 or 10, 16 | firmware of one or both buds ("04.19"), serial number |
| `case` | 1 | case battery |
| `eq` | 2 + channels x bands | preset id (u16 LE), then the bands (section 7) |
| `gender`, `age` | 1 each | HearID profile; age 0xFF = no HearID |
| `hearId` | 21 to 58 | the HearID block; its layout per model is a `Hid` in `models.rs` |
| `buttons` | 1 or 2 per button | the button group (section 6); a model can split it (A3953) or have more groups |
| `cycle` | 1 | noise control cycle: bit 0 noise cancelling, bit 1 transparency, bit 2 normal (A3968: per nibble) |
| `modes` | 4 to 8 | the sound mode group (section 5) |
| `autoOff` | 2 | on, index of the time |
| `volumeLimit` | 3 | on, dB, refresh rate |
| `dual` | 1 | dual connection on |
| other keys | 1 | one setting each (section 5, settings) |

Battery levels: most models report 0 to 5. Some report 0 to 9 with 1 added (out of 10), the A3954 0 to 100.
`models.rs` gives each model's scale. Optional fields at the end are absent when the packet ends before them.
The A3953 has a gap whose length is the byte before its buttons minus 18 `[OPENSCQ30]`.

## 5. Writes

### Sound modes

The group (`06 01`, `06 81`) differs per model. `models.rs` lists its fields: the byte, the part of the byte (whole,
high or low nibble, bit 0 or 1), the values and the parent mode. A write changes one field and sends the whole
group with `06 81`. Common fields:

| Key | Meaning |
|---|---|
| `noise` | 0 noise cancelling, 1 transparency, 2 normal (A3954: 3 airplane) |
| `ancMode` | the noise cancelling mode; its values have names per model (manual, adaptive, transport, outdoor, indoor, custom, scene) |
| `ancLevel`, `customAnc` | the strength (1 to 3 named, 1 to 5, or 0 to 10) |
| `adaptiveLevel` | read only |
| `transparencyMode`, `transparencyLevel` | full or vocal (A3040: talk or manual), and a level 1 to 5 |
| `transport`, `multiScene` | plane, train, bus, car; transport, outdoor, indoor |
| `windNoise`, `windDetected` | wind suppression; wind detected now (read only) |

Some models take a field only while its parent mode is on (OpenSCQ30 `MigrationSteps`). For them the driver
refuses such a write (a `NOTE` line). The apps show each field only while its parent mode is on (`shown`), for
every model. Special cases: the A3035 and A3959 repeat the noise control mode in byte 2; the A3968 writes 0xFF
in byte 5, the D1202 0; the A3954 holds noise cancelling 1 to 5 (6 minus the level) and transparency 7 to 11
(6 plus the level) in one slider byte.

### Settings

| Setting (`state_json` key) | Command | Body |
|---|---|---|
| A one-byte setting | its own (`models.rs` `Set`) | the value. `touchTone` `01 83`, `gaming` `01 87` (A3947, A3957 `10 85`), `wearDetection` and `autoPlay` `01 81`, `wearingTone` `01 8C`, `lowBattery` `10 82` (A3035, A3040 `01 AF`), `voicePrompt` `01 90` (A3035, A3040, A3062, D1402 `01 AE`), `leak` `10 86`, `surround` `02 86`, `touchLock` `04 86`, `ldac` `01 FF`, `buttonsOff` `10 94` (1 = the touch controls are off) |
| Auto power off `autoOff` | `01 86` | on, index |
| Limit high volume `volumeLimit` | `20 82` / `20 81` | on, dB (75 to 100, steps of 5) / refresh rate |
| Noise control cycle `cycle` | `06 82` | the bits; the buds need two modes |
| Button action `button` | `04 81` (A3330 call buttons `04 87`, D1402 function button `04 61`) | side (0 left, 1 right), button id, action byte |
| Button on or off | `04 83` | side, button id, on byte |
| All buttons (COMMON_SETTINGS models) | `04 84` | the whole button group, when two buttons change |
| Reset the buttons | `04 82` | empty; the buds answer with an empty `04 82`, then the driver sends `01 01` |
| Equalizer `eq` | section 7 | |
| Dual connection `dual` | `0b 84` | 0 or 1 |
| Connect, disconnect, forget a device | `0b 82`, `0b 81`, `0b 83` | the 6-byte address |

Model-only settings in the tables: Dolby audio `02 86` and side tone `01 84` (A3062), ambient sound prompt `10 83`
and press sensitivity `04 85` (A3953), personalized noise cancelling `03 90` (A3955), volume balance `10 8C` (the balance -6 to +6 plus 100)
(A3876), case language `07 8A` (A3954), the Hey Anka wake word `18 83` (D1402), and the sleep settings of the
D1301 (listening mode `01 A9`, default listening mode `10 95`, prompts `10 9E` and `10 9F`, noise cancelling
`06 87`, incoming calls `10 75`, tap controls `10 94`, switch once asleep `15 8F`).

### Auto power off

The index picks the time; each model lists its times: 30, 60, 90, 120 min; 10, 20, 30, 60 min; all 8 from 10 to
300 min (A3931); or 30, 60, 180, 300 min (D1402).

### Volume limit refresh rate

0 real time, 1 every 10 s, 2 every minute.

## 6. Buttons and devices

A button group lists its buttons in state order (press kind, side). Press kinds and button ids: single 2, double 0,
triple 5, long 1, slide up 6, slide down 7. Each press kind has a list of action ids with names.

A button has an action byte and, on some models, an "on" byte before it. Each byte is either one value or two
nibbles: low = the value while both buds are connected to each other, high = the value while one bud is alone.
The driver writes the nibble that applies now (TWS status) and keeps the other. Without an "on" byte, or on models
whose "on" byte is ignored, action 0xF is off.

A button turns off in one of three ways per model: not at all; one side off turns the other off too, and one side
on turns the other on with its last action; or each side alone `[OPENSCQ30]`.

| Action id | Action |
|---|---|
| 0 | volume up |
| 1 | volume down |
| 2 | previous |
| 3 | next |
| 4 | noise control mode (the cycle) |
| 5 | voice assistant |
| 6 | play / pause |
| 7 | bass up (A3035, A3040, A3062, D1402) |
| 9 | gaming mode |

Model-only actions: lights 14 (A3876), noise cancelling 8 and change mode 13 (D1301), decline 0 and answer 1 on a
call (A3330), Anka assistant 11, translation 10 and nap 13 on the function button (D1402).

Device list (`0b 01`): total packets, this packet's index, then per device: a length byte (8 + the name length),
connected (0 or 1), the 6-byte address, the name (UTF-8, up to a 0 byte). The list can come in several packets.

## 7. Equalizer

- Each model has its preset list (`presets.rs`): the 22 Soundcore presets in two variants, or a model's own list.
  Custom is preset `FE FE`; the A3944 and A3949 take presets only.
- A band byte = tenths of a dB + 120. Range -12.0 to +13.4 dB. The A3909 counts whole dB: byte = dB + 12, and its
  state holds the preset id only. The shown bands are 8 (100 Hz to 12.8 kHz). A model with 10 bands has 2 hidden
  ones: 0 and -12.0 dB for a preset, 0 and 0 for the custom curve.
- The driver writes every channel with the same bands and turns HearID off, so the bands apply.
- DRC: OpenSCQ30's `apply_drc`, from the Soundcore app ("DRC equalizer"). It mixes the 8 bands with fixed
  coefficients and divides the result by 10. Some writes carry these effective bands after the set ones.

| Write (`models.rs` `EqW`) | Command | Body |
|---|---|---|
| `Plain` | `02 81` | preset, bands |
| `Drc` | `02 83` | preset, bands, DRC bands |
| `Raw83` (A3388) | `02 83` | preset, bands (the one stored channel twice) |
| `A3876` | `02 83` | preset, channel 1, channel 1 with DRC |
| `HearId` | `03 87`; `03 86` when the age is 0xFF | preset, genre, bands, gender, age, 0, HearID bands, time, type, custom bands, DRC bands. Without HearID: 0xFF for gender, age and the band groups, and no genre. The A3936 always takes `03 87`. |
| `A3035` | `03 87` | preset, genre, bands, FF FF, 0, HearID bands, time, type, custom bands, DRC bands, 0 |
| `A3040` | `03 87` | preset, genre, channel 1 twice, 0 1, 0, HearID bands, time, type, left custom bands twice, DRC twice, 0 |
| `A3062` | `03 87` | preset, genre, bands, 0 0, 0, HearID bands, time, type, custom bands, bands, 0 |
| `A3947` | `03 87` | as A3062, with the DRC bands (band 10 = 0) in place of the second bands |
| `A3953` (A3953, A3955) | `03 87` | as A3035; FF FF is 0 0 when the HearID status byte is not 0xFF or 0xFE |
| `A3954` (A3954, A3957, D1202) | `03 87` | preset, genre, bands, 0 0, 0, HearID bands, time, type, custom bands, each band byte followed by 0, 0 0 |
| `D1402` | `03 87` | preset, genre, bands, the unknown EQ byte, age, 0, HearID bands, time, type, custom bands, DRC bands, 0 |

An absent HearID channel (first byte 0xFF) goes out as 0xFF bytes; the A3035, A3953 and D1402 writes end each
absent channel with a 0 `[OPENSCQ30]`.

## 8. What the apps show

`core/src/soundcore/driver.rs` `state_json` gives every setting. A key is absent when the model lacks the setting.

| Key | Content |
|---|---|
| `modes` | the sound mode fields: `key`, `value`, `values` (empty = read only), `labels` (value names), `shown` |
| `switches`, `choices` | the one-byte settings: `key` and `on`, or `key`, `value` and `values` |
| `gaming`, `ldac`, `dual`, `devices` | the home rows |
| `eqPreset`, `eqBands`, `eqPresets`, `eqHz`, `eqRange`, `eqStep`, `eqCustom` | the equalizer |
| `buttons` | per button: `group`, `press`, `side`, `action`, `actions` (id and name), `canOff`; also `buttonsPaired`, `buttonsReset` |
| `cycle`, `autoOff`, `volumeLimit`, `firmware`, `serial` | as their names say |

Android: the home rows (noise control, low latency = gaming mode, high-quality audio = LDAC, equalizer, dual
connection, earbud settings) show when the model has the key, and four screens: `SoundcoreSettingsActivity`,
`SoundcoreButtonsActivity`, `SoundcoreEqActivity`, `SoundcoreDualActivity`. The screens build their rows from the
lists, so they hold no model table. Dev tools › Soundcore demo shows each model's recorded state without buds.
The desktop app does not connect to Soundcore buds yet.

### Groups

A group is a setting of several parts, or one with its own write body (`models.rs` `Group`). `state_json` gives
`groups`: per group its `key` and `parts` (`key`, `kind` switch, choice, range, info or color, `value`, and
`values`, `labels` or `range`). A write is `{"part": value}` under the group key.

| Group | Models | Write |
|---|---|---|
| `spatial` | A3953, A3954: on, effect (fixed, head tracking), content; D1202: on, content | `10 81`: on, effect, content (D1202: on, 0, content) |
| `caseFeatures` | A3954: ambient light, remote camera, find device, the 3D audio button | `07 87`: the 4 bytes |
| `easyChat` | A3954: on, wait time (5, 10, 15 s), air pressure (read only) | `10 9D`: on, wait time |
| `lights` | A3876: on, brightness 1 to 10, off after 1 to 120 min, mode, color | `0E 87` on, `0E 82` brightness, `0E 84` off after, `0E 86` mode, `0E 85` red, green, blue |
| `postSleep` | D1301: pause the audio or play the sleep sounds once you sleep | `15 85`: FF FF FF, the action |
| `timer` | D1301: the sleep timer, on and 1 to 240 min. The driver asks for it with `15 03` after the state. | `15 85`: on, minutes (i16 LE), the kept byte |

Not ported: the D1301 alarms (OpenSCQ30 reads them but has no write either) and the A3116 speaker.
