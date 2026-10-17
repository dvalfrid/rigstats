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

# 1.50.0
- **Lives beside Armoury Crate** — if Armoury Crate's fan service is running, the Fans page says so and RIGStats leaves the fans to it instead of two programs fighting over every fan; the rest of a profile still applies. The message links to ASUS's own uninstall tool (the lighting message now does too) — nothing is stopped or changed behind your back, and a reinstall brings Armoury Crate back.
- **Mute your microphone with Ctrl+Alt+M** — or from the tray, or by clicking the microphone in the dashboard header; it shows green or red in the header, the tray menu and the hover card, follows a change made in Windows or a call app, and a toast says which way it went. Works for whatever microphone Windows uses for calls — or one you pick in Settings — so any headset; the toast can be turned off under Notifications.
- **Microphone level and a live meter** — set how loud your microphone records in Settings and see what it hears while you talk; the tray card shows the same meter next to the microphone.
- **No low-battery alert while a laptop is charging** — the "Battery charge below…" alert now waits until the laptop is actually discharging, like the wireless devices' alerts already did.
