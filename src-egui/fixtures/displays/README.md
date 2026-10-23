# Display layout fixtures

`displays.json` from users' diagnostics ZIPs, verbatim, one per machine, named
after its sensor-fixture slug (#572). Each lists the monitors (virtual-screen
coordinates, primary at 0,0) and the dashboard profile, and marks the monitor
the dashboard was placed on (`is_selected`).

`geometry::tests::real_display_layouts_pick_the_recorded_monitor` runs
`select_profile_monitor` on every file and checks it still picks that monitor.
A change to monitor selection that moves a real user's dashboard fails there.

Add the file from every ZIP whose layout isn't here yet: another monitor
count, size, orientation, or negative coordinates. The list of every corpus is
in `sensor-sidecar.Tests/fixtures/README.md`.
