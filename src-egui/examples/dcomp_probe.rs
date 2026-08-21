//! Standalone research probe for issue #131 (spike: per-pixel translucency in
//! Desktop Wallpaper mode via DirectComposition). Kept as a minimal reference/
//! repro for the technique now used in `bin/wallpaper.rs`.
//!
//! Checks, with no egui/eframe involvement so a failure isolates the DComp/
//! WorkerW interaction from the rest of the app:
//! 1. Whether `wgpu`'s DX12 `DxgiFromVisual` presentation system produces a
//!    real per-pixel-alpha, DirectComposition-composited swap chain.
//! 2. Whether that survives being reparented into the desktop `WorkerW` layer
//!    via the app's real `win32_wallpaper::attach`.
//! 3. Whether `win_opacity::set_no_redirection_bitmap` — applied *after* the
//!    wgpu surface already exists, since eframe/egui-winit has no hook to
//!    request `WS_EX_NOREDIRECTIONBITMAP` at window-creation time — is still
//!    effective (it is; see `docs/architecture.md` "Desktop wallpaper mode").
//!
//! Run: `cargo run --manifest-path src-egui/Cargo.toml --example dcomp_probe`
//! Expected: a half-transparent window, with whatever is behind it visible
//! through it, that reparents into WorkerW ~1s after appearing (log line
//! "attached to WorkerW") and survives Win+D / staying under desktop icons.
//! Adapter info and reported alpha modes are logged to stderr.

use std::sync::Arc;
use std::time::{Duration, Instant};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use rigstats_egui::{win32_wallpaper, win_opacity};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

const REDRAW_INTERVAL: Duration = Duration::from_millis(250);
const ATTACH_CHECK_INTERVAL: Duration = Duration::from_secs(1);

fn hwnd_of(window: &Window) -> isize {
    match window.window_handle().expect("window handle").as_raw() {
        RawWindowHandle::Win32(handle) => handle.hwnd.get(),
        _ => unreachable!("Windows-only probe"),
    }
}

struct GpuState {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
}

#[derive(Default)]
struct Probe {
    window: Option<Arc<Window>>,
    gpu: Option<GpuState>,
    next_redraw: Option<Instant>,
    hwnd: Option<isize>,
    attached: bool,
    next_attach_check: Option<Instant>,
}

impl Probe {
    fn init_gpu(&mut self, window: Arc<Window>) {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            // Forced explicitly: the default `Backends::PRIMARY` includes Vulkan,
            // which most GPUs will be picked over DX12 if left ambiguous — silently
            // making the `backend_options.dx12` block below inert.
            backends: wgpu::Backends::DX12,
            backend_options: wgpu::BackendOptions {
                dx12: wgpu::Dx12BackendOptions {
                    presentation_system: wgpu::Dx12SwapchainKind::DxgiFromVisual,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let surface = instance
            .create_surface(window.clone())
            .expect("create DComp-backed surface");

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("request DX12 adapter (DxgiFromVisual)");
        eprintln!("[dcomp_probe] adapter: {:?}", adapter.get_info());

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .expect("request device");

        let caps = surface.get_capabilities(&adapter);
        eprintln!("[dcomp_probe] surface formats: {:?}", caps.formats);
        eprintln!("[dcomp_probe] surface alpha modes: {:?}", caps.alpha_modes);

        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| *f == wgpu::TextureFormat::Bgra8Unorm)
            .unwrap_or(caps.formats[0]);

        let alpha_mode = if caps
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
        {
            wgpu::CompositeAlphaMode::PreMultiplied
        } else {
            caps.alpha_modes[0]
        };
        eprintln!("[dcomp_probe] chosen alpha mode: {alpha_mode:?}");

        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        self.gpu = Some(GpuState {
            surface,
            device,
            queue,
            config,
        });
        self.window = Some(window);
    }

    fn render(&mut self) {
        let Some(gpu) = self.gpu.as_mut() else { return };

        let surface_texture = match gpu.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => return,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                gpu.surface.configure(&gpu.device, &gpu.config);
                return;
            }
            wgpu::CurrentSurfaceTexture::Validation => return,
        };
        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("dcomp-probe-encoder"),
            });
        {
            // Half-transparent dark fill, premultiplied to match the swap chain's
            // `PreMultiplied` alpha mode (mirrors how `theme::premul()` bakes opacity
            // into panel colors in the real app — see Phase 4 of the spike plan).
            let alpha = 0.5;
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("dcomp-probe-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.05 * alpha,
                            g: 0.05 * alpha,
                            b: 0.07 * alpha,
                            a: alpha,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        gpu.queue.submit(Some(encoder.finish()));
        surface_texture.present();
    }
}

