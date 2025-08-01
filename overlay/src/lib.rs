#![feature(str_from_utf16_endian)]
use std::time::Instant;

use clipboard::ClipboardSupport;
use copypasta::ClipboardContext;
use directx::DirectXRenderBackend;
use font::FontAtlasBuilder;
use imgui::{
    Context,
    FontAtlas,
};
use imgui_winit_support::{
    winit::{
        event::{
            Event,
            WindowEvent,
        },
        event_loop::EventLoop,
        window::Window,
    },
    HiDpiMode,
    WinitPlatform,
};
use input::{
    KeyboardInputSystem,
    MouseInputSystem,
};
use obfstr::obfstr;
use opengl::OpenGLRenderBackend;
use vulkan::VulkanRenderBackend;
use window_tracker::{
    ActiveTracker,
    WindowTracker,
};
use windows::Win32::{
    Foundation::{
        BOOL,
        HWND,
        RECT,
    },
    Graphics::{
        Dwm::{
            DwmEnableBlurBehindWindow,
            DwmExtendFrameIntoClientArea,
            DwmIsCompositionEnabled,
            DWM_BB_ENABLE,
            DWM_BLURBEHIND,
        },
        Gdi::HRGN,
    },
    UI::{
        Controls::MARGINS,
        WindowsAndMessaging::{
            GetClientRect,
            SetWindowDisplayAffinity,
            SetWindowLongA,
            SetWindowLongPtrA,
            SetWindowPos,
            ShowWindow,
            GWL_EXSTYLE,
            GWL_STYLE,
            HWND_TOPMOST,
            SWP_NOACTIVATE,
            SWP_NOMOVE,
            SWP_NOSIZE,
            SW_SHOWNOACTIVATE,
            WDA_EXCLUDEFROMCAPTURE,
            WDA_NONE,
            WS_CLIPSIBLINGS,
            WS_EX_LAYERED,
            WS_EX_NOACTIVATE,
            WS_EX_TOOLWINDOW,
            WS_EX_TRANSPARENT,
            WS_POPUP,
            WS_VISIBLE,
        },
    },
};

mod clipboard;
mod error;
pub use error::*;
mod input;
mod window_tracker;
pub use window_tracker::OverlayTarget;

mod directx;
mod opengl;
mod vulkan;

mod perf;
pub use perf::PerfTracker;

mod font;
mod util;

pub use font::UnicodeTextRenderer;
pub use util::show_error_message;
use winit::{
    dpi::PhysicalSize,
    raw_window_handle::{
        HasWindowHandle,
        RawWindowHandle,
    },
    window::WindowAttributes,
};

