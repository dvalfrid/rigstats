fn main() {
  println!("cargo:rerun-if-changed=capabilities");
  println!("cargo:rerun-if-changed=tauri.conf.json");

  tauri_build::try_build(
    tauri_build::Attributes::new().app_manifest(
      tauri_build::AppManifest::new().commands(&[
        "get_settings",
        "preview_opacity",
        "save_settings",
        "close_window",
        "start_window_drag",
        "get_system_name",
        "get_cpu_info",
        "get_gpu_info",
        "get_stats",
      ]),
    ),
  )
  .unwrap();
}
