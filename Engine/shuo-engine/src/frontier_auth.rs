use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use uuid::Uuid;

use crate::frontier_protocol::{DEFAULT_APP_KEY, DEFAULT_FRONTIER_WS_URL};
use crate::state::now_millis;
use crate::Args;
#[cfg(feature = "latency-bench")]
use crate::FrontierProfile;

const MIN_USABLE_TTL_SECS: u64 = 300;
const ANDROID_REGISTER_URL: &str = "https://log.snssdk.com/service/2/device_register/";
const ANDROID_SETTINGS_URL: &str = "https://is.snssdk.com/service/settings/v3/";
const ANDROID_FRONTIER_WS_BASE_URL: &str =
    "wss://frontier-audio-ime-ws.doubao.com/ocean/api/v1/ws";
const ANDROID_AID: &str = "401734";
const ANDROID_APP_NAME: &str = "oime";
const ANDROID_VERSION_CODE: &str = "100102018";
const ANDROID_VERSION_NAME: &str = "1.1.2";
const ANDROID_CHANNEL: &str = "official";
const ANDROID_PACKAGE: &str = "com.bytedance.android.doubaoime";
const ANDROID_USER_AGENT: &str = "com.bytedance.android.doubaoime/100102018 (Linux; U; Android 16; en_US; Pixel 7 Pro; Build/BP2A.250605.031.A2; Cronet/TTNetVersion:94cf429a 2025-11-17 QuicVersion:1f89f732 2025-05-08)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthResolveMode {
    Default,
    ForceRefresh,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FrontierAuthMaterial {
    pub(crate) token: String,
    pub(crate) app_key: String,
    pub(crate) exp: u64,
    pub(crate) source: String,
    pub(crate) ws_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct FrontierAuthCacheRecord {
    version: u32,
    token: String,
    app_key: String,
    exp: u64,
    source: String,
    cached_at_ms: u64,
    #[serde(default)]
    ws_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AndroidVirtualDeviceCredentials {
    device_id: String,
    install_id: Option<String>,
    cdid: String,
    openudid: String,
    clientudid: String,
}

impl FrontierAuthMaterial {
    pub(crate) fn request_token_field(&self) -> Option<&str> {
        if self.token.matches('.').count() == 2 {
            Some(self.token.as_str())
        } else {
            None
        }
    }
}

pub(crate) fn resolve_frontier_auth(args: &Args) -> Result<FrontierAuthMaterial, String> {
    resolve_frontier_auth_impl(args, AuthResolveMode::Default, true)
}

pub(crate) fn refresh_frontier_auth(args: &Args) -> Result<FrontierAuthMaterial, String> {
    resolve_frontier_auth_impl(args, AuthResolveMode::ForceRefresh, true)
}

#[cfg(feature = "latency-bench")]
pub(crate) fn resolve_frontier_auth_for_profile(
    args: &Args,
    profile: FrontierProfile,
) -> Result<FrontierAuthMaterial, String> {
    if !profile.uses_android_payload() {
        return resolve_frontier_auth(args);
    }

    if let Some(auth) = explicit_auth(args) {
        persist_resolved_auth_artifacts(args, &auth);
        return Ok(auth);
    }

    if let Some(auth) = resolve_android_profile_cached_auth(args) {
        return Ok(auth);
    }

    if !args.disable_android_vdevice_auth {
        if let Some(auth) = resolve_android_virtual_device_auth(args, true) {
            persist_frontier_auth_cache(auth_cache_path(args).as_deref(), &auth);
            return Ok(auth);
        }
    }

    Err("no valid android frontier auth available".to_string())
}

pub(crate) fn preview_frontier_auth(args: &Args) -> Value {
    preview_frontier_auth_for_mode(args, AuthResolveMode::Default)
}

pub(crate) fn preview_frontier_auth_refresh(args: &Args) -> Value {
    preview_frontier_auth_for_mode(args, AuthResolveMode::ForceRefresh)
}

fn preview_frontier_auth_for_mode(args: &Args, mode: AuthResolveMode) -> Value {
    let cache_path = auth_cache_path(args).map(|path| path.display().to_string());
    let desktop_session_candidates = desktop_session_candidates(args, None)
        .into_iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    let desktop_session_env_path = preferred_desktop_session_env_path(args)
        .map(|path| path.display().to_string());
    let mac_live_auth_enabled = mac_live_auth_enabled(args);
    let mac_live_token_script_path = mac_live_token_script_path(args)
        .map(|path| path.display().to_string());
    match resolve_frontier_auth_impl(args, mode, false) {
        Ok(auth) => json!({
            "resolved": true,
            "mode": match mode {
                AuthResolveMode::Default => "default",
                AuthResolveMode::ForceRefresh => "force_refresh",
            },
            "provider_order": match mode {
                AuthResolveMode::Default => ["explicit", "cache", "desktop_session", "mac_live", "android_virtual_device", "bootstrap"],
                AuthResolveMode::ForceRefresh => ["explicit", "desktop_session", "mac_live", "android_virtual_device", "bootstrap", "cache"],
            },
            "source": auth.source,
            "app_key": auth.app_key,
            "expires_at_ms": if auth.exp > 0 { Some(auth.exp.saturating_mul(1000)) } else { None::<u64> },
            "ttl_ms": if auth.exp > 0 {
                Some(auth.exp.saturating_mul(1000).saturating_sub(now_millis()))
            } else {
                None::<u64>
            },
            "ws_url": auth.ws_url,
            "cache_path": cache_path,
            "desktop_session_env_path": desktop_session_env_path,
            "desktop_session_candidates": desktop_session_candidates,
            "mac_live_auth_enabled": mac_live_auth_enabled,
            "mac_live_token_script_path": mac_live_token_script_path,
        }),
        Err(error) => json!({
            "resolved": false,
            "mode": match mode {
                AuthResolveMode::Default => "default",
                AuthResolveMode::ForceRefresh => "force_refresh",
            },
            "provider_order": match mode {
                AuthResolveMode::Default => ["explicit", "cache", "desktop_session", "mac_live", "android_virtual_device", "bootstrap"],
                AuthResolveMode::ForceRefresh => ["explicit", "desktop_session", "mac_live", "android_virtual_device", "bootstrap", "cache"],
            },
            "error": error,
            "cache_path": cache_path,
            "desktop_session_env_path": desktop_session_env_path,
            "desktop_session_candidates": desktop_session_candidates,
            "mac_live_auth_enabled": mac_live_auth_enabled,
            "mac_live_token_script_path": mac_live_token_script_path,
            "bootstrap_candidates": bootstrap_candidates(args)
                .into_iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>(),
        }),
    }
}

fn resolve_frontier_auth_impl(
    args: &Args,
    mode: AuthResolveMode,
    persist_cache: bool,
) -> Result<FrontierAuthMaterial, String> {
    if let Some(auth) = explicit_auth(args) {
        if persist_cache {
            persist_resolved_auth_artifacts(args, &auth);
        }
        return Ok(auth);
    }

    if mode == AuthResolveMode::Default {
        if let Some(path) = auth_cache_path(args).as_deref() {
            if let Some(auth) = load_frontier_auth_cache(path) {
                return Ok(auth);
            }
        }
    }

    if let Some(auth) = resolve_desktop_session_auth(args) {
        if persist_cache {
            persist_resolved_auth_artifacts(args, &auth);
        }
        return Ok(auth);
    }

    if mac_live_auth_enabled(args) {
        if let Some(auth) = resolve_mac_live_auth(args, persist_cache) {
            if persist_cache {
                persist_frontier_auth_cache(auth_cache_path(args).as_deref(), &auth);
            }
            return Ok(auth);
        }
    }

    if !args.disable_android_vdevice_auth {
        if let Some(auth) = resolve_android_virtual_device_auth(args, persist_cache) {
            if persist_cache {
                persist_frontier_auth_cache(auth_cache_path(args).as_deref(), &auth);
            }
            return Ok(auth);
        }
    }

    for candidate in bootstrap_candidates(args) {
        if let Some(auth) = load_bootstrap_env(&candidate, args.frontier_app_key.clone()) {
            if persist_cache {
                persist_resolved_auth_artifacts(args, &auth);
            }
            return Ok(auth);
        }
    }

    if mode == AuthResolveMode::ForceRefresh {
        if let Some(path) = auth_cache_path(args).as_deref() {
            if let Some(auth) = load_frontier_auth_cache(path) {
                return Ok(auth);
            }
        }
    }

    Err("no valid frontier token available; pass --frontier-token or --bootstrap-env".to_string())
}

pub(crate) fn usable_token(token: &str) -> bool {
    let token = token.trim();
    !token.is_empty()
        && token != "<uninitialized>"
        && token != "null"
        && token != "None"
        && (token.matches('.').count() == 2 || token.len() >= 8)
}

pub(crate) fn decode_jwt_exp(token: &str) -> u64 {
    let Some(payload) = token.split('.').nth(1) else {
        return 0;
    };
    let Ok(decoded) = URL_SAFE_NO_PAD.decode(payload.as_bytes()) else {
        return 0;
    };
    serde_json::from_slice::<Value>(&decoded)
        .ok()
        .and_then(|value| value.get("exp").and_then(Value::as_u64))
        .unwrap_or(0)
}

fn explicit_auth(args: &Args) -> Option<FrontierAuthMaterial> {
    let source = if args.frontier_token.is_some() {
        "cli".to_string()
    } else if std::env::var("SAMI_TOKEN").ok().is_some() {
        "env".to_string()
    } else {
        return None;
    };
    let token = args
        .frontier_token
        .clone()
        .or_else(|| std::env::var("SAMI_TOKEN").ok())?;
    material_from_token(
        token,
        args.frontier_app_key
            .clone()
            .or_else(|| std::env::var("APP_KEY").ok())
            .unwrap_or_else(|| DEFAULT_APP_KEY.to_string()),
        source,
    )
}

fn material_from_token(token: String, app_key: String, source: String) -> Option<FrontierAuthMaterial> {
    material_from_token_with_ws(token, app_key, source, None)
}

fn material_from_token_with_ws(
    token: String,
    app_key: String,
    source: String,
    ws_url: Option<String>,
) -> Option<FrontierAuthMaterial> {
    if !usable_token(&token) {
        return None;
    }
    let exp = decode_jwt_exp(&token);
    if token_expiring_soon(exp) {
        return None;
    }
    Some(FrontierAuthMaterial {
        token,
        app_key,
        exp,
        source,
        ws_url,
    })
}

fn token_expiring_soon(exp: u64) -> bool {
    exp > 0 && exp <= now_millis() / 1000 + MIN_USABLE_TTL_SECS
}

fn mac_live_auth_enabled(args: &Args) -> bool {
    args.enable_mac_live_auth || truthy_env("HJ_ENABLE_MAC_LIVE_AUTH")
}

fn truthy_env(name: &str) -> bool {
    matches!(
        std::env::var(name).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES") | Some("on") | Some("ON")
    )
}

fn auth_cache_path(args: &Args) -> Option<PathBuf> {
    args.auth_cache_path
        .clone()
        .or_else(|| std::env::var("HJ_FRONTIER_AUTH_CACHE").ok())
        .map(|raw| resolve_path(&raw))
        .or_else(default_auth_cache_path)
}

fn default_auth_cache_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("shuo-engine")
            .join("frontier_auth.json"),
    )
}

