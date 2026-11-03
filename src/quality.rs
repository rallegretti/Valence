//! Graphics quality (Full / Lite) and staged startup.
//!
//! **Quality.** Full is the complete look (bloom glow, film tone mapping,
//! translucent ion halos and lone pairs). Lite draws everything with one
//! opaque lit material and no post-processing, so far fewer shader variants
//! have to be compiled, which matters on old integrated GPUs where shader
//! compilation can take tens of seconds. Chemistry and UI are identical.
//! The mode is chosen once at startup: `--lite`/`--full` flag, else the saved
//! setting, else Auto (detected from the GPU).
//!
//! **Staged startup.** The interface's shaders are tiny, so we let them
//! compile first (the UI appears in a second or two), and only then create
//! the 3D scene. Pipeline progress comes from the render world each frame.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};

use bevy::{
    prelude::*,
    render::{
        Render, RenderApp,
        render_resource::{CachedPipeline, CachedPipelineState, PipelineCache, PipelineDescriptor},
        settings::Backends,
    },
};

use crate::settings_file;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    Full,
    Lite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QualitySetting {
    Auto,
    Full,
    Lite,
}

impl QualitySetting {
    pub fn as_str(self) -> &'static str {
        match self {
            QualitySetting::Auto => "auto",
            QualitySetting::Full => "full",
            QualitySetting::Lite => "lite",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "full" => Some(Self::Full),
            "lite" => Some(Self::Lite),
            _ => None,
        }
    }
}

/// The active quality for this run, plus the saved preference (which may
/// differ until the next launch).
#[derive(Resource)]
pub struct GraphicsQuality {
    pub active: Quality,
    /// Why `active` was chosen, for the UI ("Auto: Intel HD Graphics 530").
    pub reason: String,
    /// Saved preference; changes apply on the next launch.
    pub saved: QualitySetting,
}

impl GraphicsQuality {
    pub fn save(&mut self, setting: QualitySetting) {
        self.saved = setting;
        settings_file::set("graphics", setting.as_str());
    }
}

/// Decide the quality before the app starts.
pub fn resolve(backends: Backends) -> GraphicsQuality {
    let saved = settings_file::get("graphics")
        .and_then(|s| QualitySetting::parse(&s))
        .unwrap_or(QualitySetting::Auto);
    let flag = std::env::args().find_map(|a| match a.as_str() {
        "--lite" => Some(Quality::Lite),
        "--full" => Some(Quality::Full),
        _ => None,
    });
    let (active, reason) = match (flag, saved) {
        (Some(q), _) => (q, "set by command-line flag".to_string()),
        (None, QualitySetting::Full) => (Quality::Full, "your setting".to_string()),
        (None, QualitySetting::Lite) => (Quality::Lite, "your setting".to_string()),
        (None, QualitySetting::Auto) => match crate::gpu::probe_adapter(backends) {
            Some(gpu) => {
                let q = if is_old_gpu(gpu.vendor, gpu.device, gpu.device_type) {
                    Quality::Lite
                } else {
                    Quality::Full
                };
                (q, format!("Auto: {}", gpu.name))
            }
            None => (Quality::Full, "Auto: GPU not identified".to_string()),
        },
    };
    GraphicsQuality {
        active,
        reason,
        saved,
    }
}

