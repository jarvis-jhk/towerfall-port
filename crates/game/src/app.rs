//! The application loop: a fixed 60 Hz simulation, drawing the latest
//! state into the low-res game canvas, presenting it scaled; two hosts
//! (SDL window, headless offscreen).
//!
//! A [`Scene`] is what the loop runs. The real game will implement it with
//! an adapter around `tfp-sim`: `tick` maps the [`RawInput`] snapshot to
//! per-player inputs (or feeds GGRS) and steps the sim once; `draw` fills
//! the draw list from the current state; sim events become sounds.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use sdl2::event::{Event, WindowEvent};
use sdl2::keyboard::{Keycode, Mod};
use sdl2::video::FullscreenType;
use tfp_render::{
    DrawList, Gpu, OFFSCREEN_FORMAT, OffscreenTarget, PresentSettings, Renderer, ScaleMode,
    SurfaceTarget, TextureId,
};

use crate::audio::Audio;
use crate::input::{RawInput, SdlInput, VirtualPads};
use crate::timing::{FixedStep, TICK_DT};

/// Passed to [`Scene::load`] once at start.
pub struct LoadContext<'a> {
    pub gpu: &'a Gpu,
    pub renderer: &'a mut Renderer,
    pub audio: &'a mut Audio,
}

/// Requests from a scene to its host.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HostRequests {
    /// Switch borderless fullscreen on/off.
    pub fullscreen: Option<bool>,
    /// Leave the application.
    pub quit: bool,
    /// Rumble requests this tick: (pad instance id, strength 0..1,
    /// frames); see `crate::rumble`.
    pub rumble: Vec<(u32, f32, u32)>,
}

/// Passed to [`Scene::tick`] once per 1/60 s.
pub struct TickContext<'a> {
    pub input: &'a RawInput,
    pub audio: &'a mut Audio,
    pub host: &'a mut HostRequests,
    /// Index of this tick (0-based).
    pub tick: u64,
}

/// Passed to [`Scene::draw`] once per rendered frame.
pub struct DrawContext<'a> {
    /// Draws into the game canvas (its default target).
    pub list: &'a mut DrawList,
    /// Drawn after presenting, in game pixels at window resolution (HUD
    /// text that should stay sharp at high scales, side pads).
    pub overlay: &'a mut DrawList,
    /// Scale mode, letterbox colour, screen offset (shake).
    pub present: &'a mut PresentSettings,
    pub game_size: (u32, u32),
    /// Size of the window (or headless output) the canvas is presented in.
    pub window: (u32, u32),
    /// For creating textures/canvases on demand.
    pub gpu: &'a Gpu,
    pub renderer: &'a mut Renderer,
    /// Ticks run so far.
    pub tick: u64,
}