fn default_desktop_session_env_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("shuo-engine")
            .join("desktop_session.env"),
    )
}

fn preferred_desktop_session_env_path(args: &Args) -> Option<PathBuf> {
    args.desktop_session_env
        .clone()
        .or_else(|| std::env::var("HJ_DESKTOP_SESSION_ENV").ok())
        .map(|raw| resolve_path(&raw))
        .or_else(default_desktop_session_env_path)
}

fn mac_live_token_script_path(args: &Args) -> Option<PathBuf> {
    let explicit = args
        .mac_live_token_script
        .clone()
        .or_else(|| std::env::var("HJ_MAC_LIVE_TOKEN_SCRIPT").ok())
        .map(|raw| resolve_path(&raw));
    if let Some(path) = explicit {
        return path.is_file().then_some(path);
    }

    let cwd_candidate = std::env::current_dir()
        .ok()
        .map(|cwd| cwd.join("scripts").join("lldb").join("capture_active_sami_token.sh"));
    if let Some(path) = cwd_candidate.filter(|path| path.is_file()) {
        return Some(path);
    }

    let manifest_candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("scripts")
        .join("lldb")
        .join("capture_active_sami_token.sh");
    manifest_candidate.is_file().then_some(manifest_candidate)
}

fn load_frontier_auth_cache(path: &Path) -> Option<FrontierAuthMaterial> {
    let text = fs::read_to_string(path).ok()?;
    let cached = serde_json::from_str::<FrontierAuthCacheRecord>(&text).ok()?;
    material_from_token_with_ws(
        cached.token,
        cached.app_key,
        format!("cache:{}", path.display()),
        cached.ws_url,
    )
}

