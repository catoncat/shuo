use clap::Parser;

mod audio;
#[cfg(feature = "latency-bench")]
mod benchmark;
mod config;
mod engine_ipc;
mod engine_state;
mod frontier_auth;
mod frontier_transport;
mod frontier_protocol;
mod legacy_transport;
mod state;
mod stdio_engine;

#[cfg(not(feature = "latency-bench"))]
mod benchmark {
    use crate::Args;

    pub(crate) fn run_benchmark_replay(_args: Args) {
        eprintln!("benchmark mode requires cargo feature: latency-bench");
        std::process::exit(2);
    }
}

use clap::ValueEnum;
use stdio_engine::run_stdio_engine;
const HELPER_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("SHUO_ENGINE_GIT_REV"),
    ", build ",
    env!("SHUO_ENGINE_BUILD_STAMP"),
    ")"
);
#[derive(Parser, Debug, Clone)]
#[command(version = HELPER_VERSION)]
struct Args {
    #[arg(long, value_enum, default_value_t = RunMode::StdioEngine)]
    mode: RunMode,

    #[arg(long, value_enum, default_value_t = TransportKind::DirectFrontier)]
    transport: TransportKind,

    #[arg(long, value_enum, default_value_t = FrontierProfile::CurrentOpus)]
    frontier_profile: FrontierProfile,

    #[arg(long, default_value = "ws://127.0.0.1:8765")]
    server_url: String,

    #[arg(long)]
    frontier_token: Option<String>,

    #[arg(long)]
    frontier_app_key: Option<String>,

    #[arg(long)]
    bootstrap_env: Option<String>,

    #[arg(long)]
    auth_cache_path: Option<String>,

    #[arg(long)]
    desktop_session_env: Option<String>,

    #[arg(
        long,
        default_value_t = false,
        help = "Opt in to live Mac SAMITokenManager capture before Android virtual-device auth"
    )]
    enable_mac_live_auth: bool,

    #[arg(long, help = "Path to capture_active_sami_token.sh for Mac live auth")]
    mac_live_token_script: Option<String>,

    #[arg(long, default_value_t = false)]
    disable_android_vdevice_auth: bool,

    #[arg(long, default_value_t = 0)]
    partial_interval_ms: u64,

    #[arg(long, default_value_t = false, help = "Print verbose helper logs")]
    verbose: bool,

    #[arg(long, default_value_t = false, hide = true)]
    type_partial: bool,

    #[arg(long, default_value_t = false, hide = true)]
    subtitle_overlay: bool,

    #[arg(long, default_value_t = 1.0, hide = true)]
    ui_scale: f64,

    #[arg(long)]
    benchmark_input_wav: Option<String>,

    #[arg(long, default_value_t = 20)]
    benchmark_chunk_ms: u64,

    #[arg(long, default_value_t = true)]
    benchmark_warmup: bool,

    #[arg(long, default_value_t = 10.0)]
    benchmark_timeout_secs: f64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum RunMode {
    StdioEngine,
    BenchmarkReplay,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum TransportKind {
    LegacyLocalWs,
    DirectFrontier,
}

impl TransportKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::LegacyLocalWs => "legacy_local_ws",
            Self::DirectFrontier => "direct_frontier",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum FrontierProfile {
    CurrentPcm,
    CurrentOpus,
    AndroidPcm,
    AndroidOpus,
}

impl FrontierProfile {
    #[cfg(feature = "latency-bench")]
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::CurrentPcm => "current_pcm",
            Self::CurrentOpus => "current_opus",
            Self::AndroidPcm => "android_pcm",
            Self::AndroidOpus => "android_opus",
        }
    }

    pub(crate) fn uses_opus(self) -> bool {
        matches!(self, Self::CurrentOpus | Self::AndroidOpus)
    }

    #[cfg(feature = "latency-bench")]
    pub(crate) fn uses_android_payload(self) -> bool {
        matches!(self, Self::AndroidPcm | Self::AndroidOpus)
    }
}

fn main() {
    let args = Args::parse();
    match args.mode {
        RunMode::StdioEngine => run_stdio_engine(args),
        RunMode::BenchmarkReplay => benchmark::run_benchmark_replay(args),
    }
}
