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

# 1.51.0
- **Lighter on your PC** — RIGStats now uses about 1 % of one CPU core while it runs, down from about 2.5 %: it reads the running programs in one Windows call and without a pool of threads woken every second, and it no longer asks Windows about a battery a desktop doesn't have.
- **Razer, HyperX and SteelSeries lighting** — the Razer DeathAdder 2013 and Razer Blade 15 (2018) keyboard, the HyperX Alloy Origins Core keyboard and the SteelSeries Siberia 350 headset follow your lighting profile, each on its own protocol (left alone while Razer Synapse, HyperX NGENUITY or SteelSeries GG runs). Built from the openrazer and OpenRGB projects' documentation and marked as not yet verified — a report from an owner confirms it.
- **Clearer hardware readings** — when the CPU temperature and power are missing, Status now says why: on Windows 10 older than version 1809 the driver they are read through can't be installed, otherwise how to install it. And the RAM speed shows correctly on boards that report it at half (e.g. DDR4 1200 instead of 2400 MT/s).
- **Graphics card stays read after sleep** — waking the PC from sleep no longer looks like a sensor crash: before, three sleeps in a day made RIGStats stop reading the graphics card until a restart, and the GPU panel showed the integrated graphics instead. And whenever the card you picked in Settings isn't being read, the GPU panel now shows a small amber dot next to the card dots; hover it to see which card is shown instead.
- **Try what's next** — Settings → General → Updates → "Release candidates too" offers release candidates as updates, before a release ships; you move on to the release by yourself. The default stays releases only.
- **Diagnostics that catch a reading going missing** — the diagnostics ZIP now also holds every sensor as read when you collect it, next to the readings from when the service started, so a value that disappears while RIGStats runs can be traced.