fn persist_frontier_auth_cache(path: Option<&Path>, auth: &FrontierAuthMaterial) {
    let Some(path) = path else {
        return;
    };
    if auth.source.starts_with("cache:") {
        return;
    }
    let Some(parent) = path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let record = FrontierAuthCacheRecord {
        version: 1,
        token: auth.token.clone(),
        app_key: auth.app_key.clone(),
        exp: auth.exp,
        source: auth.source.clone(),
        cached_at_ms: now_millis(),
        ws_url: auth.ws_url.clone(),
    };
    let Ok(data) = serde_json::to_vec_pretty(&record) else {
        return;
    };
    let tmp_path = path.with_extension("tmp");
    if fs::write(&tmp_path, data).is_err() {
        return;
    }
    let _ = fs::rename(tmp_path, path);
}

fn persist_resolved_auth_artifacts(args: &Args, auth: &FrontierAuthMaterial) {
    persist_frontier_auth_cache(auth_cache_path(args).as_deref(), auth);
    if !auth.source.starts_with("cache:") {
        persist_desktop_session_env(
            preferred_desktop_session_env_path(args).as_deref(),
            auth,
            None,
        );
    }
}

fn resolve_desktop_session_auth(args: &Args) -> Option<FrontierAuthMaterial> {
    for path in desktop_session_candidates(args, None) {
        if let Some(auth) = load_desktop_session_env(&path, args.frontier_app_key.clone()) {
            return Some(auth);
        }
    }
    None
}

#[cfg(feature = "latency-bench")]
fn resolve_android_profile_cached_auth(args: &Args) -> Option<FrontierAuthMaterial> {
    for path in desktop_session_candidates(args, None) {
        if let Some(auth) = load_desktop_session_env(&path, args.frontier_app_key.clone()) {
            if auth_looks_android(&auth) {
                return Some(auth);
            }
        }
    }
    if let Some(path) = auth_cache_path(args).as_deref() {
        if let Some(auth) = load_frontier_auth_cache(path) {
            if auth_looks_android(&auth) {
                return Some(auth);
            }
        }
    }
    None
}

#[cfg(feature = "latency-bench")]
fn auth_looks_android(auth: &FrontierAuthMaterial) -> bool {
    auth.source.starts_with("android_virtual_device:")
        || auth
            .ws_url
            .as_deref()
            .map(|url| url.contains("aid=401734"))
            .unwrap_or(false)
}

fn desktop_session_candidates(args: &Args, root_override: Option<&Path>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();
    let mut explicit_candidate = false;

    if let Some(path) = args
        .desktop_session_env
        .clone()
        .or_else(|| std::env::var("HJ_DESKTOP_SESSION_ENV").ok())
        .map(|raw| resolve_path(&raw))
    {
        explicit_candidate = true;
        push_unique_candidate(&mut candidates, &mut seen, path);
    }

    if explicit_candidate {
        return candidates;
    }

    if let Some(path) = default_desktop_session_env_path() {
        push_unique_candidate(&mut candidates, &mut seen, path);
    }

    let root = root_override
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok());
    if let Some(root) = root {
        let captures_dir = root.join("captures");
        if captures_dir.is_dir() {
            let mut artifact_candidates = Vec::new();
            if let Ok(entries) = fs::read_dir(&captures_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }
                    let direct = path.join("resolved-sami-token.env");
                    if direct.is_file() {
                        artifact_candidates.push(direct);
                    }
                    let nested = path.join("token").join("resolved-sami-token.env");
                    if nested.is_file() {
                        artifact_candidates.push(nested);
                    }
                }
            }
            artifact_candidates.sort();
            artifact_candidates.reverse();
            for path in artifact_candidates {
                push_unique_candidate(&mut candidates, &mut seen, path);
            }
        }
    }

    candidates
}

fn push_unique_candidate(candidates: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, path: PathBuf) {
    if seen.insert(path.clone()) {
        candidates.push(path);
    }
}

