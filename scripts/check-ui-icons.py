#!/usr/bin/env python3
"""Fails when desktop-tauri/ui/app.js calls ic('name') with a name that has no icon in I or BIG.
A missing icon throws in buildWindow(), and the window and the tray panel stay blank."""
import re, sys, pathlib

src = (pathlib.Path(__file__).parent.parent / "desktop-tauri/ui/app.js").read_text()
start = src.index("const I={")
icons = set(re.findall(r"^  (\w+):'", src[start:src.index("};", start)], re.M))
icons |= set(re.findall(r"^  (\w+):\[", src[src.index("const BIG={"):], re.M))
icons |= set(re.findall(r"^BIG\.(\w+)=", src, re.M))
used = set(re.findall(r"\bic\('(\w+)'", src))
missing = sorted(used - icons)
if missing:
    sys.exit("ui icons missing in app.js: " + ", ".join(missing))
print(f"ui icons: {len(used)} used, all defined")
