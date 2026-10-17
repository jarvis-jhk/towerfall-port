//! SDL2 platform helpers shared by the windowed host and `--list-devices`.

use sdl2::hint;

/// SDL hints set before subsystems start.
pub fn set_hints() {
    // Controllers keep working when the window is unfocused (couch play
    // with a second monitor, streaming).
    hint::set("SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS", "1");
    // Windows: RawInput + HIDAPI lift XInput's 4-controller limit.
    hint::set("SDL_JOYSTICK_RAWINPUT", "1");
    hint::set("SDL_JOYSTICK_HIDAPI", "1");
    // Do not let SDL disable the compositor or the screensaver behaviour
    // differently from other games.
    hint::set("SDL_VIDEO_X11_NET_WM_BYPASS_COMPOSITOR", "0");
}

/// The window's size in pixels (differs from `Window::size` on high-DPI
/// setups).
pub fn window_pixels(window: &sdl2::video::Window) -> (u32, u32) {
    let (mut w, mut h) = (0, 0);
    // SAFETY: valid window pointer for the duration of the call.
    unsafe { sdl2::sys::SDL_GetWindowSizeInPixels(window.raw(), &mut w, &mut h) };
    (w.max(1) as u32, h.max(1) as u32)
}

/// Fails (instead of panicking inside `raw-window-handle`) when the SDL
/// video driver cannot expose native window handles (e.g. the `offscreen`
/// or `dummy` drivers).
pub fn check_native_handles(window: &sdl2::video::Window) -> anyhow::Result<()> {
    // SAFETY: zeroed SysWMinfo with the version filled in is what
    // SDL_GetWindowWMInfo expects.
    unsafe {
        let mut info: sdl2::sys::SDL_SysWMinfo = std::mem::zeroed();
        sdl2::sys::SDL_GetVersion(&mut info.version);
        if sdl2::sys::SDL_GetWindowWMInfo(window.raw(), &mut info) == sdl2::sys::SDL_bool::SDL_FALSE
        {
            anyhow::bail!(
                "SDL video driver '{}' has no native window handles ({}); use --headless",
                window.subsystem().current_video_driver(),
                sdl2::get_error()
            );
        }
    }
    Ok(())
}

/// The window's display connection (X11 `Display*`, Wayland `wl_display`,
/// nothing on Windows) as a `Send + Sync` handle for wgpu's instance.
///
/// The connection belongs to SDL's video subsystem; the caller keeps that
/// alive for as long as the wgpu instance exists.
#[derive(Debug)]
pub struct DisplayHandle(raw_window_handle::RawDisplayHandle);

// SAFETY: the handle is an opaque pointer to a display connection that is
// valid (and thread-safe to pass to the graphics driver) while SDL's video
// subsystem lives, which outlives every wgpu object using it.
unsafe impl Send for DisplayHandle {}
unsafe impl Sync for DisplayHandle {}

impl DisplayHandle {
    pub fn of(window: &sdl2::video::Window) -> anyhow::Result<Self> {
        use raw_window_handle::HasDisplayHandle;
        let h = window
            .display_handle()
            .map_err(|e| anyhow::anyhow!("window has no display handle: {e}"))?;
        Ok(Self(h.as_raw()))
    }
}

impl raw_window_handle::HasDisplayHandle for DisplayHandle {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        // SAFETY: see the type's invariant.
        Ok(unsafe { raw_window_handle::DisplayHandle::borrow_raw(self.0) })
    }
}
