//! What sysinfo reports on this machine, the way the poll loop reads it —
//! for comparing a sysinfo upgrade (#546) before and after on real hardware.
//! Ignored: it reads the PC it runs on. Static values must match exactly;
//! load, frequency and traffic only have to be plausible.
//!
//!   cargo test -p rigstats-backend --test sysinfo_live_snapshot -- --ignored --nocapture

use sysinfo::{
    CpuRefreshKind, DiskRefreshKind, Disks, MemoryRefreshKind, Networks, RefreshKind, System,
};

#[test]
#[ignore = "reads this PC; run by hand around a sysinfo upgrade"]
fn print_what_the_poll_loop_reads() {
    let mut sys = System::new();
    sys.refresh_cpu_all();
    println!("static cpus: {}", sys.cpus().len());
    println!(
        "static brand: {:?}",
        sys.cpus().first().map(|c| c.brand().to_string())
    );
    println!("static host: {:?}", System::host_name());
    println!("static os: {:?}", System::long_os_version());
    let mut disks = Disks::new_with_refreshed_list();
    let mut networks = Networks::new_with_refreshed_list();
    let mut names: Vec<_> = networks.iter().map(|(n, _)| n.clone()).collect();
    names.sort();
    println!("static networks: {names:?}");

    std::thread::sleep(std::time::Duration::from_millis(1500));
    sys.refresh_specifics(
        RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::nothing().with_cpu_usage().with_frequency())
            .with_memory(MemoryRefreshKind::everything()),
    );
    println!("static ram_total: {}", sys.total_memory());
    println!("live ram_used: {}", sys.used_memory());
    println!("live cpu_load: {}", sys.global_cpu_usage());
    let cpus = sys.cpus();
    let freq = cpus.iter().map(|c| c.frequency() as f64).sum::<f64>() / cpus.len() as f64;
    println!("live cpu_freq_avg_mhz: {freq:.0}");
    println!(
        "live cpu_cores: {:?}",
        cpus.iter().map(|c| c.cpu_usage() as u8).collect::<Vec<_>>()
    );
    println!("live uptime>0: {}", System::uptime() > 0);

    for disk in disks.list_mut() {
        disk.refresh_specifics(DiskRefreshKind::nothing().with_storage());
    }
    let mut list: Vec<_> = disks
        .iter()
        .filter(|d| d.total_space() > 1_000_000_000)
        .map(|d| {
            (
                d.mount_point().to_string_lossy().to_string(),
                d.total_space(),
                d.available_space() > 0,
            )
        })
        .collect();
    list.sort();
    println!("static disks: {list:?}");

    networks.refresh(true);
    let traffic: u64 = networks
        .iter()
        .map(|(_, d)| d.received() + d.transmitted())
        .sum();
    println!("live network_bytes_since_last: {traffic}");
}
