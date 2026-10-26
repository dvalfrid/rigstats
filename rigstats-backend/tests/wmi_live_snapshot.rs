//! Every WMI-based detection on this machine, printed — for comparing a WMI
//! crate upgrade (#547) before and after on real hardware. Ignored: it reads
//! the PC it runs on, so it has no fixed expectation.
//!
//!   cargo test -p rigstats-backend --test wmi_live_snapshot -- --ignored --nocapture

#![cfg(windows)]

use rigstats_backend::hardware;

#[test]
#[ignore = "reads this PC's WMI; run by hand around a wmi upgrade"]
fn print_every_wmi_detection() {
    println!("probe_wmi_status: {:?}", hardware::probe_wmi_status());
    println!("detect_gpu_name: {:?}", hardware::detect_gpu_name());
    println!("detect_gpu_names: {:?}", hardware::detect_gpu_names());
    for d in hardware::detect_gpu_drivers() {
        println!("detect_gpu_drivers: {d:?}");
    }
    println!("detect_system_brand: {:?}", hardware::detect_system_brand());
    println!(
        "detect_motherboard_name: {:?}",
        hardware::detect_motherboard_name()
    );
    println!("detect_model_name: {:?}", hardware::detect_model_name());
    println!("detect_ram_spec: {:?}", hardware::detect_ram_spec());
    println!("detect_ram_details: {:?}", hardware::detect_ram_details());
    println!("detect_ping_target: {:?}", hardware::detect_ping_target());
    println!(
        "detect_battery_present: {:?}",
        hardware::detect_battery_present()
    );
    println!("sample_battery_wmi: {:?}", hardware::sample_battery_wmi());
    let mut models: Vec<_> = hardware::detect_disk_model_map().into_iter().collect();
    models.sort();
    println!("detect_disk_model_map: {models:?}");
    let mut kinds: Vec<_> = hardware::detect_disk_type_map()
        .into_iter()
        .map(|(k, v)| format!("{k}={v:?}"))
        .collect();
    kinds.sort();
    println!("detect_disk_type_map: {kinds:?}");
}
