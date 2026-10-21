# StdUtils 1.14 — NSIS plug-in

Used by `build/installer.nsi` for `ExecShellAsUser` only: the installer runs as
the administrator who approved UAC, and starts RIGStats (finish page, and after
an in-app update with `--just-updated=VERSION`) through the desktop's Explorer,
so the app runs as the signed-in user, not elevated and not as that
administrator (#416).

- Source: <https://github.com/lordmulder/stdutils/releases/tag/1.14>,
  `StdUtils.2018-10-27.zip` (SHA-256
  `3ffe893dc7477fdb1cac551a86ae017509e1f2d0ebdc7185fd0fbaf20870688c`).
- Files, verbatim from that zip: `Plugins/Unicode/StdUtils.dll`,
  `Include/StdUtils.nsh`, `LGPL.txt`, `LGPL_CLARIFICATION.txt`.
- `StdUtils.dll` SHA-256
  `b72e9013a6204e9f01076dc38dabbf30870d44dfc66962adbf73619d4331601e` —
  `installer.nsi` fails the build if it differs (`STDUTILS_DLL_SHA256`).
- Licence: LGPL 2.1 with the author's clarification: an installer that ships
  the DLL unmodified and calls it through the NSIS plug-in interface is not a
  derivative of it. Don't modify the DLL.

To update: replace the four files from a new release and update both hashes
(here and `STDUTILS_DLL_SHA256`).
