use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ui::dispatch_main;

pub(crate) fn dispatch_open_settings() {
    dispatch_main(move || {
        launch_settings_process();
    });
}

fn launch_settings_process() {
    let Some(bin) = resolve_shuo_bin() else {
        eprintln!("[shuo-engine] settings UI launcher not found (shuo)");
        return;
    };

    let mut cmd = Command::new(&bin);
    cmd.arg("settings-ui");

    if let Ok(path) = std::env::var("CONTEXT_CONFIG_PATH") {
        if !path.trim().is_empty() {
            cmd.env("CONTEXT_CONFIG_PATH", path);
        }
    }

    if let Err(error) = cmd.spawn() {
        eprintln!(
            "[shuo-engine] failed to launch settings UI via {}: {}",
            bin.display(),
            error
        );
    }
}

fn resolve_shuo_bin() -> Option<PathBuf> {
    if let Ok(raw) = std::env::var("SHUO_BIN") {
        let path = PathBuf::from(raw);
        if is_executable(&path) {
            return Some(path);
        }
    }
    if let Ok(raw) = std::env::var("HJ_VOICE_BIN") {
        let path = PathBuf::from(raw);
        if is_executable(&path) {
            return Some(path);
        }
    }

    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            roots.push(parent.to_path_buf());
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd);
    }

    for root in roots {
        let mut cur: Option<&Path> = Some(root.as_path());
        for _ in 0..8 {
            let Some(dir) = cur else { break };
            let debug = dir.join(".build/debug/shuo");
            if is_executable(&debug) {
                return Some(debug);
            }
            let release = dir.join(".build/release/shuo");
            if is_executable(&release) {
                return Some(release);
            }
            cur = dir.parent();
        }
    }

    Some(PathBuf::from("shuo"))
}

fn is_executable(path: &Path) -> bool {
    path.exists() && path.is_file()
}
