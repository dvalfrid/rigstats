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

# 1.48.0
- **Bluetooth batteries** — any Bluetooth mouse, keyboard or headset that reports its battery to Windows now shows in the Peripherals panel and the tray card, Logitech devices with charging too, and a device that disconnects is greyed out at once.
- **Laptops: RIGStats survives Eco mode** — on laptops with switchable graphics it now draws on the integrated GPU, so switching the dedicated GPU off (G-Helper's or Armoury Crate's Eco mode, for example when you unplug the charger) no longer closes it.