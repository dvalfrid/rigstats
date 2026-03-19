fn main() {
  println!("cargo:rerun-if-changed=capabilities");
  println!("cargo:rerun-if-changed=tauri.conf.json");

  tauri_build::try_build(
    tauri_build::Attributes::new().app_manifest(
      tauri_build::AppManifest::new().commands(&[
        "get_settings",
        "get_about_info",
        "preview_opacity",
        "preview_profile",
        "preview_visible_panels",
        "save_settings",
        "close_window",
        "start_window_drag",
        "get_system_name",
        "get_system_brand",
        "get_cpu_info",
        "get_gpu_info",
        "get_stats",
        "log_frontend_error",
        "collect_diagnostics",
      ]),
    ),
  )
  .unwrap();
}
