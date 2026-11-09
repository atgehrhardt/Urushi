//! GPU rendering for Urushi (ADR 0006). The only crate that touches wgpu.
//!
//! - [`SurfaceRenderer`]: draws `urushi-ui` scenes into a window (Metal, Vulkan, DX12, GLES).
//! - [`Offscreen`]: draws into memory, for pixel tests and screenshots.
//!
//! Both use the same `SceneRenderer`: two instanced pipelines (rounded quads, and
//! glyphs from a shelf-packed atlas), batched in paint order with scissor clips.

mod atlas;
mod gpu;
mod image;
mod offscreen;
mod renderer;
mod surface;

pub use gpu::Gpu;
pub use offscreen::{Image, Offscreen};
pub use renderer::{FrameTarget, SceneRenderer};
pub use surface::SurfaceRenderer;

use thiserror::Error;

/// Rendering failures.
#[derive(Debug, Error)]
pub enum RenderError {
    /// No suitable GPU adapter.
    #[error("no usable GPU adapter: {0}")]
    NoAdapter(String),
    /// The device couldn't be opened or failed.
    #[error("GPU device error: {0}")]
    Device(String),
    /// The window surface couldn't be created or configured.
    #[error("surface error: {0}")]
    Surface(String),
}
