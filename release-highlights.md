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