fn create_window(
    event_loop: &EventLoop<()>,
    title: &str,
    target_size: Option<(u32, u32)>,
) -> Result<(HWND, Window)> {
    let mut window_attrs = WindowAttributes::default()
        .with_title(title.to_owned())
        .with_visible(false);

    if let Some((width, height)) = target_size {
        window_attrs = window_attrs.with_inner_size(PhysicalSize::new(width, height));
    }

    #[allow(deprecated)]
    let window = event_loop.create_window(window_attrs)?;

    let RawWindowHandle::Win32(handle) = window.window_handle().unwrap().as_raw() else {
        panic!()
    };
    let hwnd = HWND(handle.hwnd.get());

    {
        unsafe {
            // Make it transparent
            SetWindowLongA(
                hwnd,
                GWL_STYLE,
                (WS_POPUP | WS_VISIBLE | WS_CLIPSIBLINGS).0 as i32,
            );

            let ex_style = (WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT)
                .0 as isize;
            SetWindowLongPtrA(hwnd, GWL_EXSTYLE, ex_style);

            if !DwmIsCompositionEnabled()?.as_bool() {
                return Err(OverlayError::DwmCompositionDisabled);
            }

            let mut bb: DWM_BLURBEHIND = Default::default();
            bb.dwFlags = DWM_BB_ENABLE;
            bb.fEnable = BOOL::from(true);
            bb.hRgnBlur = HRGN::default();
            DwmEnableBlurBehindWindow(hwnd, &bb)?;

            DwmExtendFrameIntoClientArea(
                hwnd,
                &MARGINS {
                    cxLeftWidth: -1,
                    cxRightWidth: -1,
                    cyBottomHeight: -1,
                    cyTopHeight: -1,
                },
            )?;

            // Move the window to the top
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }

    Ok((hwnd, window))
}

fn create_imgui_context(_options: &OverlayOptions) -> Result<(WinitPlatform, imgui::Context)> {
    let mut imgui = Context::create();
    imgui.set_ini_filename(None);

    let platform = WinitPlatform::new(&mut imgui);

    match ClipboardContext::new() {
        Ok(backend) => imgui.set_clipboard_backend(ClipboardSupport(backend)),
        Err(error) => log::warn!("Failed to initialize clipboard: {}", error),
    };

    Ok((platform, imgui))
}

pub struct OverlayOptions {
    pub title: String,
    pub target: OverlayTarget,
    pub register_fonts_callback: Option<Box<dyn Fn(&mut FontAtlas) -> ()>>,
}

pub trait RenderBackend {
    fn update_fonts_texture(&mut self, imgui: &mut imgui::Context);
    fn render_frame(
        &mut self,
        perf: &mut PerfTracker,
        window: &Window,
        draw_data: &imgui::DrawData,
    );
}

pub struct System {
    pub event_loop: EventLoop<()>,

    pub overlay_window: Window,
    pub overlay_hwnd: HWND,

    pub platform: WinitPlatform,

    pub imgui: Context,
    pub imgui_fonts: FontAtlasBuilder,
    pub imgui_register_fonts_callback: Option<Box<dyn Fn(&mut FontAtlas) -> ()>>,

    pub window_tracker: WindowTracker,

    renderer: Box<dyn RenderBackend>,
}

pub fn init(options: OverlayOptions) -> Result<System> {
    let event_loop = EventLoop::new().unwrap();
    let target_hwnd = options.target.resolve_target_window()?;
    let mut target_rect = RECT::default();
    let target_size = if unsafe { GetClientRect(target_hwnd, &mut target_rect) }.as_bool() {
        Some((
            (target_rect.right - target_rect.left) as u32,
            (target_rect.bottom - target_rect.top) as u32,
        ))
    } else {
        None
    };

    let (overlay_hwnd, overlay_window) = create_window(&event_loop, &options.title, target_size)?;

    let window_tracker = WindowTracker::new(overlay_hwnd, &options.target)?;

    let (mut platform, mut imgui) = create_imgui_context(&options)?;
    platform.attach_window(imgui.io_mut(), &overlay_window, HiDpiMode::Default);

    let mut imgui_fonts = FontAtlasBuilder::new();
    imgui_fonts.register_font(include_bytes!("../resources/Roboto-Regular.ttf"))?;
    imgui_fonts.register_font(include_bytes!("../resources/NotoSansTC-Regular.ttf"))?;
    imgui_fonts.register_font(include_bytes!("../resources/unifont-15.1.05.otf"))?;
    imgui_fonts.register_codepoints(1..255);

    let backend = std::env::var("OVERLAY_BACKEND")
        .unwrap_or_else(|_| "DIRECTX".to_string())
        .to_uppercase();
    let renderer: Box<dyn RenderBackend> = match backend.as_str() {
        "OPENGL" => {
            log::info!("Using OpenGL renderer");
            Box::new(OpenGLRenderBackend::new(&event_loop, &overlay_window)?)
        }
        "VULKAN" => {
            log::info!("Using Vulkan renderer");
            Box::new(VulkanRenderBackend::new(&overlay_window, &mut imgui)?)
        }
        _ => {
            log::info!("Using DirectX renderer");
            Box::new(unsafe { DirectXRenderBackend::new(&overlay_window, &mut imgui)? })
        }
    };

    Ok(System {
        event_loop,
        overlay_window,
        overlay_hwnd,

        imgui,
        imgui_fonts,
        imgui_register_fonts_callback: options.register_fonts_callback,

        platform,
        window_tracker,

        renderer,
    })
}

const PERF_RECORDS: usize = 2048;

impl System {
    pub fn main_loop<U, R>(self, mut update: U, mut render: R) -> i32
    where
        U: FnMut(&mut SystemRuntimeController) -> bool + 'static,
        R: FnMut(&imgui::Ui, &UnicodeTextRenderer) -> bool + 'static,
    {
        let System {
            event_loop,
            overlay_window: window,
            overlay_hwnd,

            imgui,
            imgui_fonts,
            imgui_register_fonts_callback,

            mut platform,
            window_tracker,

            mut renderer,
            ..
        } = self;

        let mut last_frame = Instant::now();

        let mut runtime_controller = SystemRuntimeController {
            hwnd: overlay_hwnd,

            imgui,
            imgui_fonts,

            active_tracker: ActiveTracker::new(overlay_hwnd),
            key_input_system: KeyboardInputSystem::new(),
            mouse_input_system: MouseInputSystem::new(overlay_hwnd),
            window_tracker,

            frame_count: 0,
            debug_overlay_shown: false,
        };

        let mut perf = PerfTracker::new(PERF_RECORDS);
        #[allow(deprecated)]
        let _ = event_loop.run(move |event, event_loop| {
            platform.handle_event(runtime_controller.imgui.io_mut(), &window, &event);

            match event {
                // New frame
                Event::NewEvents(_) => {
                    perf.begin();
                    let now = Instant::now();
                    runtime_controller
                        .imgui
                        .io_mut()
                        .update_delta_time(now - last_frame);
                    last_frame = now;
                }

                Event::AboutToWait => {
                    window.request_redraw();
                }

                Event::WindowEvent {
                    event: WindowEvent::RedrawRequested,
                    ..
                } => {
                    /* Update */
                    {
                        if !runtime_controller.update_state(&window) {
                            event_loop.exit();
                            return;
                        }

                        if !update(&mut runtime_controller) {
                            event_loop.exit();
                            return;
                        }

                        if runtime_controller.imgui_fonts.fetch_reset_flag_updated() {
                            let font_atlas = runtime_controller.imgui.fonts();
                            font_atlas.clear();

                            let (font_sources, _glyph_memory) =
                                runtime_controller.imgui_fonts.build_font_source(18.0);

                            font_atlas.add_font(&font_sources);
                            if let Some(user_callback) = &imgui_register_fonts_callback {
                                user_callback(font_atlas);
                            }

                            renderer.update_fonts_texture(&mut runtime_controller.imgui);
                        }

                        perf.mark("update");
                    }

                    /* Generate frame */
                    let draw_data = {
                        if let Err(error) =
                            platform.prepare_frame(runtime_controller.imgui.io_mut(), &window)
                        {
                            event_loop.exit();
                            log::error!("Platform implementation prepare_frame failed: {}", error);
                            return;
                        }

                        let ui = runtime_controller.imgui.frame();
                        let unicode_text =
                            UnicodeTextRenderer::new(ui, &mut runtime_controller.imgui_fonts);

                        let run = render(ui, &unicode_text);
                        if !run {
                            event_loop.exit();
                            return;
                        }
                        if runtime_controller.debug_overlay_shown {
                            ui.window("Render Debug")
                                .position([200.0, 200.0], imgui::Condition::FirstUseEver)
                                .size([400.0, 400.0], imgui::Condition::FirstUseEver)
                                .build(|| {
                                    ui.text(format!("FPS: {: >4.2}", ui.io().framerate));
                                    ui.same_line_with_pos(100.0);

                                    ui.text(format!(
                                        "Frame Time: {:.2}ms",
                                        ui.io().delta_time * 1000.0
                                    ));
                                    ui.same_line_with_pos(275.0);

                                    ui.text("History length:");
                                    ui.same_line();
                                    let mut history_length = perf.history_length();
                                    ui.set_next_item_width(75.0);
                                    if ui
                                        .input_scalar("##history_length", &mut history_length)
                                        .build()
                                    {
                                        perf.set_history_length(history_length);
                                    }
                                    perf.render(ui, ui.content_region_avail());
                                });
                        }
                        perf.mark("generate frame");

                        platform.prepare_render(ui, &window);
                        runtime_controller.imgui.render()
                    };

                    /* render */
                    renderer.render_frame(&mut perf, &window, draw_data);

                    runtime_controller.frame_rendered();
                    perf.finish("render");
                }
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => {
                    event_loop.exit();
                }
                _ => {}
            }
        });
        0
    }
}

pub struct SystemRuntimeController {
    pub hwnd: HWND,

    pub imgui: imgui::Context,
    pub imgui_fonts: FontAtlasBuilder,

    debug_overlay_shown: bool,

    active_tracker: ActiveTracker,
    mouse_input_system: MouseInputSystem,
    key_input_system: KeyboardInputSystem,

    window_tracker: WindowTracker,

    frame_count: u64,
}

impl SystemRuntimeController {
    fn update_state(&mut self, window: &Window) -> bool {
        self.mouse_input_system.update(window, self.imgui.io_mut());
        self.key_input_system.update(window, self.imgui.io_mut());
        self.active_tracker.update(self.imgui.io());
        if !self.window_tracker.update() {
            log::info!("Target window has been closed. Exiting overlay.");
            return false;
        }

        true
    }

    fn frame_rendered(&mut self) {
        self.frame_count += 1;
        if self.frame_count == 1 {
            /* initial frame */
            unsafe { ShowWindow(self.hwnd, SW_SHOWNOACTIVATE) };
            self.window_tracker.mark_force_update();
        }
    }

    pub fn toggle_screen_capture_visibility(&self, should_be_visible: bool) {
        unsafe {
            let (target_state, state_name) = if should_be_visible {
                (WDA_NONE, "normal")
            } else {
                (WDA_EXCLUDEFROMCAPTURE, "exclude from capture")
            };

            if !SetWindowDisplayAffinity(self.hwnd, target_state).as_bool() {
                log::warn!(
                    "{} '{}'.",
                    obfstr!("Failed to change overlay display affinity to"),
                    state_name
                );
            }
        }
    }

    pub fn toggle_debug_overlay(&mut self, visible: bool) {
        self.debug_overlay_shown = visible;
    }

    pub fn debug_overlay_shown(&self) -> bool {
        self.debug_overlay_shown
    }
}
