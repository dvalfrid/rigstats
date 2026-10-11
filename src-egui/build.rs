fn main() {
    // The versions the Status window names, read from the projects at build
    // time so they can never go stale: LibreHardwareMonitor from the sensor
    // service's project file, sysinfo and WMI from Cargo.lock.
    println!("cargo:rerun-if-changed=../sensor-sidecar/sensor-sidecar.csproj");
    println!("cargo:rerun-if-changed=../Cargo.lock");
    let csproj =
        std::fs::read_to_string("../sensor-sidecar/sensor-sidecar.csproj").unwrap_or_default();
    let lock = std::fs::read_to_string("../Cargo.lock").unwrap_or_default();
    let lhm = package_reference_version(&csproj, "LibreHardwareMonitorLib");
    println!(
        "cargo:rustc-env=RIGSTATS_LHM_VERSION={}",
        lhm.unwrap_or("unknown")
    );
    for (krate, var) in [
        ("sysinfo", "RIGSTATS_SYSINFO_VERSION"),
        ("wmi", "RIGSTATS_WMI_VERSION"),
    ] {
        println!(
            "cargo:rustc-env={var}={}",
            lock_version(&lock, krate).unwrap_or("unknown")
        );
    }

    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("../assets/icon.ico");
        res.set("ProductName", "RIGStats");
        res.set("FileDescription", "RIGStats");
        res.set("CompanyName", "Code By AB");
        res.set("InternalName", "rigstats");
        res.compile().expect("failed to embed Windows resources");
    }
}

/// `Version="…"` of `<PackageReference Include="{package}" …>`.
fn package_reference_version<'a>(csproj: &'a str, package: &str) -> Option<&'a str> {
    let line = csproj
        .lines()
        .find(|l| l.contains(&format!("Include=\"{package}\"")))?;
    let rest = &line[line.find("Version=\"")? + "Version=\"".len()..];
    Some(&rest[..rest.find('"')?])
}

/// The version of `krate` in Cargo.lock (the first entry if several).
fn lock_version<'a>(lock: &'a str, krate: &str) -> Option<&'a str> {
    let name = format!("name = \"{krate}\"");
    let mut lines = lock.lines();
    lines.by_ref().find(|l| l.trim() == name)?;
    let version = lines.next()?.trim().strip_prefix("version = \"")?;
    version.strip_suffix('"')
}
