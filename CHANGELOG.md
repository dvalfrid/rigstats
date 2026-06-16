# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog:
<https://keepachangelog.com/en/1.1.0/>

This project follows Semantic Versioning:
<https://semver.org/>

## [1.31.1](https://github.com/dvalfrid/rigstats/compare/v1.31.0...v1.31.1) (2026-06-16)


### Bug Fixes

* **settings:** apply Override Model Name to header panel ([ee2a49f](https://github.com/dvalfrid/rigstats/commit/ee2a49fb5077ddc51860550627508a653858e22b)), closes [#78](https://github.com/dvalfrid/rigstats/issues/78)
* **updater:** reset to Idle on close so Check for Updates reappears ([dba9cd1](https://github.com/dvalfrid/rigstats/commit/dba9cd136047d14a5e499a2752dca0838c5dfd45))

## [1.31.0](https://github.com/dvalfrid/rigstats/compare/v1.30.0...v1.31.0) (2026-06-16)


### Features

* **debug-log:** human-readable timestamps and severity levels ([d1c4cdf](https://github.com/dvalfrid/rigstats/commit/d1c4cdfc86e02077a81fce6c9e3e400e2977d5a6))
* **disk:** show free space, model name, and disk type per drive ([8f1ca46](https://github.com/dvalfrid/rigstats/commit/8f1ca46e2e90fa20c3b9941ebafb1d8cec2246d2))
* **net:** sub-ms ping precision via TCP connect timing ([a7cb110](https://github.com/dvalfrid/rigstats/commit/a7cb11089564eb6c6334d8535e116e0c889bdfbe))


### Bug Fixes

* **cpu:** clip core scroll area to exactly 2 visible rows ([d91e5bb](https://github.com/dvalfrid/rigstats/commit/d91e5bb9372cbfd90cc72303a15cb82614f65fa0))

## [1.30.0](https://github.com/dvalfrid/rigstats/compare/v1.29.0...v1.30.0) (2026-06-15)


### Features

* **net:** add WAN ping, interface subtitle, and larger UP/DOWN numbers ([a03951d](https://github.com/dvalfrid/rigstats/commit/a03951de08db8002f1c53f3994c6b83982cb2081))


### Bug Fixes

* **sensor:** ensure single-file publish and install companion native DLLs ([289be45](https://github.com/dvalfrid/rigstats/commit/289be45de845f7adc2474f8b609e4b6924b10bd3))
* **status:** load diagnostics off the UI thread to stop freezes ([69e2f74](https://github.com/dvalfrid/rigstats/commit/69e2f74506a624f38c2c3bdbd7699d08913ba976))
* **tray:** remove Show/Hide and fix startup panel clipping ([7d25150](https://github.com/dvalfrid/rigstats/commit/7d2515096dbbf6211c1e00d176e4c2f5c0a5f60d))


### Performance Improvements

* **status:** run diagnostics collection off the UI thread; hide sc.exe console ([bf98584](https://github.com/dvalfrid/rigstats/commit/bf98584981c10afe51b63a5a1585599c8781e4fd))

## [1.29.0](https://github.com/dvalfrid/rigstats/compare/v1.28.0...v1.29.0) (2026-06-15)


### Features

* **gpu:** extend panel with hotspot, FAN/VDEC bars, and code cleanup ([600acaf](https://github.com/dvalfrid/rigstats/commit/600acafe65725f7cbd1b5f6f1b7ed474f5465178))


### Bug Fixes

* **diag:** improve diagnostics and fix dark mode for child accounts ([4882f61](https://github.com/dvalfrid/rigstats/commit/4882f61278146dd4507bd8453a470b84139ee26c))
* **updater:** render commit hash links as clickable hyperlinks ([27b4012](https://github.com/dvalfrid/rigstats/commit/27b4012e013225b3b5fff9be86b379dde7e95bc7))
* **updater:** render version headings as clickable hyperlinks ([53d18d0](https://github.com/dvalfrid/rigstats/commit/53d18d051aa696da66e2e400b1a7e55c63e04900))

## [1.28.0](https://github.com/dvalfrid/rigstats/compare/v1.27.2...v1.28.0) (2026-06-14)


### Features

* dialog theme adapts to OS light/dark mode dynamically ([8945891](https://github.com/dvalfrid/rigstats/commit/8945891ad54cae3310e9288af623a5de09c9467e))


### Bug Fixes

* **egui:** clip of bottom panel when switching floating to fixed mode ([e8b2ca8](https://github.com/dvalfrid/rigstats/commit/e8b2ca83b728cd9b832ddea7da43588baca18bb6))
* **egui:** harden against silent failures and panics ([7ed655a](https://github.com/dvalfrid/rigstats/commit/7ed655a85fefd7a4bd2a5e338d5bd526d4bbc8a2))
* **egui:** make shared Mutex locking poison-tolerant ([cef8223](https://github.com/dvalfrid/rigstats/commit/cef82239ee0e3f36e50f137ff86d00e5865d48b7))


### Performance Improvements

* **egui:** cache parsed changelog in updater dialog ([aa789f2](https://github.com/dvalfrid/rigstats/commit/aa789f28cea576d24c24d19bb3729f97750cb1fa))
* **egui:** skip redundant InnerSize viewport command in fixed mode ([e0e2939](https://github.com/dvalfrid/rigstats/commit/e0e29397b589cceb03afea251419850798e019c0))
* **egui:** throttle 'always behind' Z-order enforcement in floating mode ([d68fdef](https://github.com/dvalfrid/rigstats/commit/d68fdefa338007650d5aef48ca96adda6ad1592b))

## [1.27.2](https://github.com/dvalfrid/rigstats/compare/v1.27.1...v1.27.2) (2026-06-14)


### Bug Fixes

* add Tauri minisign signature to latest.json for pre-1.26 clients ([d839a27](https://github.com/dvalfrid/rigstats/commit/d839a2704d96fa30a8991daa4d745cc719850619))
* install lefthook via winget in cargo xtask setup, update docs/setup.md ([da16d4a](https://github.com/dvalfrid/rigstats/commit/da16d4a5c78ce109b14ce457dc5784c68f3b8a1d))

## [1.27.1](https://github.com/dvalfrid/rigstats/compare/v1.27.0...v1.27.1) (2026-06-13)


### Bug Fixes

* correct exe metadata shown in Task Manager and system tray settings ([9784f4e](https://github.com/dvalfrid/rigstats/commit/9784f4e983582dc3a3b6da54a0b4cba7cc877320))

## [1.27.0](https://github.com/dvalfrid/rigstats/compare/v1.26.0...v1.27.0) (2026-06-12)


### Features

* **engine:** replace Tauri/WebView2 with native egui — eliminates WebView2 idle CPU overhead entirely ([28a41e6](https://github.com/dvalfrid/rigstats/commit/28a41e6dc))
* **battery:** new battery panel — charge %, status, time remaining, power draw with warn/crit colour coding ([e5bfb6c](https://github.com/dvalfrid/rigstats/commit/e5bfb6c25))
* **floating:** floating panel mode — detach any panel as its own borderless window, drag freely, lock positions ([1e3c9e1](https://github.com/dvalfrid/rigstats/commit/1e3c9e19f))
* **themes:** five colour presets — Dark Cyan, Dark Purple, Dark Amber, Dark Teal, Dark Rose with live preview ([1ef8a6d](https://github.com/dvalfrid/rigstats/commit/1ef8a6d1))
* **profiles:** side-monitor profiles (FHD/QHD/4K side) with automatic content scaling ([75305e6](https://github.com/dvalfrid/rigstats/commit/75305e60e))
* **settings:** redesigned four-tab settings dialog — Dashboard, Panels, Alerts, Appearance ([cac5fc9](https://github.com/dvalfrid/rigstats/commit/cac5fc9e7))
* **alerts:** dual battery thresholds — charge % (alert below) and power draw W (alert above, discharge only) ([e5bfb6c](https://github.com/dvalfrid/rigstats/commit/e5bfb6c25))
* **gpu:** AMD iGPU support — sums GPU Core + GPU SoC power for accurate total wattage ([d1f3fb1](https://github.com/dvalfrid/rigstats/commit/d1f3fb19e))
* **updater:** background download with progress, in-app changelog display ([f38a673](https://github.com/dvalfrid/rigstats/commit/f38a673dc))
* **brands:** brand logo support for ROG, MSI, Alienware, Razer, Legion, HP Omen, AORUS, Gigabyte, Predator, Taurus ([926d69e](https://github.com/dvalfrid/rigstats/commit/926d69e09))


### Bug Fixes

* **floating:** panels stay behind other windows via WS\_EX\_NOACTIVATE and Win32 SetWindowPos ([ceb8ae2](https://github.com/dvalfrid/rigstats/commit/ceb8ae282))
* **opacity:** window-level opacity via SetLayeredWindowAttributes replaces broken per-pixel alpha ([8661d36](https://github.com/dvalfrid/rigstats/commit/8661d36f7))
* **cpu:** core bars layout — natural label width eliminates dead space at column left edge ([e5bfb6c](https://github.com/dvalfrid/rigstats/commit/e5bfb6c25))

## [1.26.0](https://github.com/dvalfrid/rigstats/compare/v1.25.1...v1.26.0) (2026-06-08)


### Features

* surface Tauri capability-denied errors instead of swallowing them ([f17793d](https://github.com/dvalfrid/rigstats/commit/f17793dc5025056c9ea5df03744f2274eee88025))


### Bug Fixes

* floating panel positions never saved after drag ([7580971](https://github.com/dvalfrid/rigstats/commit/75809711188e521a32b84174748dd277ed1b03c1))
* grant outer-position permission to panel windows so drag positions save correctly ([1673afa](https://github.com/dvalfrid/rigstats/commit/1673afa8ec849b11dd9603a521b765f56631a2dd))

## [1.25.1](https://github.com/dvalfrid/rigstats/compare/v1.25.0...v1.25.1) (2026-06-08)


### Bug Fixes

* improve floating panel position diagnostics for bug reports ([70756c8](https://github.com/dvalfrid/rigstats/commit/70756c801072fa1782a63c03c465e71e65a2cda8))

## [1.25.0](https://github.com/dvalfrid/rigstats/compare/v1.24.0...v1.25.0) (2026-06-06)


### Features

* **logging:** add stats CSV logging with tray recording indicator ([696352b](https://github.com/dvalfrid/rigstats/commit/696352b2ea3c6f58cbd0151e25a41e7e73a86117))

## [1.24.0](https://github.com/dvalfrid/rigstats/compare/v1.23.0...v1.24.0) (2026-06-05)


### Features

* **floating:** add lock-positions toggle to prevent accidental panel moves ([c490349](https://github.com/dvalfrid/rigstats/commit/c490349966369a10784f9c0a98bc9eba31824f02))


### Bug Fixes

* **floating:** correct BASE_PANEL_HEIGHT for CPU panel ([eb108f4](https://github.com/dvalfrid/rigstats/commit/eb108f4a54b884a94ab8fd2dec18ea3525ed8c76))
* **floating:** eliminate frame gap and WebView2 deadlock when toggling ([4963b20](https://github.com/dvalfrid/rigstats/commit/4963b2089d40aa8649159f4a663260fca55f151d))
* **updater:** show update badge when manual check finds a new version ([2d4282d](https://github.com/dvalfrid/rigstats/commit/2d4282d6023607c92b20222c1b9bf6b9deae8d52))
* **windows:** center popup windows on primary monitor when no tray click ([d6676b5](https://github.com/dvalfrid/rigstats/commit/d6676b5cbae69e1d4a54c41f947ae80d3a66a076))
* **windows:** smooth fade-in on show and eliminate white flash ([ea7e973](https://github.com/dvalfrid/rigstats/commit/ea7e9736ee6c3605fe724b17727da8b6d7627598))

## [1.23.0](https://github.com/dvalfrid/rigstats/compare/v1.22.6...v1.23.0) (2026-06-02)


### Features

* **cpu:** display core bars two per row for denser layout ([addf819](https://github.com/dvalfrid/rigstats/commit/addf819beda75af24bbbd2ef3915dc97bbe40cf3))
* **cpu:** make core bars area dynamically fill available panel height ([502074b](https://github.com/dvalfrid/rigstats/commit/502074bd0e019395e6c36234019e70ccf7100a2b))


### Bug Fixes

* **app:** reapply visible panels after profile change to remove blank gap ([e2b3441](https://github.com/dvalfrid/rigstats/commit/e2b3441d25aeb425aa52aec7596c9d51460bfa6f))
* **commands:** restore always_on_top when leaving floating mode ([15a8ef3](https://github.com/dvalfrid/rigstats/commit/15a8ef39c7ea13d86226764f1be0546d2af08b09))
* **diagnostics:** lower status window always_on_top before save dialog ([6e9fd83](https://github.com/dvalfrid/rigstats/commit/6e9fd8351227ee7f935c11f69948e348b1917956))
* **gpu:** merge GPU+VRAM bars onto one row to free space for sparkline ([34d5ead](https://github.com/dvalfrid/rigstats/commit/34d5ead534c9d90e0a9d8d5b111ede3e2088457f))
* **windows:** center all modal windows on the tray monitor consistently ([ce8dcc7](https://github.com/dvalfrid/rigstats/commit/ce8dcc720f77164b53c69c7922bbbda886af315a))

## [1.22.6](https://github.com/dvalfrid/rigstats/compare/v1.22.5...v1.22.6) (2026-05-31)


### Bug Fixes

* **installer:** fix pnputil cmd quoting, use ${VERSION} for version, call pnputil directly without cmd /C ([02a6764](https://github.com/dvalfrid/rigstats/commit/02a6764c6537e91e7481b8aadeeba7fdaa82a2a6))
* **installer:** treat pnputil exit 259 as success on reinstall ([73fbb65](https://github.com/dvalfrid/rigstats/commit/73fbb65e5088dec33b787063f38e70c40f1bb36a))
* **installer:** use Sysnative to bypass WOW64 redirection for pnputil ([ace42bc](https://github.com/dvalfrid/rigstats/commit/ace42bc21008f40fd16a42604824c6149b5d030b))

## [1.22.5](https://github.com/dvalfrid/rigstats/compare/v1.22.4...v1.22.5) (2026-05-31)


### Bug Fixes

* **installer:** add pawnio.inf pre-check and driver store fallback on install failure ([181b369](https://github.com/dvalfrid/rigstats/commit/181b369fcf991ca362179c97c5d4c99f7af0c955))
* **installer:** add version, instdir, timestamp and sc create output to install log ([5c762f6](https://github.com/dvalfrid/rigstats/commit/5c762f6be245fdc81d2c421fd671b5e399a4779a))
* **installer:** use full path to pnputil.exe — not reliably in PATH in elevated NSIS context ([f4907a5](https://github.com/dvalfrid/rigstats/commit/f4907a52c2b8854cc77c97cce0f6bf49bbddf8a6))

## [1.22.4](https://github.com/dvalfrid/rigstats/compare/v1.22.3...v1.22.4) (2026-05-31)


### Bug Fixes

* **gpu:** combine 3D and VDEC into single bar row to prevent sparkline overflow ([0e941f3](https://github.com/dvalfrid/rigstats/commit/0e941f3b99cd241d977ec12078c208795c49a9ae))
* **installer:** use ReadEnvStr to read %PROGRAMDATA% — $COMMONAPPDATA is ([b24313b](https://github.com/dvalfrid/rigstats/commit/b24313b49135d835ecf51021e17f9b72c5fcccdb))

## [1.22.3](https://github.com/dvalfrid/rigstats/compare/v1.22.2...v1.22.3) (2026-05-31)


### Bug Fixes

* **gpu:** add GPU VR SoC as fallback temperature for AMD iGPU (Radeon 890M) ([1988794](https://github.com/dvalfrid/rigstats/commit/1988794a04c1cee593d12dc21aae78c3da24307b))
* **installer:** use $COMMONAPPDATA instead of $PROGRAMDATA in NSIS hook ([3a8a146](https://github.com/dvalfrid/rigstats/commit/3a8a146b6b91f8eaacbef567aea18daab618aa76))

## [1.22.2](https://github.com/dvalfrid/rigstats/compare/v1.22.1...v1.22.2) (2026-05-30)


### Bug Fixes

* ensure PawnIO and .NET 10 changes are included in release ([2a6717b](https://github.com/dvalfrid/rigstats/commit/2a6717b8c773b332df9ff62f16948597de66f6ed))

## [1.22.1](https://github.com/dvalfrid/rigstats/compare/v1.22.0...v1.22.1) (2026-05-30)


### Bug Fixes

* **installer:** fix PawnIO catalog mismatch caused by git line ending conversion ([1bfa927](https://github.com/dvalfrid/rigstats/commit/1bfa92760fc7320ce78b5702560ca41a03789909))
* **installer:** log pnputil output to install log for easier diagnostics ([90e8b52](https://github.com/dvalfrid/rigstats/commit/90e8b52494cfb53f68587df58946833e414a00bb))

## [1.22.0](https://github.com/dvalfrid/rigstats/compare/v1.21.1...v1.22.0) (2026-05-30)


### Features

* **diagnostics:** write sensor tree to file on sidecar start and include in diagnostics ZIP ([0b9a2df](https://github.com/dvalfrid/rigstats/commit/0b9a2df2b935b3e1cbe0689555a144b4601858e2))
* **sidecar:** replace WinRing0 with PawnIO via LHM 0.9.6 ([8f94e36](https://github.com/dvalfrid/rigstats/commit/8f94e367304387149771e289415f0b4bdae2750b))

## [1.21.1](https://github.com/dvalfrid/rigstats/compare/v1.21.0...v1.21.1) (2026-05-30)


### Bug Fixes

* **installer:** use path exclusion for Defender instead of process ([8a2bf99](https://github.com/dvalfrid/rigstats/commit/8a2bf99646a34c1baefb439df57e3c678d930a34))

## [1.21.0](https://github.com/dvalfrid/rigstats/compare/v1.20.0...v1.21.0) (2026-05-29)


### Features

* **diagnostics:** add sidecar log and service status; replace LHM task probe ([1f07487](https://github.com/dvalfrid/rigstats/commit/1f0748730c9ed99afe10aefbaa1bcf76e88d7089))
* **sidecar:** replace LHM HTTP polling with named pipe sensor sidecar ([5867d1d](https://github.com/dvalfrid/rigstats/commit/5867d1df808e982ccf8588be3d25f3e180941167))
* **sidecar:** run as Windows Service with PipeSecurity and auto-restart ([78110b0](https://github.com/dvalfrid/rigstats/commit/78110b018f4d81d0b029e7f689db0233e03cfecd))
* **status:** replace LHM task fields with sidecar service status ([00c9583](https://github.com/dvalfrid/rigstats/commit/00c95837141021b659aee918fab8bd8e77b67420))


### Bug Fixes

* **installer:** add Defender exclusion for rigstats-sensor.exe ([70a7044](https://github.com/dvalfrid/rigstats/commit/70a7044a65e7eb5d4695df59009326e66edd3719))
* **sidecar:** grant full GENERIC_READ rights on pipe ACL ([18279ea](https://github.com/dvalfrid/rigstats/commit/18279ea2e47cc7d97e3e077898ccda2e03e7f18e))

## [1.20.0](https://github.com/dvalfrid/rigstats/compare/v1.19.0...v1.20.0) (2026-05-29)


### Features

* add battery panel and fix clock/header UI issues ([ed7d35d](https://github.com/dvalfrid/rigstats/commit/ed7d35d9574cd7f0bb0a4d7e47b75c6268d21d24))
* **diag:** expand diagnostics ZIP and add regression tests ([3df0868](https://github.com/dvalfrid/rigstats/commit/3df0868fffa73e8c74d400fc192439a16544b1a2))
* **panels:** remove right-click context menu and standardize secondary panel heights to 320px ([5b46c71](https://github.com/dvalfrid/rigstats/commit/5b46c71d8eebcfe7300de9f83e3c68e3415db9b5))
* **settings:** redesign with four-tab UI, battery alerts, and DPI fix ([adac083](https://github.com/dvalfrid/rigstats/commit/adac08314c159480de732600e67f866b7dbc083b))


### Bug Fixes

* remove RAM sparkline and fix SPEED/TYPE detection on laptops ([b0bba5d](https://github.com/dvalfrid/rigstats/commit/b0bba5d4219891cb481bbdfcf1427dca0d4347a4))
* **ui:** sync clock/RAM/battery font size and add battery power colours ([c80d3d2](https://github.com/dvalfrid/rigstats/commit/c80d3d2a9df60c7397be597836f857c5f92a9e91))

## [1.19.0](https://github.com/dvalfrid/rigstats/compare/v1.18.0...v1.19.0) (2026-04-18)


### Features

* **gpu:** add multi-GPU selector and persist preferred GPU to prevent auto-switching ([af23665](https://github.com/dvalfrid/rigstats/commit/af23665dfbfe83be35b49525cb9785c5aa8dd873))

## [1.18.0](https://github.com/dvalfrid/rigstats/compare/v1.17.0...v1.18.0) (2026-04-17)


### Features

* **floating:** add floating panel scale setting with live preview ([8305caf](https://github.com/dvalfrid/rigstats/commit/8305cafb20be2d68bc99533aaa02fb193e6eb42b))

## [1.17.0](https://github.com/dvalfrid/rigstats/compare/v1.16.0...v1.17.0) (2026-04-16)


### Features

* **gpu:** show active GPU when iGPU and dGPU coexist ([c6810ec](https://github.com/dvalfrid/rigstats/commit/c6810ec5cf30ba86396a95f96f317d403cc0120b))


### Bug Fixes

* **ci:** use cargo generate-lockfile to sync Cargo.lock after release ([79d2e15](https://github.com/dvalfrid/rigstats/commit/79d2e1577e844808e2e2ea3362af2a3cd2420757))
* **gpu:** prevent ring clipping when D3D/VDEC bars are visible ([08f1497](https://github.com/dvalfrid/rigstats/commit/08f1497d865121045371635c91afb0991b6a5647))
* **layout:** clip procList from bottom and scale ring-wrap gap in floating panels ([2be2923](https://github.com/dvalfrid/rigstats/commit/2be292302de3b5a7b298378926890f7ada262f70))
* **lhm:** extract temp and power for AMD iGPU (Radeon 890M) ([55173c6](https://github.com/dvalfrid/rigstats/commit/55173c6d988e0809f9f3019ab6bd0b94a912a9bd))
* **lhm:** fall back to GPU Memory Junction for hotspot on laptop GPUs ([39dbd1a](https://github.com/dvalfrid/rigstats/commit/39dbd1a69283dc35df33e059d3fa18d5bfbff785))
* **lhm:** show AMD SVI2 voltage rails in motherboard panel on laptops ([1bee073](https://github.com/dvalfrid/rigstats/commit/1bee073fc8f694098a07a732c74acb12da2e68b0))
* **startup:** reload webview when WebView2 fails to init at Windows boot ([0ce4bb6](https://github.com/dvalfrid/rigstats/commit/0ce4bb64a0f9e385d83e699398fa8149cc1c9e67))

## [1.16.0](https://github.com/dvalfrid/rigstats/compare/v1.15.0...v1.16.0) (2026-04-14)


### Features

* add floating panel layout mode ([78663c2](https://github.com/dvalfrid/rigstats/commit/78663c255d9c5afc38d2cd9d5bab02cf7e96e1bb))


### Bug Fixes

* **floating:** stabilize panel mode sync, drag behavior, and spark history parity ([a4490c5](https://github.com/dvalfrid/rigstats/commit/a4490c5800cef094c3580c5bdebf65ef5f436062))
* **layout:** correct CSS overflow on high-DPI displays ([d7314b5](https://github.com/dvalfrid/rigstats/commit/d7314b5a3a9b9a2da9c35fb3370e1058cb569077))
* **layout:** replace hardcoded sizes with scaled CSS variables across all panels ([2187c7b](https://github.com/dvalfrid/rigstats/commit/2187c7bfca0ed07caae3976b7542b6c1466d7d25))

## [1.15.0](https://github.com/dvalfrid/rigstats/compare/v1.14.0...v1.15.0) (2026-04-02)


### Features

* add process monitor panel ([f498716](https://github.com/dvalfrid/rigstats/commit/f498716dba6073b60d265c5bcd9b4aa09178f961))

## [1.14.0](https://github.com/dvalfrid/rigstats/compare/v1.13.0...v1.14.0) (2026-04-01)


### Features

* add customisable colour themes to the dashboard ([6ecfe9f](https://github.com/dvalfrid/rigstats/commit/6ecfe9f4e253969e5c0581b5b6d9de41d80fbebe))


### Bug Fixes

* update badge click blocked by header window-drag mousedown ([523fb94](https://github.com/dvalfrid/rigstats/commit/523fb942c2239dee7c43624ceb822c90b4771614))

## [1.13.0](https://github.com/dvalfrid/rigstats/compare/v1.12.0...v1.13.0) (2026-03-31)


### Features

* extend GPU panel with memory clock and D3D workload bars ([1f67a11](https://github.com/dvalfrid/rigstats/commit/1f67a11a14592f5249f8139f6e0daecd102ae367))


### Bug Fixes

* handle GitHub release manifest race condition gracefully ([d1a15a1](https://github.com/dvalfrid/rigstats/commit/d1a15a1c3af8f5d1c40ea2d8b395a2e797b437de))

## [1.12.0](https://github.com/dvalfrid/rigstats/compare/v1.11.0...v1.12.0) (2026-03-30)


### Features

* retry WMI hardware detection after startup if fields are missing ([72392ba](https://github.com/dvalfrid/rigstats/commit/72392ba49b8d05c14eb18aee7666fb9e044246f9))
* show read and write separately in storage sparkline ([f17140c](https://github.com/dvalfrid/rigstats/commit/f17140cbab2dcdce96d36bfd542a5d767ae4df1a))


### Bug Fixes

* show both upload and download in network sparkline with correct colours ([181c06b](https://github.com/dvalfrid/rigstats/commit/181c06bc5dfb4e44d76f47b2d27186cfa149bf97))
* truncate long rig name and reject version strings as model name ([edcea41](https://github.com/dvalfrid/rigstats/commit/edcea41878997386893525b6eb24ca224467753b))
* update spark tests to reflect renamed history series ([6a33251](https://github.com/dvalfrid/rigstats/commit/6a33251e538257db82c8454a76c520e70237e0ee))

## [1.11.0](https://github.com/dvalfrid/rigstats/compare/v1.10.1...v1.11.0) (2026-03-27)


### Features

* add Motherboard panel with fans, temps, and voltage rails ([819176e](https://github.com/dvalfrid/rigstats/commit/819176ecffec83945b05a0e28bebdd7fa695eba6))
* redesign dialogs with Windows 11 dark-mode aesthetic ([9a1aa2a](https://github.com/dvalfrid/rigstats/commit/9a1aa2a062bbe666e85bc5103dac940c019f8b21))

## [1.10.1](https://github.com/dvalfrid/rigstats/compare/v1.10.0...v1.10.1) (2026-03-26)


### Bug Fixes

* stop LHM before file extraction to prevent locked DLL errors on update ([a6ca37e](https://github.com/dvalfrid/rigstats/commit/a6ca37e9a7366398de26182245d272d832025c40))

## [1.10.0](https://github.com/dvalfrid/rigstats/compare/v1.9.3...v1.10.0) (2026-03-26)


### Features

* cycle disk drives in pages of three when more than three are present ([f1c55d5](https://github.com/dvalfrid/rigstats/commit/f1c55d5181f1545daf6e9758301ccf58f49c8459))


### Bug Fixes

* GPU sensors, SATA SSD temps, and DDR5 RAM type detection ([9c4b4de](https://github.com/dvalfrid/rigstats/commit/9c4b4de3996571154ba29dda9a92dc1725e4c502))
* sum all disk throughput, LPDDR types, VRAM fallback as Option ([a8028f0](https://github.com/dvalfrid/rigstats/commit/a8028f07399b63d6cc82ba63887928fe4bafbf41))

## [1.9.3](https://github.com/dvalfrid/rigstats/compare/v1.9.2...v1.9.3) (2026-03-25)


### Bug Fixes

* correct CPU temp and power sensor matching for Intel CPUs ([e53ae79](https://github.com/dvalfrid/rigstats/commit/e53ae797e2ad27d50464bb29251d7e181a874380))
* remove set_fullscreen(false) from set_main_height to prevent window shift on save ([10d602e](https://github.com/dvalfrid/rigstats/commit/10d602ed1f737d1562b9f7f98cb8a884534399d8))
* stable window placement and correct CPU sensor parsing ([1501395](https://github.com/dvalfrid/rigstats/commit/1501395dfd726fbfb4c2ff1d0fb2036c2c0812ab))

## [1.9.2](https://github.com/dvalfrid/rigstats/compare/v1.9.1...v1.9.2) (2026-03-25)


### Bug Fixes

* FHD Sidebar profile, GPU dGPU fix, panel-hide window resize ([2b72c27](https://github.com/dvalfrid/rigstats/commit/2b72c2792ee8ff60bf1501ea1b7562428e8b6b20))

## [1.9.1](https://github.com/dvalfrid/rigstats/compare/v1.9.0...v1.9.1) (2026-03-24)


### Bug Fixes

* repair three update-flow bugs ([b2a6b6e](https://github.com/dvalfrid/rigstats/commit/b2a6b6eb3209fc494b1c1d1cad00388dead070d4))

## [1.9.0](https://github.com/dvalfrid/rigstats/compare/v1.8.1...v1.9.0) (2026-03-24)


### Features

* add DDR5/DDR4 DIMM temperature to RAM panel ([4d65740](https://github.com/dvalfrid/rigstats/commit/4d657401a57272814ba4c486fc89021606954e96))
* temperature threshold alerts with configurable thresholds and notifications ([6cac71e](https://github.com/dvalfrid/rigstats/commit/6cac71e1ac6a63b44a572781ccf1e3fd9dcaf327))


### Bug Fixes

* show disk temp when WMI model map is empty; expand diagnostics ([61dbf3f](https://github.com/dvalfrid/rigstats/commit/61dbf3f76c0392cb5c98cc89470b42b108f241f9))

## [1.8.1](https://github.com/dvalfrid/rigstats/compare/v1.8.0...v1.8.1) (2026-03-24)


### Bug Fixes

* repair updater — missing event permission blocked install ([d96795f](https://github.com/dvalfrid/rigstats/commit/d96795f05b0b65f71c6262d71a2ec2b1314cb44a))

## [1.8.0](https://github.com/dvalfrid/rigstats/compare/v1.7.1...v1.8.0) (2026-03-24)


### Features

* add NVMe/SSD temperature display to disk panel ([8ab3187](https://github.com/dvalfrid/rigstats/commit/8ab3187ca2e264901ecf5fa580b66aa2678f6934))


### Bug Fixes

* write install.log to ProgramData instead of AppData ([eb968ac](https://github.com/dvalfrid/rigstats/commit/eb968acc80d1698ea9e8b03926c89004a40206fd))

## [1.7.1](https://github.com/dvalfrid/rigstats/compare/v1.7.0...v1.7.1) (2026-03-23)


### Bug Fixes

* **updater:** show full changelog history and polish no-update view ([b6ed4dd](https://github.com/dvalfrid/rigstats/commit/b6ed4dd1091e56a6b563c6d1d96fd0a853c74306))

## [1.7.0](https://github.com/dvalfrid/rigstats/compare/v1.6.0...v1.7.0) (2026-03-23)


### Features

* **updater:** move changelog to updater dialog and refine UX ([b14739c](https://github.com/dvalfrid/rigstats/commit/b14739c331a9b81790b94d540ad168cbe16297a4))

## [1.6.0](https://github.com/dvalfrid/rigstats/compare/v1.5.1...v1.6.0) (2026-03-23)


### Features

* **updater:** add auto-update with background check and progress UI ([3114d2d](https://github.com/dvalfrid/rigstats/commit/3114d2d8949f54bb71b52797d0ca61689d032c50))


### Bug Fixes

* remove window decorations and force Node 24 in CI ([e2cb4fd](https://github.com/dvalfrid/rigstats/commit/e2cb4fd60e775bfd91dcdf7042f69619248980ee))

## [1.5.1](https://github.com/dvalfrid/rigstats/compare/v1.5.0...v1.5.1) (2026-03-22)


### Bug Fixes

* **lhm:** detect Intel CPU temperature via priority sensor list ([43c58b3](https://github.com/dvalfrid/rigstats/commit/43c58b3d981c381e77bb3600bebd431b05e6c46a))

## [1.5.0](https://github.com/dvalfrid/rigstats/compare/v1.4.0...v1.5.0) (2026-03-21)


### Features

* **diagnostics:** add installer log to diagnostics ZIP and pretty-print all JSON files ([04611e6](https://github.com/dvalfrid/rigstats/commit/04611e6bbfd187763172ff76fdf2d70e58a25bc0))


### Bug Fixes

* **hardware:** add PowerShell fallback for model name detection and filter placeholder values ([bf1b5b5](https://github.com/dvalfrid/rigstats/commit/bf1b5b55f516f4bba7f10a87ca8cd4b90b4e96a8))
* **lhm:** detect Intel CPU temperature via priority sensor list (Core (Tctl/Tdie) → CPU Package → Core Average)
* **settings:** auto-detect model name immediately when field is cleared ([3b3e4f8](https://github.com/dvalfrid/rigstats/commit/3b3e4f8e0d679c773d51f05a7675f4817d77a183))

## [1.4.0](https://github.com/dvalfrid/rigstats/compare/v1.3.1...v1.4.0) (2026-03-21)


### Features

* **autostart:** add launch-at-startup toggle to Settings ([c69057c](https://github.com/dvalfrid/rigstats/commit/c69057cf20402b5c8c1478004a83e28de1ef168c))
* **ui:** add drag-to-reorder panel ordering ([2fb5f92](https://github.com/dvalfrid/rigstats/commit/2fb5f92ea4ff6d2c9e89ffc6bbddb3ddcfe43339))


### Bug Fixes

* **ui:** add consistent hover animations to all dialog buttons and panel toggles ([d203fd0](https://github.com/dvalfrid/rigstats/commit/d203fd01ad5de4889f80f3e1db1431f309fc8d77))

## [1.3.1](https://github.com/dvalfrid/rigstats/compare/v1.3.0...v1.3.1) (2026-03-20)


### Bug Fixes

* **lhm:** fix scheduled task setup and improve diagnostics ([dc6c25c](https://github.com/dvalfrid/rigstats/commit/dc6c25c4247af0fcd18f66dd99889f27345d5606))

## [1.3.0](https://github.com/dvalfrid/rigstats/compare/v1.2.2...v1.3.0) (2026-03-20)


### Features

* **about:** add changelog viewer with version history ([a99f6e4](https://github.com/dvalfrid/rigstats/commit/a99f6e484d3caabf67b7cca51a0e4434d4c838bf))
* **diagnostics:** add display topology to diagnostics export ([801fb86](https://github.com/dvalfrid/rigstats/commit/801fb86295189b4ed0fea9584fc3709f988d93e0))
* rebrand to RIGStats and add SEO + custom domain support ([309ae48](https://github.com/dvalfrid/rigstats/commit/309ae48ee73f363fafeaf3ed144c4c9fc1e03e18))
* **website:** add product website with GitHub Pages deployment ([bc35a02](https://github.com/dvalfrid/rigstats/commit/bc35a021c70895ea7c75e4cac55b5d66d3c12b1c))

## [1.2.2](https://github.com/dvalfrid/rigstats/compare/v1.2.1...v1.2.2) (2026-03-20)


### Bug Fixes

* fill dialog shell to window height and pin buttons to bottom ([c6114b7](https://github.com/dvalfrid/rigstats/commit/c6114b7a8f229516dcd9e6e6e0b84a7ed5c4fbc2))

## [1.2.1](https://github.com/dvalfrid/rigstats/compare/v1.2.0...v1.2.1) (2026-03-20)


### Bug Fixes

* correct Cargo.toml version to 1.2.0 and add release-please marker ([337fbde](https://github.com/dvalfrid/rigstats/commit/337fbde6761610c8e64539cec4546a02457b53df))

## [1.2.0](https://github.com/dvalfrid/rigstats/compare/v1.1.0...v1.2.0) (2026-03-19)


### Features

* **display:** add profile-aware dashboard layouts with live size preview and monitor fallback improvements ([31ba3a2](https://github.com/dvalfrid/rigstats/commit/31ba3a22ceea8e05478bc8bc23c0b1234491792e))

## [1.1.0](https://github.com/dvalfrid/rigstats/compare/v1.0.1...v1.1.0) (2026-03-14)


### Features

* add Collect Diagnostics export to Status dialog ([745ef04](https://github.com/dvalfrid/rigstats/commit/745ef04aa1b526c05c3da7698f65731cb6dd59e7))
* add panel visibility control + live preview and unify dialogs to ultra-compact layout ([73323b1](https://github.com/dvalfrid/rigstats/commit/73323b13210190332e2106e020f0571c999acc8d))
* Add support for Acer, Alienware, Gigabyte, HP Omen, Lenovo, MSI, Razer ([7b0b99f](https://github.com/dvalfrid/rigstats/commit/7b0b99f0c0cb1bd5b5b72e57854d764b4947803f))


### Bug Fixes

* eliminate memory leaks from reqwest client churn and orphaned Tauri listeners ([0a7cbf7](https://github.com/dvalfrid/rigstats/commit/0a7cbf7c8e3a3ffe23e5fc65ad2c1651c061ef05))

## [1.0.1](https://github.com/dvalfrid/rigstats/compare/v1.0.0...v1.0.1) (2026-03-14)


### Bug Fixes

* **Fix installation and debugging:** Fix installation and Status page for debugging ([2efb9bb](https://github.com/dvalfrid/rigstats/commit/2efb9bb7fa17fa328ba74f3eb351d0d7e769d109))
* Remove support for msi and some UI fixes ([0e676b3](https://github.com/dvalfrid/rigstats/commit/0e676b36646190d6e9b5e3e49dbee2aee73c3db4))

## 1.0.0 (2026-03-14)


### Miscellaneous Chores

* bootstrap first release ([9b40fc4](https://github.com/dvalfrid/rigstats/commit/9b40fc4a5e893cd57d1d710a2dfb54877b9b99e8))
