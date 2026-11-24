# SOUNDCORE-PROTOCOL.md — the Soundcore wire format

This is the RFCOMM protocol of Anker Soundcore earbuds, as QuickBuds speaks it. The code is in `core/src/soundcore/`:
`packet.rs` (framing), `msg.rs` (the packets every model shares) and `driver.rs` (the model layouts, the state and
every write). The Android app runs it through `core-jni/` (`sc_command`, `sc_state_json`).

The source is [OpenSCQ30](https://github.com/Oppzippy/OpenSCQ30) (`lib/src/devices/soundcore/`, commit `c941bc4`),
GPL-3.0-or-later. Its owner agreed to the port. The ported code carries the note "Copyright 2026 Kyle Scheuing,
GPL-3.0-or-later".

**Evidence tags:**
- `[OPENSCQ30]`: OpenSCQ30's code or its tests.
- `[RECORDED]`: a state packet from a real device in OpenSCQ30's tests or its device faker.
- `[TESTED]`: a tester's log. The developer owns no Soundcore buds.
- `[GUESS]`: unverified.

**Models:** Space A40 (A3936) and Liberty 5 (A3957). Not tested on buds.

## 1. Connect

- Brand pick (`ConnectPolicy.brand`): the buds list a service UUID that starts with
  `0cf12d31-fac3-4553-bd80-d6832e7`, or their name starts with "soundcore". Else the OPPO driver runs `[OPENSCQ30]`.
- Service: the vendor UUIDs the buds list. Only buds without one get plain SPP `00001101-0000-1000-8000-00805f9b34fb`
  `[OPENSCQ30]`. A Space A40 with the vendor service gives no state on SPP `[TESTED]`. The OPPO channel 15 fallback is off.
- The driver sends `01 01` (request state). No answer in 1.5 s: it sends it once more.
- The answer's serial number (bytes 16 to 31) starts with the model number, for example `3936`. That picks the layout.
  A serial of an unknown model, or a short packet, gives a `NOTE` log line and no state.
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

The buds answer each write with a packet of the same command: empty, or with the written bytes (`0b 83`, `0b 84`). The driver changes its copy of the state when it
sends the write, as OpenSCQ30 does after the answer. A write that the buds refuse is not seen `[GUESS]`.

## 3. Pushes

| Command | Body | Meaning |
|---|---|---|
| `01 01` | the state packet (section 4) | answer to the request; replaces the whole state |
| `01 02` | host, both | TWS status: both buds connected to each other |
| `01 03` | left, right (1 or 2 more bytes on newer firmware; a Space A40 sends 3) | battery level |
| `01 04` | left, right | charging, 1 = yes |
| `01 05` | 2 firmware versions ("04.19"), 16-byte serial | firmware |
| `01 11` | 1 byte | gaming mode changed on the buds |
| `01 7F` | 1 byte | LDAC |
| `06 01` | 4 bytes, or the model's full sound mode group | sound modes |
| `0b 01` | total, index, devices | dual connection devices (section 6) |

## 4. The state packet

Offsets count from the first body byte `[OPENSCQ30]` `[RECORDED]`.

| Field | A3936 Space A40 | A3957 Liberty 5 |
|---|---|---|
| TWS status | 0 | 0 |
| Battery left, right; charging left, right | 2 | 2 |
| Firmware left, right | 6 | 6 |
| Serial | 16 | 16 |
| Case battery | 124 | 37 |
| Equalizer: preset u16 LE, 2 x 10 bands | 32 | 38 |
| Age range | 54 | 60 |
| HearID block (48 bytes) | 55 | 61 |
| Buttons | 104 (6 x 2 bytes) | 110 (8 x 1 byte) |
| Noise control cycle | 116 | 118 |
| Sound modes | 117 (6 bytes) | 119 (7 bytes) |
| Touch tone | 123 | 145 |
| LDAC | 126 | 129 |
| Dual connection | 127 | 130 |
| Auto power off: on, index | 128 | 131 |
| Gaming mode | 130 | 146 |
| Wearing tone | - | 127 |
| Low battery prompt | - | 128 |
| Limit high volume: on, dB, refresh rate | - | 133 |
| Sound leak compensation | - | 143 |
| Wear detection | - | 144 |
| Shortest packet | 131 | 148 |

Battery: the Space A40 reports 0 to 5. The Liberty 5 reports 0 to 9 for the buds and the case: the level is
`raw + 1` out of 10. The case of the Space A40 is 0 to 10.

The Liberty 5 also has an immersive byte (136) and a pressure sensitivity byte (147). OpenSCQ30 has no write for
them, so the driver does not show them.

## 5. Writes

| Setting (`state_json` key) | Command | Body |
|---|---|---|
| Sound modes (all fields below) | `06 81` | the full sound mode group with one field changed |
| Touch tone `touchTone` | `01 83` | 0 or 1 |
| LDAC `ldac` | `01 FF` | 0 or 1 |
| Gaming mode `gaming` | A3936 `01 87`, A3957 `10 85` | 0 or 1 |
| Wearing tone `wearingTone` | `01 8C` | 0 or 1 |
| Low battery prompt `lowBattery` | `10 82` | 0 or 1 |
| Wear detection `wearDetection` | `01 81` | 0 or 1 |
| Sound leak compensation `leak` | `10 86` | 0 or 1 |
| Auto power off `autoOff` | `01 86` | on, index |
| Limit high volume `volumeLimit` | `20 82` / `20 81` | on, dB (75 to 100) / refresh rate |
| Noise control cycle `cycle` | `06 82` | bit 0 noise cancelling, bit 1 transparency, bit 2 normal |
| Button action `button` | `04 81` | side (0 left, 1 right), button id, action byte |
| Button on or off (A3936) | `04 83` | side, button id, on byte |
| Reset the buttons | `04 82` | empty; the buds answer with an empty `04 82`, then the driver sends `01 01` |
| Equalizer `eq` | `03 87` | see section 7 |
| Dual connection `dual` | `0b 84` | 0 or 1 |
| Connect, disconnect, forget a device | `0b 82`, `0b 81`, `0b 83` | the 6-byte address |

### Sound modes

| Byte | Field | Values |
|---|---|---|
| 0 | noise control | 0 noise cancelling, 1 transparency, 2 normal |
| 1 | strengths | high nibble: manual level (A3936 1 to 3, A3957 1 to 5); low nibble: adaptive level, read only |
| 2 | transparency mode | 0 full, 1 vocal |
| 3 | noise cancelling mode | 0 manual, 1 adaptive (A3936), 2 transport (A3957) |
| 4 | wind | bit 0 suppression on, bit 1 wind detected now (read only) |
| 5 | adaptive sensitivity | kept as it is |
| 6 | transport (A3957) | 0 plane, 3 car |

The A3957 takes a field only while its parent mode is on: transparency mode in transparency, the noise cancelling
mode in noise cancelling, the strength in manual noise cancelling, the transport type in transport mode, wind in
noise cancelling or transparency. OpenSCQ30 plans such changes in steps (`MigrationSteps`) `[OPENSCQ30]`.
The driver refuses a field whose parent mode is off (a `NOTE` line), and the apps show each sub-setting only while
its parent mode is on, for every model. So no write needs a mode change first.
A `06 01` push of another length than the model's group is refused, as OpenSCQ30's parser does.

### Auto power off

The index picks the time. A3936: 30, 60, 90, 120 min. A3957: 10, 20, 30, 60 min.

### Volume limit refresh rate

0 real time, 1 every 10 s, 2 every minute.

## 6. Buttons and devices

Press kinds and button ids: single 2, double 0, triple 5, long 1. A3936: single, double, long. A3957: single,
double, triple, long. In the state, the order is left single, right single, left double, right double, and so on.

The action byte has 2 nibbles: low = the action while both buds are connected to each other, high = the action
while one bud is alone. The driver writes the nibble that applies now (TWS status) and keeps the other.

| Action id | Action |
|---|---|
| 0 | volume up |
| 1 | volume down |
| 2 | previous |
| 3 | next |
| 4 | noise control mode (the cycle) |
| 5 | voice assistant |
| 6 | play / pause |
| 9 | gaming mode |
| 15 | none (A3957) |

A3936 single press: 0, 1, 2, 3, 6. A3936 double and long: all 8. A3957: 0 to 6.

A3936 buttons have an "on" byte before the action byte, with the same 2 nibbles. Turning one side off turns the
other side off too. Turning one side on turns the other side on with its last action `[OPENSCQ30]`.
A3957 buttons have no "on" byte: action 15 is off.

Device list (`0b 01`): total packets, this packet's index, then per device: a length byte (8 + the name length),
connected (0 or 1), the 6-byte address, the name (UTF-8, up to a 0 byte). The list can come in several packets.

## 7. Equalizer

- Preset ids 0 to 21 (Soundcore Signature to Treble Reducer), custom `FE FE`. The driver has the 22 presets'
  bands (OpenSCQ30 `common_settings_type_2`).
- A band byte = tenths of a dB + 120. Range -12.0 to +13.4 dB. 10 bands per channel: 8 shown
  (100 Hz to 12.8 kHz), then 2 hidden bands: 0 and -12.0 dB for a preset, 0 and 0 for the custom curve.
- The driver writes both channels with the same bands and turns HearID off, so the bands apply.
- A3936 body: preset (2), music genre (2, from the HearID block), bands (2 x 10), gender (0), age range, 0,
  HearID bands (20), HearID time (4, big-endian), HearID type, HearID custom bands (20), the DRC bands (2 x 10).
- A3957 body: preset (2), music genre (2), bands (2 x 10), 0, 0, HearID on (0), HearID bands (20), time (4),
  type, custom bands (20), each active band byte followed by 0 (40), 0, 0.
- DRC (A3936): OpenSCQ30's `apply_drc`. It mixes the 8 bands with fixed coefficients and divides the result by 10.

## 8. What the apps show

`core/src/soundcore/driver.rs` `state_json` gives every setting. A key is absent when the model lacks the setting.
Android: the home rows (noise control, low latency = gaming mode, high-quality audio = LDAC, equalizer, dual
connection, earbud settings) and four screens: `SoundcoreSettingsActivity`, `SoundcoreButtonsActivity`,
`SoundcoreEqActivity`, `SoundcoreDualActivity`. The desktop app does not connect to Soundcore buds yet.