fn load_desktop_session_env(path: &Path, app_key_override: Option<String>) -> Option<FrontierAuthMaterial> {
    let values = parse_env_file(path).ok()?;
    let token = values
        .get("DESKTOP_SESSION_TOKEN")
        .or_else(|| values.get("CURRENT_TOKEN"))
        .or_else(|| values.get("SAMI_TOKEN"))
        .or_else(|| values.get("TOKEN"))
        .cloned()?;
    let app_key = app_key_override
        .or_else(|| values.get("APP_KEY").cloned())
        .or_else(|| values.get("DESKTOP_SESSION_APP_KEY").cloned())
        .or_else(|| {
            if token.matches('.').count() == 2 {
                None
            } else {
                Some(token.clone())
            }
        })
        .unwrap_or_else(|| DEFAULT_APP_KEY.to_string());
    let exp = values
        .get("TOKEN_EXP")
        .or_else(|| values.get("DESKTOP_SESSION_TOKEN_EXP"))
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or_else(|| decode_jwt_exp(&token));
    if token_expiring_soon(exp) || !usable_token(&token) {
        return None;
    }
    Some(FrontierAuthMaterial {
        token,
        app_key,
        exp,
        source: format!("desktop_session:{}", path.display()),
        ws_url: values
            .get("FRONTIER_WS_URL")
            .or_else(|| values.get("ANDROID_VDEVICE_FRONTIER_WS_URL"))
            .cloned(),
    })
}

fn load_bootstrap_env(path: &Path, app_key_override: Option<String>) -> Option<FrontierAuthMaterial> {
    let values = parse_env_file(path).ok()?;
    let token = values
        .get("CURRENT_TOKEN")
        .or_else(|| values.get("SAMI_TOKEN"))
        .or_else(|| values.get("TOKEN"))
        .cloned()?;
    let app_key = app_key_override
        .or_else(|| values.get("APP_KEY").cloned())
        .unwrap_or_else(|| DEFAULT_APP_KEY.to_string());
    let source = path.display().to_string();
    let exp = values
        .get("TOKEN_EXP")
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or_else(|| decode_jwt_exp(&token));
    if token_expiring_soon(exp) || !usable_token(&token) {
        return None;
    }
    Some(FrontierAuthMaterial {
        token,
        app_key,
        exp,
        source,
        ws_url: None,
    })
}

fn resolve_android_virtual_device_auth(
    args: &Args,
    persist_artifacts: bool,
) -> Option<FrontierAuthMaterial> {
    let session_env_path = preferred_desktop_session_env_path(args)?;
    let existing_values = parse_env_file(&session_env_path).ok();
    let creds = existing_values
        .as_ref()
        .and_then(load_android_virtual_device_credentials)
        .or_else(register_android_virtual_device)?;
    let token = fetch_android_virtual_device_token(&creds.device_id, &creds.cdid)?;
    let auth = material_from_token_with_ws(
        token.clone(),
        args.frontier_app_key.clone().unwrap_or_else(|| token.clone()),
        format!("android_virtual_device:{}", creds.device_id),
        Some(android_virtual_device_ws_url(&creds.device_id)),
    )?;
    if persist_artifacts {
        persist_desktop_session_env(Some(session_env_path.as_path()), &auth, Some(&creds));
    }
    Some(auth)
}

fn resolve_mac_live_auth(args: &Args, persist_artifacts: bool) -> Option<FrontierAuthMaterial> {
    let script_path = mac_live_token_script_path(args)?;
    let capture_root = std::env::temp_dir().join(format!(
        "hj-mac-live-auth-{}-{}",
        std::process::id(),
        now_millis()
    ));
    let passive_env = run_mac_live_capture(&script_path, &capture_root.join("passive"), false);
    let refreshed_env = passive_env
        .as_ref()
        .filter(|path| mac_live_capture_usable(path))
        .cloned()
        .or_else(|| run_mac_live_capture(&script_path, &capture_root.join("refresh"), true))?;
    let auth = load_mac_live_capture_env(&refreshed_env, args.frontier_app_key.clone())?;
    if persist_artifacts {
        persist_desktop_session_env(
            preferred_desktop_session_env_path(args).as_deref(),
            &auth,
            None,
        );
    }
    Some(auth)
}

fn run_mac_live_capture(script_path: &Path, output_dir: &Path, refresh: bool) -> Option<PathBuf> {
    fs::create_dir_all(output_dir).ok()?;
    let mut command = Command::new("/bin/bash");
    command.arg(script_path);
    if refresh {
        command.arg("--refresh");
    }
    command.arg(output_dir);
    let status = command.status().ok()?;
    if !status.success() {
        return None;
    }
    let env_path = output_dir.join("resolved-sami-token.env");
    env_path.is_file().then_some(env_path)
}

fn mac_live_capture_usable(path: &Path) -> bool {
    let values = parse_env_file(path).ok();
    values
        .as_ref()
        .and_then(|values| {
            values
                .get("CURRENT_TOKEN")
                .or_else(|| values.get("TOKEN"))
                .or_else(|| values.get("SAMI_TOKEN"))
        })
        .map(|token| usable_token(token))
        .unwrap_or(false)
}

fn load_mac_live_capture_env(
    path: &Path,
    app_key_override: Option<String>,
) -> Option<FrontierAuthMaterial> {
    let values = parse_env_file(path).ok()?;
    let token = values
        .get("CURRENT_TOKEN")
        .or_else(|| values.get("TOKEN"))
        .or_else(|| values.get("SAMI_TOKEN"))
        .cloned()?;
    let app_key = app_key_override
        .or_else(|| values.get("APP_KEY").cloned())
        .unwrap_or_else(|| DEFAULT_APP_KEY.to_string());
    let fetch_url = values
        .get("FETCH_TOKEN_URL")
        .cloned()
        .unwrap_or_else(|| "active_sami_manager".to_string());
    material_from_token_with_ws(
        token,
        app_key,
        format!("mac_live:{fetch_url}"),
        Some(DEFAULT_FRONTIER_WS_URL.to_string()),
    )
}

