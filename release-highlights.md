<!--
Highlights for the next release: a few lines written for users, shown first in
the app's Updates window (and its "What's new") and above the release text on
GitHub. Write them in a pull request before merging the release PR, under the
version that release PR will publish, one line per highlight:

# 1.48.0
- **Big new thing** — what it does for you, in one sentence.
- **Another one** — optional? Say so.

Only a list for the version being released is used, so last release's list
never shows up again; replace it for the next one. Everything inside this
comment is ignored. See "Release routine" in docs/release.md.
-->

# 1.52.0
- **Compact panels** — in Control Center → Dashboard, the CPU, GPU, RAM and Network panels can be Full or Compact: the key values at a glance (load and temperature rings, usage, up/down and ping) without graphs or per-core bars, about half the height, per profile. As floating panels they are also narrower, and floating panels now scale up to 200 %, so you can make them bigger to read without them getting wide. An idea from @SirBlacky21 — thank you!
- **Floating panels per profile** — floating panels or the fixed dashboard, and the panels' size, now follow your Control Center profile (Control Center → Dashboard): float them in one profile, keep the fixed dashboard in another, and switching profile switches the layout too.
- **Choose your own shortcuts** — Settings → Shortcuts lets you change or turn off each global shortcut (overlay, next profile, profile 1–9, microphone): choose New shortcut… and press the keys. You can also give a key to the tray menu's desk lamp, Control Center, Settings, Status, recording, overlay lock and floating panels (off until you choose one). Handy when another program already uses a shortcut, like NVIDIA App with Ctrl+Alt+M; Status points you there when that happens.