/// GPUs that are slow to compile Bevy's shaders: software renderers and
/// Intel integrated graphics up to Gen 9.5 (Skylake … Comet Lake, ~2015-2020).
pub fn is_old_gpu(vendor: u32, device: u32, device_type: wgpu::DeviceType) -> bool {
    const INTEL: u32 = 0x8086;
    // High byte of PCI device ids used by Intel Gen 7–9.5 integrated GPUs.
    const OLD_INTEL_FAMILIES: [u32; 16] = [
        0x01, 0x04, 0x0A, 0x0C, 0x0D, 0x0F, 0x16, 0x19, 0x1A, 0x22, 0x31, 0x3E, 0x59, 0x5A, 0x87,
        0x9B,
    ];
    match device_type {
        wgpu::DeviceType::Cpu => true,
        wgpu::DeviceType::IntegratedGpu if vendor == INTEL => {
            OLD_INTEL_FAMILIES.contains(&((device >> 8) & 0xFF))
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Staged startup
// ---------------------------------------------------------------------------

/// Shared with the render world: how many pipelines are still compiling.
#[derive(Resource, Clone, Default)]
pub struct RenderStats(Arc<Stats>);

#[derive(Default)]
pub struct Stats {
    compiling: AtomicUsize,
    frames: AtomicU64,
    /// The interface's (egui) pipeline has been built.
    ui_ready: AtomicBool,
}

impl RenderStats {
    pub fn compiling(&self) -> usize {
        self.0.compiling.load(Ordering::Relaxed)
    }
    pub fn frames(&self) -> u64 {
        self.0.frames.load(Ordering::Relaxed)
    }
    pub fn ui_ready(&self) -> bool {
        self.0.ui_ready.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Phase {
    /// Only the UI exists; its shaders are compiling.
    #[default]
    Interface,
    /// The 3D scene exists and its shaders are compiling.
    Scene,
    /// Everything is drawn.
    Ready,
}

#[derive(Resource, Default)]
pub struct Startup {
    pub phase: Phase,
    /// Seconds after launch at which the UI / the 3D scene became visible.
    pub ui_ready_at: Option<f64>,
    pub scene_ready_at: Option<f64>,
    scene_started_at: f64,
    idle_frames: u32,
    last_render_frame: u64,
    /// The player dismissed the "use Lite next time" suggestion.
    pub slow_hint_dismissed: bool,
}

impl Startup {
    pub fn scene_allowed(&self) -> bool {
        self.phase != Phase::Interface
    }

    /// How long the 3D scene took to appear (if it has).
    pub fn scene_secs(&self) -> Option<f64> {
        self.scene_ready_at.map(|t| t - self.scene_started_at)
    }
}

/// Pipelines must be idle for this many rendered frames to count as done.
const IDLE_FRAMES: u32 = 3;
/// Never wait longer than this for the UI alone before building the scene.
const UI_WAIT_LIMIT_SECS: f64 = 20.0;

pub struct StartupPlugin;

impl Plugin for StartupPlugin {
    fn build(&self, app: &mut App) {
        let stats = RenderStats::default();
        app.insert_resource(stats.clone())
            .init_resource::<Startup>()
            .add_systems(Update, advance_startup);
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .insert_resource(stats)
                .add_systems(Render, count_compiling_pipelines);
        }
    }
}

fn is_compiling(p: &CachedPipeline) -> bool {
    matches!(
        p.state,
        CachedPipelineState::Queued | CachedPipelineState::Creating(_)
    )
}

/// Bevy's always-on background features (atmosphere, environment maps,
/// SSAO) that this game never draws with. They don't count towards "the scene
/// is ready"; everything else (meshes, GPU culling, post-processing) does.
const BACKGROUND_PIPELINES: [&str; 9] = [
    "transmittance_lut",
    "multi_scattering_lut",
    "sky_view_lut",
    "aerial_view_lut",
    "radiance_pipeline",
    "irradiance_pipeline",
    "environment_pipeline",
    "mip generation",
    "ssao_",
];

fn is_needed(p: &CachedPipeline) -> bool {
    let name = label(p).unwrap_or("");
    !BACKGROUND_PIPELINES.iter().any(|b| name.starts_with(b)) && name != "copy_pipeline"
}

fn label(p: &CachedPipeline) -> Option<&str> {
    match &p.descriptor {
        PipelineDescriptor::RenderPipelineDescriptor(d) => d.label.as_deref(),
        PipelineDescriptor::ComputePipelineDescriptor(d) => d.label.as_deref(),
        PipelineDescriptor::MeshPipelineDescriptor(d) => d.label.as_deref(),
    }
}

fn count_compiling_pipelines(cache: Res<PipelineCache>, stats: Res<RenderStats>) {
    let compiling = cache
        .pipelines()
        .filter(|p| is_needed(p) && is_compiling(p))
        .count();
    let ui_ready = cache.pipelines().any(|p| {
        matches!(p.state, CachedPipelineState::Ok(_)) && label(p) == Some("egui_pipeline")
    });
    stats.0.compiling.store(compiling, Ordering::Relaxed);
    stats.0.ui_ready.store(ui_ready, Ordering::Relaxed);
    let frame = stats.0.frames.fetch_add(1, Ordering::Relaxed);

    // VALENCE_DEBUG_PIPELINES=1: every ~2 s, log which pipelines are still compiling.
    if frame.is_multiple_of(120) && std::env::var_os("VALENCE_DEBUG_PIPELINES").is_some() {
        let names: Vec<&str> = cache
            .pipelines()
            .filter(|p| is_compiling(p))
            .map(|p| label(p).unwrap_or("?"))
            .collect();
        if !names.is_empty() {
            info!(
                "pipelines still compiling at render frame {frame}: {}",
                names.join(", ")
            );
        }
    }
}

fn advance_startup(
    mut startup: ResMut<Startup>,
    stats: Res<RenderStats>,
    time: Res<Time<Real>>,
    ready: Res<crate::ui::FontsReady>,
) {
    let now = time.elapsed_secs_f64();
    let frame = stats.frames();
    if frame != startup.last_render_frame {
        startup.last_render_frame = frame;
        startup.idle_frames = if stats.compiling() == 0 {
            startup.idle_frames + 1
        } else {
            0
        };
    }
    let idle = startup.idle_frames >= IDLE_FRAMES;
    match startup.phase {
        Phase::Interface => {
            // Wait only for the interface's own pipeline (and fonts), not for
            // Bevy's background pipelines.
            if (ready.is_ready() && stats.ui_ready() && frame > 3) || now > UI_WAIT_LIMIT_SECS {
                startup.ui_ready_at = Some(now);
                startup.scene_started_at = now;
                startup.phase = Phase::Scene;
                startup.idle_frames = 0;
                info!("startup: interface ready after {now:.1}s; building 3D scene");
            }
        }
        Phase::Scene => {
            if idle {
                startup.scene_ready_at = Some(now);
                startup.phase = Phase::Ready;
                info!(
                    "startup: 3D scene ready after {now:.1}s ({:.1}s of shader compilation)",
                    now - startup.scene_started_at
                );
            }
        }
        Phase::Ready => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_gpu_detection() {
        use wgpu::DeviceType::*;
        assert!(
            is_old_gpu(0x8086, 0x1912, IntegratedGpu),
            "HD 530 (Skylake)"
        );
        assert!(
            is_old_gpu(0x8086, 0x3E92, IntegratedGpu),
            "UHD 630 (Coffee Lake)"
        );
        assert!(
            !is_old_gpu(0x8086, 0x9A49, IntegratedGpu),
            "Iris Xe (Tiger Lake)"
        );
        assert!(!is_old_gpu(0x10DE, 0x2803, DiscreteGpu), "RTX 4060 Ti");
        assert!(!is_old_gpu(0, 0, IntegratedGpu), "Apple Silicon");
        assert!(is_old_gpu(0x1414, 0x8C, Cpu), "WARP software renderer");
    }

    #[test]
    fn setting_parse() {
        assert_eq!(QualitySetting::parse(" Lite "), Some(QualitySetting::Lite));
        assert_eq!(QualitySetting::parse("nonsense"), None);
    }
}