fn load_android_virtual_device_credentials(
    values: &HashMap<String, String>,
) -> Option<AndroidVirtualDeviceCredentials> {
    let device_id = values
        .get("ANDROID_VDEVICE_DEVICE_ID")
        .or_else(|| values.get("DEVICE_ID"))
        .cloned()?;
    let cdid = values
        .get("ANDROID_VDEVICE_CDID")
        .or_else(|| values.get("CDID"))
        .cloned()?;
    let openudid = values
        .get("ANDROID_VDEVICE_OPENUDID")
        .or_else(|| values.get("OPENUDID"))
        .cloned()
        .unwrap_or_else(random_openudid);
    let clientudid = values
        .get("ANDROID_VDEVICE_CLIENTUDID")
        .or_else(|| values.get("CLIENTUDID"))
        .cloned()
        .unwrap_or_else(random_uuid);
    Some(AndroidVirtualDeviceCredentials {
        device_id,
        install_id: values
            .get("ANDROID_VDEVICE_INSTALL_ID")
            .or_else(|| values.get("INSTALL_ID"))
            .cloned(),
        cdid,
        openudid,
        clientudid,
    })
}

fn register_android_virtual_device() -> Option<AndroidVirtualDeviceCredentials> {
    let cdid = random_uuid();
    let openudid = random_openudid();
    let clientudid = random_uuid();
    let client = http_client()?;
    let now = now_millis();
    let body = json!({
        "magic_tag": "ss_app_log",
        "header": {
            "device_id": 0,
            "install_id": 0,
            "aid": ANDROID_AID.parse::<u64>().ok()?,
            "app_name": ANDROID_APP_NAME,
            "version_code": ANDROID_VERSION_CODE.parse::<u64>().ok()?,
            "version_name": ANDROID_VERSION_NAME,
            "manifest_version_code": ANDROID_VERSION_CODE.parse::<u64>().ok()?,
            "update_version_code": ANDROID_VERSION_CODE.parse::<u64>().ok()?,
            "channel": ANDROID_CHANNEL,
            "package": ANDROID_PACKAGE,
            "device_platform": "android",
            "os": "android",
            "os_api": "34",
            "os_version": "16",
            "device_type": "Pixel 7 Pro",
            "device_brand": "google",
            "device_model": "Pixel 7 Pro",
            "resolution": "1080*2400",
            "dpi": "420",
            "language": "zh",
            "timezone": 8,
            "access": "wifi",
            "rom": "UP1A.231005.007",
            "rom_version": "UP1A.231005.007",
            "openudid": openudid,
            "clientudid": clientudid,
            "cdid": cdid,
            "region": "CN",
            "tz_name": "Asia/Shanghai",
            "tz_offset": 28800,
            "sim_region": "cn",
            "carrier_region": "cn",
            "cpu_abi": "arm64-v8a",
            "build_serial": "unknown",
            "not_request_sender": 0,
            "sig_hash": "",
            "google_aid": "",
            "mc": "",
            "serial_number": "",
        },
        "_gen_time": now,
    });
    let response = client
        .post(ANDROID_REGISTER_URL)
        .query(&[
            ("device_platform", "android"),
            ("os", "android"),
            ("ssmix", "a"),
            ("_rticket", &now.to_string()),
            ("cdid", &cdid),
            ("channel", ANDROID_CHANNEL),
            ("aid", ANDROID_AID),
            ("app_name", ANDROID_APP_NAME),
            ("version_code", ANDROID_VERSION_CODE),
            ("version_name", ANDROID_VERSION_NAME),
            ("manifest_version_code", ANDROID_VERSION_CODE),
            ("update_version_code", ANDROID_VERSION_CODE),
            ("resolution", "1080*2400"),
            ("dpi", "420"),
            ("device_type", "Pixel 7 Pro"),
            ("device_brand", "google"),
            ("language", "zh"),
            ("os_api", "34"),
            ("os_version", "16"),
            ("ac", "wifi"),
        ])
        .header("User-Agent", ANDROID_USER_AGENT)
        .json(&body)
        .send()
        .ok()?
        .error_for_status()
        .ok()?
        .json::<Value>()
        .ok()?;
    let device_id = response
        .get("device_id")
        .and_then(Value::as_u64)
        .map(|value| value.to_string())
        .or_else(|| {
            response
                .get("device_id_str")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })?;
    let install_id = response
        .get("install_id")
        .and_then(Value::as_u64)
        .map(|value| value.to_string())
        .or_else(|| {
            response
                .get("install_id_str")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        });
    Some(AndroidVirtualDeviceCredentials {
        device_id,
        install_id,
        cdid,
        openudid,
        clientudid,
    })
}

