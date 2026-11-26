// On Windows, start as a window app (no extra black console window).
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod barcode;
mod barcode_dicts;
mod trim;
mod vartext;
mod editops;
mod filemenu;
mod toolsmenu;
mod organize;
mod lasertools;
mod webtools;
mod webview;
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
    // a web tool's own window (Strumenti Web), started by the main window
    if args.len() > 2 && args[1] == "--webtool" {
        webview::run(&args[2]);
        return Ok(());
    }
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
        filemenu::backup_prefs();
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
    match run(first, screenshot, open_file) {
        // wgpu couldn't start (no DirectX 12 at all): start again on OpenGL. A program may
        // open its window system only once, so the new start is a fresh copy of RayEtch.
        Err(e) if first == eframe::Renderer::Wgpu && std::env::var_os("RAYETCH_RENDERER").is_none() => {
            eprintln!("wgpu non disponibile ({e}): uso OpenGL");
            let exe = std::env::current_exe().map_err(|_| e)?;
            let _ = std::process::Command::new(exe).args(&args[1..]).env("RAYETCH_RENDERER", "opengl").spawn();
            Ok(())
        }
        r => r,
    }
}

/// How wgpu picks its graphics card. On Windows it uses DirectX 12 only: the person's 2011
/// HP Pavilion dv7 has OpenGL and Vulkan drivers that draw garbage (broken colours, torn
/// triangles, unreadable text) or freeze, and its card has no DirectX 12 driver, so Windows'
/// own DirectX 12 software drawing (WARP, "Microsoft Basic Render Driver") is taken instead:
/// slower, but always right. A real DirectX 12 card comes first. Elsewhere a real card only;
/// drawing on the processor is refused there, as the system's OpenGL is the better fallback.
fn wgpu_options() -> eframe::egui_wgpu::WgpuConfiguration {
    use eframe::egui_wgpu::{WgpuSetup, wgpu};
    let mut cfg = eframe::egui_wgpu::WgpuConfiguration::default();
    if let WgpuSetup::CreateNew(ref mut new) = cfg.wgpu_setup {
        if cfg!(windows) {
            new.instance_descriptor.backends = wgpu::Backends::DX12;
        }
        new.native_adapter_selector = Some(std::sync::Arc::new(|adapters: &[wgpu::Adapter], surface: Option<&wgpu::Surface<'_>>| {
            let software = |a: &wgpu::Adapter| a.get_info().device_type == wgpu::DeviceType::Cpu || a.get_info().name.to_lowercase().contains("basic render");
            let rank = |a: &wgpu::Adapter| {
                if software(a) {
                    return 9;
                }
                match a.get_info().device_type {
                    wgpu::DeviceType::DiscreteGpu => 0,
                    wgpu::DeviceType::IntegratedGpu => 1,
                    wgpu::DeviceType::VirtualGpu => 2,
                    _ => 3,
                }
            };
            adapters
                .iter()
                .filter(|a| cfg!(windows) || !software(a))
                .filter(|a| surface.is_none_or(|s| a.is_surface_supported(s)))
                .min_by_key(|a| rank(a))
                .cloned()
                .ok_or_else(|| "nessuna scheda video adatta".to_string())
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
            // RAYETCH_OPEN=barcode (for testing): a QR code made by a drag, its window open
            if std::env::var("RAYETCH_OPEN").is_ok_and(|v| v.contains("barcode")) {
                a.tool = app::Tool::Barcode;
                barcode::create(&mut a, [100.0, 200.0], 60.0, 60.0);
                if let Some(d) = a.barcode_dialog.as_mut() {
                    d.spec.kind = barcode::DEFAULT_KIND;
                    d.spec.content = "https://example.com".into();
                    // barcode,serial: a Code 128 serial number, counter at 41, offset 1
                    if std::env::var("RAYETCH_OPEN").is_ok_and(|v| v.contains("serial")) {
                        d.spec.kind = 12;
                        d.spec.content = "SN-0000d".into();
                        d.spec.eval = vartext::EVAL_SERIAL;
                        d.spec.var_offset = 1;
                        a.doc.vars.current = 41;
                        vartext::set(&a.doc.vars);
                    }
                }
                // barcode,vartab: the Testo variabile tab
                if std::env::var("RAYETCH_OPEN").is_ok_and(|v| v.contains("vartab")) {
                    a.right_tab = app::RightTab::VarText;
                }
                // barcode,closed: the code finished, its window shut
                if std::env::var("RAYETCH_OPEN").is_ok_and(|v| v.contains("closed")) {
                    if let Some(d) = a.barcode_dialog.take() {
                        barcode::apply(&mut a, d.shape, &d.spec);
                    }
                }
            }
            // RAYETCH_OPEN=nest (for testing): scattered shapes nested by their real outlines
            if std::env::var("RAYETCH_OPEN").is_ok_and(|v| v.contains("nest")) {
                use model::{Geometry, Shape};
                let mut ids = Vec::new();
                for k in 0..14 {
                    let (x, y) = (30.0 + (k * 37 % 300) as f64, 60.0 + (k * 53 % 280) as f64);
                    let g = match k % 3 {
                        0 => Geometry::Star { points: 5, rx: 30.0, ry: 30.0, inner: 0.4, middle: None },
                        1 => Geometry::Ellipse { rx: 25.0, ry: 15.0 },
                        _ => Geometry::Rect { w: 70.0, h: 20.0, radius: 0.0 },
                    };
                    ids.push(a.doc.add_shape(Shape::new(0, g, 0, x, y)));
                }
                a.selection = ids;
                a.nest(2.0, 0.0, 4);
                // nest,check: report any two nested shapes whose outlines overlap
                if std::env::var("RAYETCH_OPEN").is_ok_and(|v| v.contains("check")) {
                    let n = a.doc.shapes.len();
                    for i in 0..n {
                        for j in i + 1..n {
                            let r = combine::combine(&a.doc.shapes[i].world_paths(), &a.doc.shapes[j].world_paths(), combine::CombineOp::Intersect);
                            if !r.is_empty() {
                                eprintln!("overlap {i} {j}");
                            }
                        }
                    }
                }
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
/// How the window is drawn. Windows: wgpu on DirectX 12 (see wgpu_options), because OpenGL
/// drew a garbled, frozen window on the person's 2011 PC; if wgpu can't start at all, OpenGL.
/// macOS and Linux: OpenGL (glow), which runs smoothly on the same PC under Linux Mint.
/// RAYETCH_RENDERER=wgpu or =opengl, or "wgpu"/"opengl" in renderer.json beside the
/// settings, picks one by hand.
fn renderer() -> eframe::Renderer {
    let choice = std::env::var("RAYETCH_RENDERER").ok().or_else(|| crate::i18n::load_json::<String>("renderer.json"));
    match choice.map(|v| v.to_ascii_lowercase()).as_deref() {
        Some("wgpu" | "directx" | "dx12") => eframe::Renderer::Wgpu,
        Some("opengl" | "gl" | "glow") => eframe::Renderer::Glow,
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
