# Vendored NSIS plugins

## UAC plug-in (Anders Kjersem) — v0.2.4c

Used by `build/installer.nsi` to implement the "administrator broker" model:
the installer starts unelevated as the real interactive user, elevates an inner
instance only for the machine-wide work (driver, service, Program Files, HKLM),
and runs per-user actions + the final app launch back in the **unelevated user's
context**. This ensures RIGStats is installed/launched for the user actually
sitting at the machine, even when a *different* administrator account approves
the UAC prompt (over-the-shoulder elevation).

### Files

| File | Purpose |
| --- | --- |
| `UAC.nsh` | Include macros (`UAC_RunElevated`, `UAC_AsUser_ExecShell`, …) |
| `x86-unicode/UAC.dll` | Plugin DLL used by the build (`Unicode True`) |
| `x86-ansi/UAC.dll` | ANSI variant (kept for completeness; not used) |
| `LICENSE.txt` | zlib/libpng license |
| `History.txt` | Upstream changelog |

`build/installer.nsi` references this directory via `!addplugindir` and
`!addincludedir`, so nothing needs to be copied into the NSIS install directory
(works the same locally and in CI where NSIS comes from choco).

### Provenance (pinned)

- Source: official NSIS wiki distribution
  <https://nsis.sourceforge.io/UAC_plug-in> → `UAC.zip`
  (`https://nsis.sourceforge.io/mediawiki/images/8/8f/UAC.zip`)
- Version: v0.2.4c (2015-05-26), author Anders Kjersem
- `UAC.zip` SHA256: `20E3192AF5598568887C16D88DE59A52C2CE4A26E42C5FB8BEE8105DCBBD1760`
- `x86-unicode/UAC.dll` SHA256: `2F7F8FC05DC4FD0D5CDA501B47E4433357E887BBFED7292C028D99C73B52DC08`
- `x86-ansi/UAC.dll` SHA256: `32DD7269ABF5A0E5DB888E307D9DF313E87CEF4F1B597965A9D8E00934658822`

### License

zlib/libpng (permissive, no copyleft) — compatible with this project's MIT
license. The upstream copyright notice is retained in `LICENSE.txt`; per the
license terms it must not be removed. The plugin is used unmodified.

> Note: the upstream wiki marks the plugin "deprecated", but it remains the
> de-facto standard for the admin-broker pattern on Windows 10/11 and has no
> official successor. It is used here unmodified.