fn fetch_android_virtual_device_token(device_id: &str, cdid: &str) -> Option<String> {
    let client = http_client()?;
    let now = now_millis().to_string();
    let body = "body=null";
    let x_ss_stub = format!("{:X}", md5::compute(body.as_bytes()));
    let response = client
        .post(ANDROID_SETTINGS_URL)
        .query(&[
            ("device_platform", "android"),
            ("os", "android"),
            ("ssmix", "a"),
            ("_rticket", now.as_str()),
            ("cdid", cdid),
            ("channel", ANDROID_CHANNEL),
            ("aid", ANDROID_AID),
            ("app_name", ANDROID_APP_NAME),
            ("version_code", ANDROID_VERSION_CODE),
            ("version_name", ANDROID_VERSION_NAME),
            ("device_id", device_id),
        ])
        .header("User-Agent", ANDROID_USER_AGENT)
        .header("x-ss-stub", x_ss_stub)
        .body(body.to_string())
        .send()
        .ok()?
        .error_for_status()
        .ok()?
        .json::<Value>()
        .ok()?;
    response
        .pointer("/data/settings/asr_config/app_key")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn persist_desktop_session_env(
    path: Option<&Path>,
    auth: &FrontierAuthMaterial,
    android: Option<&AndroidVirtualDeviceCredentials>,
) {
    let Some(path) = path else {
        return;
    };
    let Some(parent) = path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let mut lines = vec![
        format!("DESKTOP_SESSION_PROVIDER={}", auth.source),
        format!("DESKTOP_SESSION_TOKEN={}", auth.token),
        format!("CURRENT_TOKEN={}", auth.token),
        format!("APP_KEY={}", auth.app_key),
    ];
    if auth.exp > 0 {
        lines.push(format!("DESKTOP_SESSION_TOKEN_EXP={}", auth.exp));
        lines.push(format!("TOKEN_EXP={}", auth.exp));
    }
    if let Some(ws_url) = &auth.ws_url {
        lines.push(format!("FRONTIER_WS_URL={ws_url}"));
        lines.push(format!("ANDROID_VDEVICE_FRONTIER_WS_URL={ws_url}"));
    }
    if let Some(android) = android {
        lines.push(format!("DEVICE_ID={}", android.device_id));
        lines.push(format!("ANDROID_VDEVICE_DEVICE_ID={}", android.device_id));
        if let Some(install_id) = &android.install_id {
            lines.push(format!("INSTALL_ID={install_id}"));
            lines.push(format!("ANDROID_VDEVICE_INSTALL_ID={install_id}"));
        }
        lines.push(format!("CDID={}", android.cdid));
        lines.push(format!("OPENUDID={}", android.openudid));
        lines.push(format!("CLIENTUDID={}", android.clientudid));
        lines.push(format!("ANDROID_VDEVICE_CDID={}", android.cdid));
        lines.push(format!("ANDROID_VDEVICE_OPENUDID={}", android.openudid));
        lines.push(format!("ANDROID_VDEVICE_CLIENTUDID={}", android.clientudid));
    }
    let tmp_path = path.with_extension("tmp");
    if fs::write(&tmp_path, lines.join("\n") + "\n").is_err() {
        return;
    }
    let _ = fs::rename(tmp_path, path);
}

fn android_virtual_device_ws_url(device_id: &str) -> String {
    format!(
        "{ANDROID_FRONTIER_WS_BASE_URL}?aid={ANDROID_AID}&device_id={device_id}"
    )
}

fn http_client() -> Option<Client> {
    Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .ok()
}

fn random_uuid() -> String {
    Uuid::new_v4().to_string()
}

fn random_openudid() -> String {
    Uuid::new_v4().simple().to_string()[..16].to_string()
}

fn parse_env_file(path: &Path) -> Result<HashMap<String, String>, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("read bootstrap env failed for {}: {error}", path.display()))?;
    let mut values = HashMap::new();
    for line in text.lines() {
        let content = line.trim();
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        let Some((key, value)) = content.split_once('=') else {
            continue;
        };
        values.insert(key.trim().to_string(), value.trim().to_string());
    }
    Ok(values)
}

