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

# 1.48.1
- **A crashing graphics driver can't take RIGStats down** — the AMD and NVIDIA driver libraries now run in their own process. If one crashes or hangs (as AMD's did after Windows Update replaced the Adrenalin driver), sensors, fans and lighting keep working, the GPU tab says what happened, and the GPU power limits come back after a restart or a driver update.
- **Updates install even when the service is in trouble** — the installer now stops the sensor service for real before replacing it, instead of failing with "Error opening file for writing".
