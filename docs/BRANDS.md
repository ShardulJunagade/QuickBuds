# Other brands

QuickBuds adds a brand only when the open source community has already worked out its protocol.
The project does not reverse-engineer a new brand and does not buy earbuds for it.
Each brand driver names its sources here and in the README credits.

## Order

Soundcore comes first: issue #32 asked for it. Then the brands follow by share of true wireless earbud
sales (Canalys, Q1 2025: Apple 23 %, Xiaomi 11.5 %, Samsung 7 %, Huawei 6 %, boAt 5 %) and by how complete
the open source work is.

1. **Anker Soundcore.** Source: [OpenSCQ30](https://github.com/Oppzippy/OpenSCQ30), GPL-3.0, Rust, RFCOMM.
   It covers many Liberty, Life, Space and Q models. Its owner allows QuickBuds to copy parts with a
   copyright note ("Copyright 2026 Kyle Scheuing, GPL-3.0-or-later").
2. **Apple AirPods and Beats.** Source: [LibrePods](https://github.com/librepods-org/librepods), GPL-3.0,
   Android and Linux. Battery, noise control, ear detection, conversation awareness and head gestures over L2CAP.
   Android needs root, except ColorOS and OxygenOS 16 and Pixel phones on Android 16 QPR3. Battery from the BLE
   advertisement needs no root. Linux needs a changed `DeviceID` in `/etc/bluetooth/main.conf` for some features.
3. **Samsung Galaxy Buds.** Source: [GalaxyBudsClient](https://github.com/timschneeb/GalaxyBudsClient), GPL-3.0, C#,
   RFCOMM. It covers all Galaxy Buds models.
4. **Sony.** Source: [mos9527/SonyHeadphonesClient](https://github.com/mos9527/SonyHeadphonesClient), MIT, with the
   XM5 generation. It continues [Plutoberth/SonyHeadphonesClient](https://github.com/Plutoberth/SonyHeadphonesClient)
   (archived).
5. **Huawei and Honor.** Source: [OpenFreebuds](https://github.com/melianmiko/OpenFreebuds), GPL-3.0, Python.
6. **Xiaomi and Redmi.** Source: [Gadgetbridge](https://codeberg.org/Freeyourgadget/Gadgetbridge), AGPL-3.0.
   Only a few models, for example the Redmi Buds 6 Pro.
7. **Nothing and CMF.** Sources: Gadgetbridge, [EarA](https://github.com/oneydef/EarA-linux) (GPL-3.0) and
   [something-x](https://pypi.org/project/something-x/) (MIT). RFCOMM.
8. **Google Pixel Buds.** Source: [pbpctrl](https://github.com/qzed/pbpctrl), Apache-2.0, Rust.
   Pixel Buds Pro only. Android already has the Pixel Buds settings built in.

No open source protocol work was found for boAt and JBL. They wait.

[BudsLink](https://github.com/maniacx/BudsLink) (GPL-3.0, Linux) covers Apple, Samsung, Sony and Nothing in one
app. It is a second source to check each fact against.

## Rules

- A fact from a source gets a link to that source in the driver and in PROTOCOL.md.
- Code is written new from the documented facts, unless the source's licence and its owner allow a copy.
  QuickBuds has a commercial licence too (README, License). GPL code copied from another project
  cannot go under that licence.
- An owner of the earbuds confirms each model with the model test before it counts as confirmed.
