// On Windows, start as a window app (no extra black console window).
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod canvas;
mod combine;
mod device_ui;
mod devices;
mod fileicon;
mod formats;
mod gcode;
mod i18n;
mod imageproc;
mod icons;
mod import;
mod import_job;
mod material_lib;
mod measure;
mod laser;
mod lasertest;
mod lasertest_ui;
mod model;
mod preview;
mod offset;
mod panels;
mod prefs;
mod settings_ui;
mod sysmenu;
mod text;
mod theme;
mod update;
#[cfg(test)]
mod mechanics_check;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mut screenshot = None;
    let mut open_file = None;
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--screenshot" && i + 1 < args.len() {
            screenshot = Some(std::path::PathBuf::from(&args[i + 1]));
            i += 2;
            continue;
        }
        open_file = Some(std::path::PathBuf::from(&args[i]));
        i += 1;
    }
    if screenshot.is_none() {
        std::thread::spawn(fileicon::register);
    }
    let viewport = eframe::egui::ViewportBuilder::default()
        .with_title("RayEtch")
        .with_inner_size([1440.0, 860.0])
        .with_min_inner_size([900.0, 600.0])
        .with_icon(app_icon());
    // No title bar, to give the space to the program (as the person asked): on macOS the
    // window's content runs up to the top edge and only the three window buttons stay, drawn
    // over the toolbar; elsewhere the system frame goes and RayEtch draws its own buttons.
    // Opens filling the screen's free area, as the person asked (the original does this when it was
    // maximised last time: Core_MainWindow::ShowEvent 0x10071d984 → showMaximized). Not in --screenshot
    // runs, which keep the fixed size above so checks compare alike.
    let viewport = if screenshot.is_none() { viewport.with_maximized(true) } else { viewport };
    #[cfg(target_os = "macos")]
    let viewport = viewport.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false);
    #[cfg(not(target_os = "macos"))]
    let viewport = viewport.with_decorations(false);
    let first = renderer();
    let run = |renderer: eframe::Renderer, screenshot: Option<std::path::PathBuf>, open_file: Option<std::path::PathBuf>| {
        let options = eframe::NativeOptions { viewport: viewport.clone(), renderer, wgpu_options: wgpu_options(), ..Default::default() };
        eframe::run_native("RayEtch", options, Box::new(move |cc| Ok(Box::new(make_app(cc, screenshot, open_file)))))
    };
    match run(first, screenshot.clone(), open_file.clone()) {
        // no graphics card wgpu can use (an old PC without DirectX 12 / Vulkan, or only Windows'
        // software drawing): OpenGL instead, which such PCs have
        Err(e) if first == eframe::Renderer::Wgpu => {
            eprintln!("wgpu non disponibile ({e}): uso OpenGL");
            run(eframe::Renderer::Glow, screenshot, open_file)
        }
        r => r,
    }
}

/// wgpu takes a real graphics card only: Windows' software DirectX ("Microsoft Basic Render
/// Driver", WARP) and other drawing on the processor are refused, so an old PC falls back to
/// OpenGL instead of crawling (the person's 2011 PC). A discrete card comes first.
fn wgpu_options() -> eframe::egui_wgpu::WgpuConfiguration {
    use eframe::egui_wgpu::{WgpuSetup, wgpu};
    let mut cfg = eframe::egui_wgpu::WgpuConfiguration::default();
    if let WgpuSetup::CreateNew(ref mut new) = cfg.wgpu_setup {
        new.native_adapter_selector = Some(std::sync::Arc::new(|adapters: &[wgpu::Adapter], surface: Option<&wgpu::Surface<'_>>| {
            let rank = |t: wgpu::DeviceType| match t {
                wgpu::DeviceType::DiscreteGpu => 0,
                wgpu::DeviceType::IntegratedGpu => 1,
                wgpu::DeviceType::VirtualGpu => 2,
                wgpu::DeviceType::Other => 3,
                wgpu::DeviceType::Cpu => 9,
            };
            adapters
                .iter()
                .filter(|a| a.get_info().device_type != wgpu::DeviceType::Cpu)
                .filter(|a| !a.get_info().name.to_lowercase().contains("basic render"))
                .filter(|a| surface.is_none_or(|s| a.is_surface_supported(s)))
                .min_by_key(|a| rank(a.get_info().device_type))
                .cloned()
                .ok_or_else(|| "nessuna scheda video adatta (solo disegno software)".to_string())
        }));
    }
    cfg
}