fn bootstrap_candidates(args: &Args) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = args
        .bootstrap_env
        .clone()
        .or_else(|| std::env::var("BOOTSTRAP_ENV").ok())
    {
        let path = resolve_path(&path);
        if path.is_file() {
            candidates.push(path);
        }
    }
    let captures_dir = std::env::current_dir().ok().map(|cwd| cwd.join("captures"));
    if let Some(captures_dir) = captures_dir.filter(|path| path.is_dir()) {
        let mut entries = fs::read_dir(captures_dir)
            .ok()
            .into_iter()
            .flat_map(|entries| entries.flatten())
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .filter(|path| {
                path.file_name()
                    .and_then(|value| value.to_str())
                    .map(|name| name.ends_with("-bootstrap-doubao-token-once"))
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        entries.sort();
        entries.reverse();
        for directory in entries {
            let env_path = directory.join("bootstrap.env");
            if env_path.is_file() {
                candidates.push(env_path);
            }
        }
    }
    candidates
}

fn resolve_path(raw: &str) -> PathBuf {
    let path = PathBuf::from(raw).expanduser_like();
    if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

trait ExpandUserLike {
    fn expanduser_like(self) -> Self;
}

impl ExpandUserLike for PathBuf {
    fn expanduser_like(self) -> Self {
        let raw = self.to_string_lossy();
        if raw == "~" || raw.starts_with("~/") {
            if let Ok(home) = std::env::var("HOME") {
                return PathBuf::from(raw.replacen('~', &home, 1));
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_jwt_exp, desktop_session_candidates, load_desktop_session_env, load_frontier_auth_cache, persist_desktop_session_env, persist_frontier_auth_cache, refresh_frontier_auth, resolve_frontier_auth, usable_token, AuthResolveMode, FrontierAuthMaterial};
    use crate::frontier_protocol::DEFAULT_FRONTIER_WS_URL;
    use crate::{Args, FrontierProfile, RunMode, TransportKind};
    use std::fs;
    use std::path::PathBuf;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("shuo-engine-{}-{}-{}.json", name, std::process::id(), crate::state::now_millis()))
    }

    fn base_args() -> Args {
        Args {
            mode: RunMode::StdioEngine,
            transport: TransportKind::DirectFrontier,
            frontier_profile: FrontierProfile::CurrentPcm,
            server_url: "ws://127.0.0.1:8765".to_string(),
            frontier_token: None,
            frontier_app_key: None,
            bootstrap_env: None,
            auth_cache_path: None,
            desktop_session_env: Some(
                temp_path("isolated-desktop-session")
                    .with_extension("env")
                    .to_string_lossy()
                    .into_owned(),
            ),
            enable_mac_live_auth: false,
            mac_live_token_script: None,
            disable_android_vdevice_auth: true,
            partial_interval_ms: 0,
            verbose: false,
            type_partial: false,
            subtitle_overlay: false,
            ui_scale: 1.0,
            benchmark_input_wav: None,
            benchmark_chunk_ms: 20,
            benchmark_warmup: true,
            benchmark_timeout_secs: 10.0,
        }
    }

    #[test]
    fn validates_token_shape_and_jwt_exp() {
        assert!(!usable_token(""));
        assert!(usable_token("a.b.c"));
        assert_eq!(decode_jwt_exp("x.eyJleHAiOjEyM30.y"), 123);
    }

    #[test]
    fn persists_and_loads_cache() {
        let path = temp_path("auth-cache");
        let auth = FrontierAuthMaterial {
            token: "x.eyJleHAiOjQxMDI0NDQ4MDAwfQ.y".to_string(),
            app_key: "demo".to_string(),
            exp: 41_024_448_000,
            source: "bootstrap.env".to_string(),
            ws_url: None,
        };
        persist_frontier_auth_cache(Some(&path), &auth);
        let loaded = load_frontier_auth_cache(&path).expect("cache auth");
        assert_eq!(loaded.token, auth.token);
        assert_eq!(loaded.app_key, auth.app_key);
        assert!(loaded.source.starts_with("cache:"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persists_and_loads_desktop_session_env() {
        let path = temp_path("desktop-session-writer").with_extension("env");
        let auth = FrontierAuthMaterial {
            token: "short-app-key".to_string(),
            app_key: "short-app-key".to_string(),
            exp: 0,
            source: "android_virtual_device:123".to_string(),
            ws_url: Some("wss://frontier-audio-ime-ws.doubao.com/ocean/api/v1/ws?aid=401734&device_id=123".to_string()),
        };
        persist_desktop_session_env(Some(&path), &auth, None);
        let loaded = load_desktop_session_env(&path, None).expect("desktop session auth");
        assert_eq!(loaded.token, "short-app-key");
        assert_eq!(loaded.app_key, "short-app-key");
        assert_eq!(loaded.ws_url, auth.ws_url);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn resolves_from_cache_before_bootstrap() {
        let path = temp_path("auth-cache-priority");
        persist_frontier_auth_cache(
            Some(&path),
            &FrontierAuthMaterial {
                token: "x.eyJleHAiOjQxMDI0NDQ4MDAwfQ.y".to_string(),
                app_key: "cached-app".to_string(),
                exp: 41_024_448_000,
                source: "bootstrap.env".to_string(),
                ws_url: None,
            },
        );
        let mut args = base_args();
        args.auth_cache_path = Some(path.to_string_lossy().into_owned());
        let resolved = resolve_frontier_auth(&args).expect("resolved auth");
        assert_eq!(resolved.app_key, "cached-app");
        assert!(resolved.source.starts_with("cache:"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn force_refresh_prefers_bootstrap_before_cache() {
        let cache_path = temp_path("auth-cache-force");
        let bootstrap_path = temp_path("auth-bootstrap-force").with_extension("env");
        persist_frontier_auth_cache(
            Some(&cache_path),
            &FrontierAuthMaterial {
                token: "x.eyJleHAiOjQxMDI0NDQ4MDAwfQ.y".to_string(),
                app_key: "cached-app".to_string(),
                exp: 41_024_448_000,
                source: "bootstrap.env".to_string(),
                ws_url: None,
            },
        );
        fs::write(
            &bootstrap_path,
            "CURRENT_TOKEN=x.eyJleHAiOjQxMDI0NDQ4MDAxfQ.y\nAPP_KEY=bootstrap-app\nTOKEN_EXP=41024448001\n",
        )
        .expect("write bootstrap env");

        let mut args = base_args();
        args.auth_cache_path = Some(cache_path.to_string_lossy().into_owned());
        args.bootstrap_env = Some(bootstrap_path.to_string_lossy().into_owned());
        let resolved = refresh_frontier_auth(&args).expect("resolved auth");
        assert_eq!(resolved.app_key, "bootstrap-app");
        assert_eq!(resolved.source, bootstrap_path.display().to_string());

        let _ = fs::remove_file(cache_path);
        let _ = fs::remove_file(bootstrap_path);
        let _ = AuthResolveMode::Default;
    }

    #[test]
    fn resolves_from_desktop_session_before_bootstrap() {
        let desktop_path = temp_path("desktop-session").with_extension("env");
        let bootstrap_path = temp_path("bootstrap-session").with_extension("env");
        let isolated_cache_path = temp_path("desktop-session-cache");
        fs::write(
            &desktop_path,
            "DESKTOP_SESSION_TOKEN=x.eyJleHAiOjQxMDI0NDQ4MDAyfQ.y\nAPP_KEY=desktop-app\nDESKTOP_SESSION_TOKEN_EXP=41024448002\n",
        )
        .expect("write desktop session env");
        fs::write(
            &bootstrap_path,
            "CURRENT_TOKEN=x.eyJleHAiOjQxMDI0NDQ4MDAzfQ.y\nAPP_KEY=bootstrap-app\nTOKEN_EXP=41024448003\n",
        )
        .expect("write bootstrap env");

        let mut args = base_args();
        args.auth_cache_path = Some(isolated_cache_path.to_string_lossy().into_owned());
        args.desktop_session_env = Some(desktop_path.to_string_lossy().into_owned());
        args.bootstrap_env = Some(bootstrap_path.to_string_lossy().into_owned());
        let resolved = resolve_frontier_auth(&args).expect("resolved auth");
        assert_eq!(resolved.app_key, "desktop-app");
        assert!(resolved.source.starts_with("desktop_session:"));

        let _ = fs::remove_file(desktop_path);
        let _ = fs::remove_file(bootstrap_path);
        let _ = fs::remove_file(isolated_cache_path);
    }

    #[test]
    fn discovers_latest_capture_desktop_session_candidate() {
        let root = temp_path("desktop-root");
        let captures = root.join("captures");
        let older = captures.join("20260404-aaaa");
        let newer = captures.join("20260405-bbbb");
        fs::create_dir_all(older.join("token")).expect("create older");
        fs::create_dir_all(newer.join("token")).expect("create newer");
        let older_env = older.join("token").join("resolved-sami-token.env");
        let newer_env = newer.join("resolved-sami-token.env");
        fs::write(&older_env, "CURRENT_TOKEN=x.eyJleHAiOjQxMDI0NDQ4MDAyfQ.y\nTOKEN_EXP=41024448002\n")
            .expect("write older env");
        fs::write(&newer_env, "CURRENT_TOKEN=x.eyJleHAiOjQxMDI0NDQ4MDAzfQ.y\nTOKEN_EXP=41024448003\n")
            .expect("write newer env");

        let mut args = base_args();
        args.desktop_session_env = None;
        let candidates = desktop_session_candidates(&args, Some(root.as_path()));
        assert!(candidates.iter().any(|path| path == &newer_env));
        assert!(candidates.iter().any(|path| path == &older_env));
        let newer_index = candidates.iter().position(|path| path == &newer_env).expect("newer idx");
        let older_index = candidates.iter().position(|path| path == &older_env).expect("older idx");
        assert!(newer_index < older_index);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn resolves_from_bootstrap_and_materializes_canonical_desktop_session() {
        let desktop_path = temp_path("materialized-desktop").with_extension("env");
        let bootstrap_path = temp_path("materialized-bootstrap").with_extension("env");
        let isolated_cache_path = temp_path("materialized-cache");
        fs::write(
            &bootstrap_path,
            "CURRENT_TOKEN=x.eyJleHAiOjQxMDI0NDQ4MDAzfQ.y\nAPP_KEY=bootstrap-app\nTOKEN_EXP=41024448003\n",
        )
        .expect("write bootstrap env");

        let mut args = base_args();
        args.auth_cache_path = Some(isolated_cache_path.to_string_lossy().into_owned());
        args.desktop_session_env = Some(desktop_path.to_string_lossy().into_owned());
        args.bootstrap_env = Some(bootstrap_path.to_string_lossy().into_owned());
        let resolved = resolve_frontier_auth(&args).expect("resolved auth");
        assert_eq!(resolved.app_key, "bootstrap-app");
        let materialized =
            load_desktop_session_env(&desktop_path, None).expect("materialized desktop auth");
        assert_eq!(materialized.token, resolved.token);
        assert_eq!(materialized.app_key, resolved.app_key);

        let _ = fs::remove_file(desktop_path);
        let _ = fs::remove_file(bootstrap_path);
        let _ = fs::remove_file(isolated_cache_path);
    }

    #[test]
    fn resolves_from_mac_live_capture_and_materializes_desktop_session() {
        let script_path = temp_path("mac-live-script").with_extension("sh");
        let desktop_path = temp_path("mac-live-desktop").with_extension("env");
        let isolated_cache_path = temp_path("mac-live-cache");
        fs::write(
            &script_path,
            r#"#!/usr/bin/env bash
set -euo pipefail
REFRESH=0
OUTPUT_DIR=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --refresh)
      REFRESH=1
      shift
      ;;
    *)
      OUTPUT_DIR="$1"
      shift
      ;;
  esac
done
mkdir -p "$OUTPUT_DIR"
if [ "$REFRESH" = "1" ]; then
  cat >"$OUTPUT_DIR/resolved-sami-token.env" <<'EOF'
CURRENT_TOKEN=x.eyJleHAiOjQxMDI0NDQ4MDA0fQ.y
APP_KEY=mac-live-app
FETCH_TOKEN_URL=https://ime.oceancloudapi.com/api/v1/user/get_config
EOF
else
  cat >"$OUTPUT_DIR/resolved-sami-token.env" <<'EOF'
CURRENT_TOKEN=<uninitialized>
APP_KEY=mac-live-app
EOF
fi
printf '{}\n' >"$OUTPUT_DIR/resolved-sami-token.json"
"#,
        )
        .expect("write mac live script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&script_path).expect("metadata").permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script_path, perms).expect("chmod");
        }

        let mut args = base_args();
        args.enable_mac_live_auth = true;
        args.mac_live_token_script = Some(script_path.to_string_lossy().into_owned());
        args.desktop_session_env = Some(desktop_path.to_string_lossy().into_owned());
        args.auth_cache_path = Some(isolated_cache_path.to_string_lossy().into_owned());
        let resolved = resolve_frontier_auth(&args).expect("resolved mac live auth");
        assert_eq!(resolved.app_key, "mac-live-app");
        assert_eq!(resolved.ws_url.as_deref(), Some(DEFAULT_FRONTIER_WS_URL));
        assert!(resolved.source.starts_with("mac_live:https://ime.oceancloudapi.com/api/v1/user/get_config"));

        let materialized =
            load_desktop_session_env(&desktop_path, None).expect("materialized desktop auth");
        assert_eq!(materialized.token, resolved.token);
        assert_eq!(materialized.app_key, resolved.app_key);

        let _ = fs::remove_file(script_path);
        let _ = fs::remove_file(desktop_path);
        let _ = fs::remove_file(isolated_cache_path);
    }
}
