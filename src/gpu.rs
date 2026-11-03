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

/// Graphics APIs to use: `WGPU_BACKEND` if set, else the platform default.
pub fn choose_backends() -> Backends {
    Backends::from_env().unwrap_or_else(default_backends)
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
#[cfg(windows)]
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
