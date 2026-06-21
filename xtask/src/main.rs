use std::{
    env,
    path::{Path, PathBuf},
    process::{Command, exit},
};

fn main() {
    let task = env::args().nth(1).unwrap_or_default();
    let result = match task.as_str() {
        "build" => task_build(),
        "test" => task_test(),
        "clippy" => task_clippy(),
        "fmt" => task_fmt(false),
        "fmt-check" => task_fmt(true),
        "setup" => task_setup(),
        "verify" => task_verify(),
        _ => {
            eprintln!("Unknown task: `{task}`");
            eprintln!("Available tasks:");
            eprintln!("  build      — publish sidecar + build egui release binary");
            eprintln!("  test       — run Rust tests (backend + egui)");
            eprintln!("  clippy     — clippy with -D warnings");
            eprintln!("  fmt        — format Rust code (modifies files)");
            eprintln!("  fmt-check  — check Rust formatting without modifying");
            eprintln!("  setup      — install lefthook git hooks (run once after cloning)");
            eprintln!("  verify     — full pipeline (sidecar + tests + clippy + fmt-check)");
            exit(1);
        }
    };
    if let Err(e) = result {
        eprintln!("\nxtask failed: {e}");
        exit(1);
    }
}

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask has no parent directory")
        .to_path_buf()
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let status = cmd
        .current_dir(project_root())
        .status()
        .map_err(|e| format!("failed to spawn `{cmd:?}`: {e}"))?;
    if !status.success() {
        return Err(format!("`{cmd:?}` exited with {status}"));
    }
    Ok(())
}

fn task_build() -> Result<(), String> {
    run(Command::new("dotnet").args([
        "publish",
        "sensor-sidecar/sensor-sidecar.csproj",
        "-c",
        "Release",
        "-r",
        "win-x64",
        "--self-contained",
        "true",
        "-p:PublishSingleFile=true",
    ]))?;
    run(Command::new("cargo").args([
        "build",
        "--release",
        "--manifest-path",
        "src-egui/Cargo.toml",
    ]))?;
    Ok(())
}

fn task_test() -> Result<(), String> {
    run(Command::new("cargo").args(["test", "--manifest-path", "rigstats-backend/Cargo.toml"]))?;
    run(Command::new("cargo").args(["test", "--manifest-path", "src-egui/Cargo.toml"]))?;
    Ok(())
}

fn task_clippy() -> Result<(), String> {
    run(Command::new("cargo").args([
        "clippy",
        "--manifest-path",
        "src-egui/Cargo.toml",
        "--",
        "-D",
        "warnings",
    ]))?;
    Ok(())
}

fn task_fmt(check: bool) -> Result<(), String> {
    let mut cmd = Command::new("cargo");
    cmd.args(["fmt", "--manifest-path", "src-egui/Cargo.toml"]);
    if check {
        cmd.args(["--", "--check"]);
    }
    run(&mut cmd)
}

fn task_setup() -> Result<(), String> {
    if Command::new("lefthook").arg("--version").status().is_err() {
        println!("lefthook not found — installing via winget...");
        if run(Command::new("winget").args([
            "install",
            "--id",
            "evilmartians.lefthook",
            "-e",
            "--accept-package-agreements",
            "--accept-source-agreements",
        ]))
        .is_ok()
        {
            println!("Installed. Open a new terminal and run `cargo xtask setup` again.");
            return Ok(());
        }
        return Err(
            "Could not install lefthook automatically.\n\
             Install manually and re-run:\n\
             \n  winget install evilmartians.lefthook\
             \n  scoop install lefthook"
                .to_owned(),
        );
    }
    run(Command::new("lefthook").arg("install"))?;
    println!("Git hooks installed.");
    Ok(())
}

fn task_verify() -> Result<(), String> {
    println!("── sidecar ─────────────────────────────────────────────────────");
    run(Command::new("dotnet").args([
        "publish",
        "sensor-sidecar/sensor-sidecar.csproj",
        "-c",
        "Release",
        "-r",
        "win-x64",
        "--self-contained",
        "true",
        "-p:PublishSingleFile=true",
    ]))?;

    println!("── test: sidecar ───────────────────────────────────────────────");
    run(Command::new("dotnet").args([
        "test",
        "sensor-sidecar.Tests/sensor-sidecar.Tests.csproj",
        "-c",
        "Release",
    ]))?;

    println!("── test: rigstats-backend ──────────────────────────────────────");
    run(Command::new("cargo").args(["test", "--manifest-path", "rigstats-backend/Cargo.toml"]))?;

    println!("── test: src-egui ──────────────────────────────────────────────");
    run(Command::new("cargo").args(["test", "--manifest-path", "src-egui/Cargo.toml"]))?;

    println!("── clippy ──────────────────────────────────────────────────────");
    run(Command::new("cargo").args([
        "clippy",
        "--manifest-path",
        "src-egui/Cargo.toml",
        "--",
        "-D",
        "warnings",
    ]))?;

    println!("── fmt check ───────────────────────────────────────────────────");
    run(Command::new("cargo").args([
        "fmt",
        "--manifest-path",
        "src-egui/Cargo.toml",
        "--",
        "--check",
    ]))?;

    println!("── all checks passed ───────────────────────────────────────────");
    Ok(())
}