pub trait Scene {
    /// Size of the game canvas (320x240, 420x240, ...); may change between
    /// frames (the host recreates the canvas).
    fn game_size(&self) -> (u32, u32);
    fn load(&mut self, ctx: &mut LoadContext<'_>) -> Result<()>;
    fn tick(&mut self, ctx: &mut TickContext<'_>);
    fn draw(&mut self, ctx: &mut DrawContext<'_>);
    /// Headless: a native frame to save after this draw (proof captures).
    fn take_capture(&mut self) -> Option<PathBuf> {
        None
    }
}

/// The game canvas plus the per-frame draw lists.
pub(crate) struct Frame {
    canvas: TextureId,
    size: (u32, u32),
    list: DrawList,
    overlay: DrawList,
    present: PresentSettings,
}

impl Frame {
    pub(crate) fn new(
        gpu: &Gpu,
        renderer: &mut Renderer,
        size: (u32, u32),
        mode: ScaleMode,
    ) -> Self {
        Self {
            canvas: renderer.create_canvas(gpu, size),
            size,
            list: DrawList::new(),
            overlay: DrawList::new(),
            present: PresentSettings {
                mode,
                ..Default::default()
            },
        }
    }

    /// Lets the scene draw into the canvas (resized if needed).
    pub(crate) fn draw(
        &mut self,
        gpu: &Gpu,
        renderer: &mut Renderer,
        scene: &mut dyn Scene,
        tick: u64,
        window: (u32, u32),
    ) -> Result<()> {
        let want = scene.game_size();
        if want != self.size {
            renderer.destroy_texture(self.canvas);
            self.canvas = renderer.create_canvas(gpu, want);
            self.size = want;
        }
        self.list.clear();
        self.overlay.clear();
        scene.draw(&mut DrawContext {
            list: &mut self.list,
            overlay: &mut self.overlay,
            present: &mut self.present,
            game_size: self.size,
            window,
            gpu,
            renderer,
            tick,
        });
        renderer.render(gpu, &self.list, self.canvas)?;
        Ok(())
    }

    /// The game canvas as an image (flow test sheets).
    pub(crate) fn capture(&self, gpu: &Gpu, renderer: &mut Renderer) -> Result<tfp_render::Image> {
        Ok(renderer.capture(gpu, self.canvas)?)
    }

    fn overlay(&self) -> Option<&DrawList> {
        (!self.overlay.is_empty()).then_some(&self.overlay)
    }
}

/// Saves the presented frame (window-sized) and/or the raw game canvas.
fn save_screenshots(
    gpu: &Gpu,
    renderer: &mut Renderer,
    frame: &Frame,
    window: (u32, u32),
    presented: Option<&PathBuf>,
    native: Option<&PathBuf>,
) -> Result<()> {
    if let Some(path) = presented {
        let target = OffscreenTarget::new(gpu, window, OFFSCREEN_FORMAT);
        renderer.present(
            gpu,
            frame.canvas,
            target.target(),
            &frame.present,
            frame.overlay(),
        )?;
        target.capture(gpu)?.save_png(path)?;
        println!("screenshot: {} ({}x{})", path.display(), window.0, window.1);
    }
    if let Some(path) = native {
        renderer.capture(gpu, frame.canvas)?.save_png(path)?;
        println!(
            "native screenshot: {} ({}x{})",
            path.display(),
            frame.size.0,
            frame.size.1
        );
    }
    Ok(())
}

/// Options for the headless host.
pub struct HeadlessOptions {
    /// Size of the virtual window the canvas is presented into.
    pub window: (u32, u32),
    pub mode: ScaleMode,
    pub frames: u32,
    pub screenshot: Option<PathBuf>,
    pub native_screenshot: Option<PathBuf>,
    pub wav: Option<PathBuf>,
    /// Attach this many SDL virtual controllers driven by a script.
    pub virtual_pads: usize,
    /// Unplug virtual pad 0 at this tick (hot-plug tests).
    pub unplug_at: Option<u64>,
    /// Measure every frame (waiting for the GPU) and print percentiles.
    pub frame_times: bool,
}

/// Percentiles of per-frame times in milliseconds.
fn report_times(label: &str, v: &mut [f64]) {
    if v.is_empty() {
        return;
    }
    v.sort_by(f64::total_cmp);
    let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    println!(
        "frame times {label:>10}: mean {mean:.3} ms, p50 {:.3}, p95 {:.3}, p99 {:.3}, max {:.3} ({} frames)",
        at(0.5),
        at(0.95),
        at(0.99),
        at(1.0),
        v.len()
    );
}

/// Scripted input for virtual pads: each pad moves its stick in a circle
/// at its own phase and taps A/X in a pattern; pad 0 taps Y at tick 30.
fn drive_virtual_pads(pads: &VirtualPads, tick: u64) {
    use sdl2::controller::{Axis, Button};
    for i in 0..pads.len() {
        let t = tick as f32 / 60.0 * 2.0 + i as f32 * 0.7;
        pads.set_axis(i, Axis::LeftX, (t.cos() * 30000.0) as i16);
        pads.set_axis(i, Axis::LeftY, (t.sin() * 30000.0) as i16);
        let phase = (tick / 10 + i as u64) % 4;
        pads.set_button(i, Button::A, phase == 0);
        pads.set_button(i, Button::X, phase == 2);
        // Pad 0 taps Y once (the demo toggles its screen wave).
        pads.set_button(i, Button::Y, i == 0 && tick == 30);
    }
}

/// Renders `frames` frames offscreen at a virtual 60 Hz (one tick per
/// frame), then saves screenshots. Deterministic: no wall clock, no window,
/// no audio device.
pub fn run_headless(scene: &mut dyn Scene, opts: &HeadlessOptions) -> Result<()> {
    let gpu = Gpu::headless().context("creating a headless GPU device")?;
    println!("GPU: {}", gpu.describe());
    let mut renderer = Renderer::new(&gpu);
    let mut audio = Audio::offline(opts.wav.as_deref())?;
    println!("audio: {}", audio.description());
    scene.load(&mut LoadContext {
        gpu: &gpu,
        renderer: &mut renderer,
        audio: &mut audio,
    })?;

    // Optional: real SDL controller events from virtual pads.
    let mut sdl_input = None;
    if opts.virtual_pads > 0 {
        crate::platform::set_hints();
        let sdl = sdl2::init().map_err(anyhow::Error::msg)?;
        let gc = sdl.game_controller().map_err(anyhow::Error::msg)?;
        let pump = sdl.event_pump().map_err(anyhow::Error::msg)?;
        let pads = VirtualPads::attach(opts.virtual_pads)?;
        println!("attached {} virtual controllers", pads.len());
        sdl_input = Some((sdl, pump, SdlInput::new(gc), pads));
    }

    let mut frame = Frame::new(&gpu, &mut renderer, scene.game_size(), opts.mode);
    let mut input = RawInput::default();
    let mut host = HostRequests::default();
    let started = Instant::now();
    let (mut t_tick, mut t_draw, mut t_all) = (Vec::new(), Vec::new(), Vec::new());
    let mut t_cpu = Vec::new();
    for tick in 0..opts.frames as u64 {
        let t0 = Instant::now();
        if let Some((_, pump, sdl_in, pads)) = &mut sdl_input {
            if opts.unplug_at == Some(tick) {
                pads.detach(0);
                println!("tick {tick}: virtual pad 0 unplugged");
            }
            drive_virtual_pads(pads, tick);
            // SDL updates virtual joysticks when events are pumped.
            for e in pump.poll_iter() {
                sdl_in.handle_event(&e);
            }
            sdl_in.snapshot(&mut input);
        }
        scene.tick(&mut TickContext {
            input: &input,
            audio: &mut audio,
            host: &mut host,
            tick,
        });
        if host.quit {
            break;
        }
        let t1 = Instant::now();
        frame.draw(&gpu, &mut renderer, scene, tick + 1, opts.window)?;
        if opts.frame_times {
            t_cpu.push(t1.elapsed().as_secs_f64() * 1e3);
            gpu.wait_idle();
            let t2 = Instant::now();
            t_tick.push((t1 - t0).as_secs_f64() * 1e3);
            t_draw.push((t2 - t1).as_secs_f64() * 1e3);
            t_all.push((t2 - t0).as_secs_f64() * 1e3);
        }
        if let Some(path) = scene.take_capture() {
            renderer.capture(&gpu, frame.canvas)?.save_png(&path)?;
            println!("capture: {}", path.display());
        }
        audio.advance_offline(TICK_DT)?;
    }
    if opts.frame_times {
        let mut slow: Vec<(usize, f64, f64)> = t_all
            .iter()
            .enumerate()
            .filter(|&(_, &t)| t > 1000.0 / 60.0)
            .map(|(i, &t)| (i, t, t_tick[i]))
            .collect();
        println!("frames over 16.7 ms: {}", slow.len());
        slow.sort_by(|a, b| b.1.total_cmp(&a.1));
        for (i, t, tk) in slow.iter().take(8) {
            println!("  frame {i}: {t:.1} ms (tick {tk:.1} ms)");
        }
        report_times("tick", &mut t_tick);
        report_times("draw", &mut t_draw);
        report_times("draw (CPU)", &mut t_cpu);
        report_times("total", &mut t_all);
    }
    let s = renderer.stats();
    println!(
        "rendered {} frames at {}x{} in {:.2?}; last frame: {} quads, {} draw calls, {} passes; {} controllers",
        opts.frames,
        frame.size.0,
        frame.size.1,
        started.elapsed(),
        s.quads,
        s.draw_calls,
        s.passes,
        input.pads.len()
    );
    save_screenshots(
        &gpu,
        &mut renderer,
        &frame,
        opts.window,
        opts.screenshot.as_ref(),
        opts.native_screenshot.as_ref(),
    )?;
    audio.finish()?;
    if let Some(w) = &opts.wav {
        println!("audio written: {}", w.display());
    }
    Ok(())
}

/// Options for the windowed host.
pub struct WindowOptions {
    pub size: (u32, u32),
    pub mode: ScaleMode,
    pub fullscreen: bool,
    pub vsync: bool,
    /// Quit after this many frames (0 = run until closed).
    pub frames: u32,
    pub screenshot: Option<PathBuf>,
    pub native_screenshot: Option<PathBuf>,
}

/// Opens the SDL window and runs until it is closed. F11 or Alt+Enter
/// toggles borderless fullscreen; F9 cycles the scale mode.
pub fn run_windowed(scene: &mut dyn Scene, opts: &WindowOptions) -> Result<()> {
    crate::platform::set_hints();
    let sdl = sdl2::init().map_err(anyhow::Error::msg)?;
    let video = sdl.video().map_err(anyhow::Error::msg)?;
    let controllers = sdl.game_controller().map_err(anyhow::Error::msg)?;
    let mut events = sdl.event_pump().map_err(anyhow::Error::msg)?;

    let mut win = video.window("towerfall-port", opts.size.0, opts.size.1);
    win.position_centered().resizable().allow_highdpi();
    let mut window = win.build().context("creating window")?;
    window.set_minimum_size(320, 240).ok();
    if opts.fullscreen {
        window
            .set_fullscreen(FullscreenType::Desktop)
            .map_err(anyhow::Error::msg)?;
    }
    crate::platform::check_native_handles(&window)?;
    let instance = tfp_render::new_instance_for_display(Box::new(
        crate::platform::DisplayHandle::of(&window)?,
    ));
    // SAFETY: the surface is dropped before the window (explicit drops at
    // the end of this function), so the handles stay valid.
    let surface = unsafe {
        instance.create_surface_unsafe(tfp_render::wgpu::SurfaceTargetUnsafe::from_window(&window)?)
    }
    .context("creating wgpu surface")?;
    let gpu = Gpu::new(instance, Some(&surface))?;
    log::info!("GPU: {}", gpu.describe());
    let mut surface = SurfaceTarget::new(
        &gpu,
        surface,
        crate::platform::window_pixels(&window),
        opts.vsync,
    )?;
    log::info!("surface format {:?}", surface.format());
    let mut renderer = Renderer::new(&gpu);

    let mut audio = match sdl
        .audio()
        .map_err(anyhow::Error::msg)
        .and_then(|a| Audio::open_device(&a))
    {
        Ok(a) => a,
        Err(e) => {
            log::warn!("{e:#}; continuing without sound");
            Audio::offline(None)?
        }
    };
    let mut input = SdlInput::new(controllers);
    let mut rumbles = crate::rumble::Rumbles::default();
    scene.load(&mut LoadContext {
        gpu: &gpu,
        renderer: &mut renderer,
        audio: &mut audio,
    })?;

    let mut frame = Frame::new(&gpu, &mut renderer, scene.game_size(), opts.mode);
    let mut step = FixedStep::new();
    let refresh = |w: &sdl2::video::Window| {
        w.display_mode()
            .map(|m| m.refresh_rate as f64)
            .unwrap_or(0.0)
    };
    // Display refresh (SDL reports 0 when unknown, e.g. virtual X servers;
    // then no snapping, and 60 Hz is assumed for the pacing guard).
    let update_refresh = |w: &sdl2::video::Window, step: &mut FixedStep| {
        let hz = refresh(w);
        if opts.vsync {
            step.set_refresh_rate(hz);
        }
        if hz > 1.0 { hz } else { 60.0 }
    };
    let mut refresh_hz = update_refresh(&window, &mut step);
    let mut raw = RawInput::default();
    let mut host = HostRequests::default();
    let mut last = Instant::now();
    let mut frames = 0u32;
    'main: loop {
        for event in events.poll_iter() {
            match &event {
                Event::Quit { .. } => break 'main,
                Event::Window {
                    win_event: WindowEvent::SizeChanged(..) | WindowEvent::Resized(..),
                    ..
                } => surface.resize(&gpu, crate::platform::window_pixels(&window)),
                Event::Window {
                    win_event: WindowEvent::DisplayChanged(..) | WindowEvent::Moved(..),
                    ..
                } => {
                    refresh_hz = update_refresh(&window, &mut step);
                    log::info!("display refresh {refresh_hz} Hz");
                }
                Event::KeyDown {
                    keycode: Some(k),
                    keymod,
                    repeat: false,
                    ..
                } if *k == Keycode::F11
                    || (*k == Keycode::Return
                        && keymod.intersects(Mod::LALTMOD | Mod::RALTMOD)) =>
                {
                    let next = match window.fullscreen_state() {
                        FullscreenType::Off => FullscreenType::Desktop,
                        _ => FullscreenType::Off,
                    };
                    if let Err(e) = window.set_fullscreen(next) {
                        log::warn!("fullscreen toggle: {e}");
                    }
                    continue;
                }
                Event::KeyDown {
                    keycode: Some(Keycode::F9),
                    repeat: false,
                    ..
                } => {
                    frame.present.mode = match frame.present.mode {
                        ScaleMode::Integer => ScaleMode::Fit,
                        ScaleMode::Fit => ScaleMode::Stretch,
                        ScaleMode::Stretch => ScaleMode::Integer,
                    };
                    log::info!("scale mode {:?}", frame.present.mode);
                    continue;
                }
                _ => {}
            }
            input.handle_event(&event);
        }

        let now = Instant::now();
        let dt = (now - last).as_secs_f64();
        last = now;
        let ticks = step.advance(dt);
        for _ in 0..ticks {
            input.snapshot(&mut raw);
            scene.tick(&mut TickContext {
                input: &raw,
                audio: &mut audio,
                host: &mut host,
                tick: step.ticks(),
            });
            rumbles.tick();
            for (pad, s, frames) in host.rumble.drain(..) {
                if let Some((s, f)) = rumbles.request(pad, s, frames) {
                    input.rumble(pad, s, s, crate::rumble::frames_ms(f));
                }
            }
        }
        if let Some(on) = host.fullscreen.take() {
            let mode = if on {
                FullscreenType::Desktop
            } else {
                FullscreenType::Off
            };
            if let Err(e) = window.set_fullscreen(mode) {
                log::warn!("fullscreen: {e}");
            }
        }
        if host.quit {
            break 'main;
        }

        if ticks > 0 || frames == 0 {
            let size = crate::platform::window_pixels(&window);
            frame.draw(&gpu, &mut renderer, scene, step.ticks(), size)?;
        }
        match surface.acquire(&gpu) {
            Some(out) => {
                renderer.present(
                    &gpu,
                    frame.canvas,
                    out.target(),
                    &frame.present,
                    frame.overlay(),
                )?;
                out.present(&gpu);
                frames += 1;
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
        if opts.frames > 0 && frames >= opts.frames {
            break;
        }
        // Frame pacing: without vsync wait for the next tick. With vsync the
        // present blocks; if it returned almost at once (no vblank, e.g. a
        // virtual X server, or a compositor ignoring FIFO), do the same
        // instead of spinning.
        let spent = now.elapsed().as_secs_f64();
        if !opts.vsync || spent < 0.5 / refresh_hz {
            let wait = step.until_next_tick() - spent;
            if wait > 0.0 {
                std::thread::sleep(Duration::from_secs_f64(wait));
            }
        }
    }
    let size = crate::platform::window_pixels(&window);
    save_screenshots(
        &gpu,
        &mut renderer,
        &frame,
        size,
        opts.screenshot.as_ref(),
        opts.native_screenshot.as_ref(),
    )?;
    println!("ran {frames} frames, {} ticks", step.ticks());
    drop(surface);
    drop(window);
    audio.finish()
}
