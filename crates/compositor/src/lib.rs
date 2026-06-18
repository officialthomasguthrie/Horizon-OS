//! The Horizon Wayland compositor (L5, the experience layer).
//!
//! This is the headless core: a real Wayland display server that real clients
//! connect to. It owns the core protocol, [`wl_compositor`], [`wl_shm`],
//! [`xdg_shell`], [`wl_seat`], and [`wl_output`], and a scene graph (a Smithay
//! `Space`) that tracks every mapped toplevel. It does not yet paint to a
//! screen: a display backend (a winit window nested in an existing session,
//! then a real DRM/KMS backend) sits on top of this core later. That split is
//! deliberate. The protocol and scene logic is the part that can be proven
//! without a display or a GPU, so it is built and tested headlessly here; the
//! on-screen backend is the part that needs real hardware and is verified by
//! eye, the same way the Constellation's networking core is fully tested on one
//! host while only NAT traversal waits for real machines.
//!
//! Each app on Horizon is meant to be a confined Wayland client living in a
//! Cell; the exec path that makes that real already exists in the `cells`
//! crate. Glass, the live transparency surface over the Weave audit log, will
//! land here as a compositor surface once there is something to draw it on.
//!
//! Linux only. On other hosts the crate compiles (so the workspace builds on
//! darwin) but [`available`] reports false and there is no `Compositor`.
//!
//! [`wl_compositor`]: https://wayland.app/protocols/wayland#wl_compositor
//! [`wl_shm`]: https://wayland.app/protocols/wayland#wl_shm
//! [`xdg_shell`]: https://wayland.app/protocols/xdg-shell
//! [`wl_seat`]: https://wayland.app/protocols/wayland#wl_seat
//! [`wl_output`]: https://wayland.app/protocols/wayland#wl_output

mod error;
pub use error::{Error, Result};

#[cfg(target_os = "linux")]
mod server;
#[cfg(target_os = "linux")]
pub use server::Compositor;

/// Whether a compositor can run on this host. Linux only; elsewhere there is no
/// Wayland server to host and [`Compositor`] does not exist.
pub fn available() -> bool {
    cfg!(target_os = "linux")
}