impl ApplicationHandler for Probe {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        // Deliberately NOT setting `.with_transparent(true)`/a creation-time
        // no-redirection-bitmap flag here — eframe/egui-winit can't request
        // `WS_EX_NOREDIRECTIONBITMAP` at window-creation time either (only
        // `with_drag_and_drop`/`with_skip_taskbar` are wired through in
        // `egui-winit-0.34.3/src/lib.rs`), so this probe mirrors that constraint:
        // create the window and wgpu surface first, then apply the style via
        // `win_opacity::set_no_redirection_bitmap` — the same function
        // `bin/wallpaper.rs` calls — after the fact. Verified this ordering still
        // works (issue #131), despite Microsoft's docs suggesting the style is
        // creation-time-only.
        let attrs = WindowAttributes::default()
            .with_title("dcomp_probe")
            .with_inner_size(winit::dpi::LogicalSize::new(500.0, 400.0));
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        let hwnd = hwnd_of(&window);
        self.init_gpu(window);
        win_opacity::set_no_redirection_bitmap(hwnd);

        // Reparent while still top-level, mirroring `bin/wallpaper.rs`'s real ordering
        // (opacity/DComp setup happens before `attach`, since some window properties
        // are rejected once the window is a WorkerW child).
        self.hwnd = Some(hwnd);
        self.attached = win32_wallpaper::attach(hwnd);
        eprintln!("[dcomp_probe] initial attach to WorkerW: {}", self.attached);
        self.next_attach_check = Some(Instant::now() + ATTACH_CHECK_INTERVAL);

        self.next_redraw = Some(Instant::now());
        event_loop.set_control_flow(ControlFlow::Wait);
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.config.width = size.width.max(1);
                    gpu.config.height = size.height.max(1);
                    gpu.surface.configure(&gpu.device, &gpu.config);
                }
            }
            WindowEvent::RedrawRequested => self.render(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(next) = self.next_redraw else { return };
        if Instant::now() >= next {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
            self.next_redraw = Some(Instant::now() + REDRAW_INTERVAL);
        }

        // Mirrors the real host's re-attach safety net (`bin/wallpaper.rs`): if
        // Explorer restarts and destroys the WorkerW hierarchy, re-attach.
        if let (Some(hwnd), Some(check_at)) = (self.hwnd, self.next_attach_check) {
            if Instant::now() >= check_at {
                let now_attached = win32_wallpaper::is_attached(hwnd);
                if now_attached != self.attached {
                    eprintln!("[dcomp_probe] WorkerW attachment changed: {now_attached}");
                }
                if !now_attached {
                    let ok = win32_wallpaper::attach(hwnd);
                    eprintln!("[dcomp_probe] re-attach to WorkerW: {ok}");
                    self.attached = ok;
                } else {
                    self.attached = true;
                }
                self.next_attach_check = Some(Instant::now() + ATTACH_CHECK_INTERVAL);
            }
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_redraw.unwrap()));
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("create event loop");
    let mut app = Probe::default();
    event_loop.run_app(&mut app).expect("run event loop");
}
