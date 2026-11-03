//! Graphics backend selection.
//!
//! On Windows we prefer DirectX 12 over Vulkan: older Intel integrated GPUs
//! (e.g. HD 530 on Intel's final legacy driver) crash inside their Vulkan
//! driver (`igvk64.dll`), while DX12 on the same hardware is solid. If no DX12
//! adapter exists we fall back to Vulkan, then OpenGL. Setting `WGPU_BACKEND`
//! (e.g. `vulkan`, `dx12`, `gl`) still overrides everything.
//!
//! Shaders on DX12 are compiled with DXC, statically linked on Windows (see
//! Cargo.toml). The older FXC compiler left the window black for over a
//! minute on an Intel HD 530. `WGPU_DX12_COMPILER` overrides this.

use bevy::render::settings::{Backends, RenderCreation, WgpuSettings};

/// The player's graphics API preference (Settings window).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiSetting {
    Auto,
    Dx12,
    Vulkan,
    Metal,
    OpenGl,
}

impl ApiSetting {
    pub const ALL: [ApiSetting; 5] = [
        Self::Auto,
        Self::Dx12,
        Self::Vulkan,
        Self::Metal,
        Self::OpenGl,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto (recommended)",
            Self::Dx12 => "DirectX 12",
            Self::Vulkan => "Vulkan",
            Self::Metal => "Metal",
            Self::OpenGl => "OpenGL (compatibility)",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Dx12 => "dx12",
            Self::Vulkan => "vulkan",
            Self::Metal => "metal",
            Self::OpenGl => "gl",
        }
    }

    fn backends(self) -> Option<Backends> {
        match self {
            Self::Auto => None,
            Self::Dx12 => Some(Backends::DX12),
            Self::Vulkan => Some(Backends::VULKAN),
            Self::Metal => Some(Backends::METAL),
            Self::OpenGl => Some(Backends::GL),
        }
    }

    /// Choices that make sense on this operating system.
    pub fn available(self) -> bool {
        match self {
            Self::Dx12 => cfg!(windows),
            Self::Metal => cfg!(target_os = "macos"),
            Self::Vulkan | Self::OpenGl => !cfg!(target_os = "macos"),
            Self::Auto => true,
        }
    }

    pub fn saved() -> ApiSetting {
        let v = crate::settings_file::get("graphics_api").unwrap_or_default();
        Self::ALL
            .into_iter()
            .find(|a| a.key() == v.trim())
            .unwrap_or(Self::Auto)
    }

    pub fn save(self) {
        crate::settings_file::set("graphics_api", self.key());
    }
}

/// Graphics APIs to use: `WGPU_BACKEND` if set, else the saved preference
/// (only if a GPU is actually available through it, so a bad choice can't
/// stop the game starting), else the platform default.
pub fn choose_backends() -> Backends {
    if let Some(b) = Backends::from_env() {
        return b;
    }
    let saved = ApiSetting::saved();
    if let Some(b) = saved.backends()
        && saved.available()
        && has_adapter(b)
    {
        return b;
    }
    default_backends()
}

pub fn render_creation(backends: Backends) -> RenderCreation {
    WgpuSettings {
        backends: Some(backends),
        ..Default::default()
    }
    .into()
}

#[cfg(windows)]
fn default_backends() -> Backends {
    if has_adapter(Backends::DX12) {
        Backends::DX12
    } else {
        Backends::VULKAN | Backends::GL
    }
}

#[cfg(not(windows))]
fn default_backends() -> Backends {
    Backends::all()
}

fn instance(backends: Backends) -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    })
}

/// Is there at least one usable GPU adapter for these backends? Probing only
/// enumerates adapters (no device is created), and any panic counts as "no".
fn has_adapter(backends: Backends) -> bool {
    crate::crash::guard("adapter probe", || {
        !bevy::tasks::block_on(instance(backends).enumerate_adapters(backends)).is_empty()
    })
    .unwrap_or(false)
}

/// The adapter Bevy will most likely pick (same backends and power
/// preference), used to choose graphics quality. No device is created.
pub fn probe_adapter(backends: Backends) -> Option<wgpu::AdapterInfo> {
    crate::crash::guard("adapter probe", || {
        let options = wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        };
        bevy::tasks::block_on(instance(backends).request_adapter(&options))
            .ok()
            .map(|a| a.get_info())
    })
    .flatten()
}
