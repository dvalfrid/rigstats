# Aura USB fixtures

One JSON file per motherboard: the firmware string and the 60-byte config
table its ASUS Aura USB controller returned, plus the zones RIGStats must
derive from it. `AuraFixtureTests` runs every file here.

Adding a board: a user's diagnostics ZIP has a `rigstats-sensor.log` line

    [rigstats-control] Aura: 0x19AF Motherboard firmware 'AULA3-AR32-0304', config 1E9F03…, zones argb1(1), …

Copy the product id, firmware and `config` hex into a new file here, write
the zones you expect (from the board's spec: onboard RGB → a `mainboard`
zone, one `argbN` per addressable header), and run the tests.

Newer exports also carry `lighting-devices.json`: each device's
`product_id`, `firmware` and `config_table` are there directly, and
`hid_scan` lists every HID collection on the machine — entries with
`"known_as": null` are devices RIGStats doesn't support yet.
