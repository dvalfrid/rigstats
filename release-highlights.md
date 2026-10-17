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

# 1.49.1
- **Clear messages when something goes wrong** — every problem now looks and reads the same: what happened, what still works and what to do, with a button for the fix, for collecting diagnostics or for reading more. A page that stops working says so instead of disappearing, Status lists everything that is wrong in one place, and a profile switch from the hotkey or an app rule that fails shows a notification.
- **A crashing driver no longer takes the sensors down** — the sensor reader runs in a process of its own: if a graphics driver crashes it, it is back within seconds while fans, lighting and profiles keep working, and a card that keeps crashing it (a laptop switching its NVIDIA GPU off, for example) is left out until Windows restarts, with Status saying why and what to do. The service also no longer fails to start at boot when the hardware is slow to open.
- **Logitech mice and keyboards on a Bolt receiver** — their battery now shows in Peripherals, like Bluetooth and ROG devices (a Unifying receiver should work the same way, but hasn't been tried yet). A Bluetooth headset that is switched off now shows as not connected instead of keeping its last level.
- **Tray icon fixes** — no empty tooltip box over the hover card, the card shows again after the menu has been closed, and the menu keeps working when another window (an Explorer or editor window on a folder) is titled "rigstats".