fn make_app(cc: &eframe::CreationContext<'_>, screenshot: Option<std::path::PathBuf>, open_file: Option<std::path::PathBuf>) -> app::RayEtch {
    {
        {
            let mut a = app::RayEtch::new(cc, screenshot);
            if let Some(p) = open_file {
                if fileicon::is_project(&p) {
                    a.open_path_now(p);
                } else {
                    a.import_path(&p);
                }
            }
            // RAYETCH_OPEN=deselect (for testing): show the imported drawing unselected
            if std::env::var("RAYETCH_OPEN").is_ok_and(|v| v.contains("deselect")) {
                a.selection.clear();
            }
            // RAYETCH_OPEN=offset (for testing): open the Offset dialog on the imported drawing
            if std::env::var("RAYETCH_OPEN").is_ok_and(|v| v.contains("offset")) {
                a.open_offset();
            }
            // RAYETCH_OPEN=interval / material / testpreview (for testing): the laser test windows
            if std::env::var("RAYETCH_OPEN").is_ok_and(|v| v.contains("interval")) {
                a.open_interval_test();
            }
            if std::env::var("RAYETCH_OPEN").is_ok_and(|v| v.contains("material")) {
                a.open_material_test();
                // material,mtedit1: the cut-settings editor on the test's text setting
                if let Some(k) = std::env::var("RAYETCH_OPEN").ok().and_then(|v| v.split(',').find_map(|w| w.strip_prefix("mtedit").and_then(|n| n.parse::<usize>().ok()))) {
                    a.test_edit = Some(k);
                    a.editor_layer = Some(k);
                }
            }
            // RAYETCH_OPEN=itpreview (and itpreview,dither): the interval test's preview
            if std::env::var("RAYETCH_OPEN").is_ok_and(|v| v.contains("itpreview")) {
                let mut t = lasertest::IntervalTest::default();
                t.dithered = std::env::var("RAYETCH_OPEN").is_ok_and(|v| v.contains("dither"));
                let d = t.build(&a.doc);
                a.preview_test(d);
            }
            if std::env::var("RAYETCH_OPEN").is_ok_and(|v| v.contains("testpreview")) {
                let mut t = lasertest::MaterialTest::new(&a.doc.device);
                // testpreview,diodecut: the built-in "Diode - Cut test"
                if std::env::var("RAYETCH_OPEN").is_ok_and(|v| v.contains("diodecut")) {
                    t.preset(2);
                }
                // testpreview,preset<n>: built-in preset n (0 CO2 cut, 1 CO2 engrave, 2 diode cut, 3 diode engrave)
                if let Some(n) = std::env::var("RAYETCH_OPEN").ok().and_then(|v| v.split(',').find_map(|w| w.strip_prefix("preset").and_then(|n| n.parse::<usize>().ok()))) {
                    t.preset(n);
                }
                let d = t.build(&a.doc);
                a.preview_test(d);
            }
            // RAYETCH_OPEN=measure (for testing): ruler on the first shape's second piece
            if std::env::var("RAYETCH_OPEN").is_ok_and(|v| v.contains("measure")) {
                a.tool = app::Tool::Measure;
                a.measure.open = true;
                a.measure.shape = a.doc.shapes.first().map(|s| s.id);
                a.measure.prim = Some(1);
            }
            a
        }
    }
}

/// The window and Dock/taskbar icon, built into the program (assets/icon.png, made by
/// `cargo run --example make_icon`).
/// How the window is drawn. On Windows with wgpu (DirectX 12 or Vulkan): many Windows PCs
/// (integrated graphics, old drivers, virtual machines, Remote Desktop) only have a slow software
/// OpenGL, which made the program crawl there. macOS and Linux keep OpenGL, which works well
/// on them. RAYETCH_RENDERER=glow or =wgpu overrides it, should a computer draw badly.
fn renderer() -> eframe::Renderer {
    match std::env::var("RAYETCH_RENDERER").map(|v| v.to_ascii_lowercase()) {
        Ok(v) if v == "glow" || v == "opengl" => eframe::Renderer::Glow,
        Ok(v) if v == "wgpu" => eframe::Renderer::Wgpu,
        _ if cfg!(windows) => eframe::Renderer::Wgpu,
        _ => eframe::Renderer::Glow,
    }
}

fn app_icon() -> eframe::egui::IconData {
    let png = include_bytes!("../assets/icon.png");
    match image::load_from_memory(png) {
        Ok(img) => {
            let img = img.resize_exact(256, 256, image::imageops::FilterType::Triangle).to_rgba8();
            let (width, height) = img.dimensions();
            eframe::egui::IconData { rgba: img.into_raw(), width, height }
        }
        Err(_) => eframe::egui::IconData::default(),
    }
}
