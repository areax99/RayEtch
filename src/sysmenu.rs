//! macOS system menu bar (the original uses the system menu, not one inside the window).

use crate::i18n::tr;
use crate::app::{RayEtch, RightTab};
use eframe::egui;
use muda::{AboutMetadata, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu, accelerator::{Accelerator, Code, Modifiers}};
use std::sync::{Mutex, Once, OnceLock};

/// Menu clicks waiting for the next frame. muda accepts its event handler only once per run
/// (a OnceLock), so one permanent handler feeds this queue for every menu ever installed.
static QUEUE: Mutex<Vec<String>> = Mutex::new(Vec::new());
static REPAINT: OnceLock<egui::Context> = OnceLock::new();
static HANDLER: Once = Once::new();

pub struct SystemMenu {
    _menu: Menu,
}

thread_local! {
    /// Every menu-bar item with its last enabled state, so it can be greyed out when it can't run.
    static ITEMS: std::cell::RefCell<Vec<(MenuItem, bool)>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn item(id: &str, text: &str, acc: Option<Accelerator>) -> MenuItem {
    let m = MenuItem::with_id(id, text, true, acc);
    ITEMS.with(|v| v.borrow_mut().push((m.clone(), true)));
    m
}

impl RayEtch {
    /// Whether an action can run now; when it can't, its menu item and button are greyed out.
    /// Like the original (MainWindow::UpdateActionsStatus 0x100c39980 enables each action from
    /// Core_Checks::UpdateSelectionStatus): selection actions need a selection, arranging needs
    /// 2 objects (align, group, same size, move together, combine: exactly 2), distribute 3
    /// (Core_Arrange rules), paste a copied shape, laser commands a connected laser.
    pub fn can(&self, id: &str) -> bool {
        let n = self.selection.len();
        let items = || self.selection_items().len();
        let has_shapes = !self.doc.shapes.is_empty();
        // UpdateActionsStatus 0x100c39980: every selection/clipboard action is also off while the current
        // tool is busy (virtual call +0x90 on the active tool): a drag, a shape being drawn, an open polyline.
        let busy = !matches!(self.drag, crate::app::Drag::None) || !self.line_pts.is_empty();
        let edit = matches!(
            id,
            "cut" | "copy" | "paste" | "delete" | "duplicate" | "mirror_h" | "mirror_v" | "rot_cw" | "rot_ccw" | "center_page" | "offset"
                | "group" | "ungroup" | "same_w" | "same_h" | "together_h" | "together_v" | "combine" | "combine_assist"
                | "dist_h_spaced" | "dist_v_spaced" | "dist_h_centered" | "dist_v_centered" | "undo" | "redo"
        ) || id.starts_with("align_");
        if busy && edit {
            return false;
        }
        match id {
            "cut" | "copy" | "delete" | "duplicate" | "mirror_h" | "mirror_v" | "rot_cw" | "rot_ccw" | "center_page" | "offset" => n > 0,
            "paste" => !self.clipboard.is_empty(),
            "undo" => !self.undo.is_empty(),
            "redo" => !self.redo.is_empty(),
            "select_all" | "preview" | "export_gcode" | "export_svg" | "export" | "print_black" | "print_color" => has_shapes,
            "save_capture" | "none" => false,
            "invert_selection" | "delete_dups" | "select_open" | "select_open_fill" | "select_layer" | "flash_layer" | "select_circles" => has_shapes,
            "paste_in_place" => !self.clipboard.is_empty() && !busy,
            "undo_history" => !self.undo.is_empty() || !self.redo.is_empty(),
            "convert_path" | "convert_bitmap" | "cut_path" | "close_path" | "close_tolerance" | "reverse" | "optimize" => n > 0 && !busy,
            "auto_join" => n > 0 && !busy,
            "select_contained" | "select_smaller" => n == 1,
            "auto_group" => has_shapes,
            "mirror_line" | "copy_along_path" => items() >= 2,
            "two_point" | "nest" | "rubber_band" | "break_apart" | "grid_array" | "circular_array" | "order_forward" | "order_backward" | "order_front"
            | "order_back" | "lock" | "unlock_shapes" | "dock_left" | "dock_right" | "dock_top" | "dock_bottom" => n > 0 && !busy,
            other if other.starts_with("page_") => n > 0 && !busy,
            other if other.starts_with("laser_sel_") => n > 0 && self.connected.is_some(),
            "weld" | "cut_shapes" | "text_on_path" | "resize_slots" | "warp4" | "warp16" => n > 0 && !busy,
            "bool_union" | "bool_subtract" | "bool_intersect" => items() == 2 && !busy,
            "img_crop" => self.doc.shapes.iter().any(|s| self.selection.contains(&s.id) && matches!(s.geom, crate::model::Geometry::Image { .. })) && n >= 2,
            "img_trace" | "img_adjust" | "img_replace" | "img_save" => self.doc.shapes.iter().any(|s| self.selection.contains(&s.id) && matches!(s.geom, crate::model::Geometry::Image { .. })),
            "img_mask" => self.doc.shapes.iter().any(|s| self.selection.contains(&s.id) && matches!(s.geom, crate::model::Geometry::Image { .. })) && n >= 2,
            "frame" | "frame_outline" | "start" => has_shapes && self.connected.is_some(),
            "pause" | "stop" => self.job_progress.is_some() || self.paused,
            "group" | "same_w" | "same_h" | "together_h" | "together_v" => items() >= 2,
            "combine_assist" | "combine" => items() == 2,
            "dist_h_spaced" | "dist_v_spaced" | "dist_h_centered" | "dist_v_centered" => items() >= 3,
            "ungroup" => self.doc.shapes.iter().any(|s| self.selection.contains(&s.id) && !s.groups.is_empty()),
            "pnc_wizard" => self.connected.is_some(),
            "pnc_first" => self.connected.is_some() && items() == 1,
            "pnc_second" => self.connected.is_some() && items() == 1 && self.pnc.a.is_some(),
            "pnc_align" | "pnc_align_noscale" => self.pnc.a.is_some() && self.pnc.b.is_some(),
            "pnc_reset" => self.pnc.a.is_some() || self.pnc.b.is_some() || self.pnc.fit.is_some(),
            "taper" => self.doc.device.rotary.enabled && n > 0 && !busy,
            "home" | "unlock" | "fire_on" | "fire_off" | "run_gcode" | "goto_origin" => self.connected.is_some(),
            other if other.starts_with("align_") => items() >= 2,
            _ => true,
        }
    }
}

/// Grey out the menu-bar items that can't run now (only the ones whose state changed).
fn refresh_enabled(app: &RayEtch) {
    ITEMS.with(|v| {
        for (m, last) in v.borrow_mut().iter_mut() {
            let on = app.can(&m.id().0);
            if on != *last {
                m.set_enabled(on);
                *last = on;
            }
        }
    });
}

fn cmd(code: Code) -> Option<Accelerator> {
    Some(Accelerator::new(Modifiers::META, code))
}

/// A submenu from shared menu rows (the same rows the window's own menu draws).
#[allow(dead_code)]
fn rows_submenu(title: &str, rows: Vec<crate::panels::MenuRow>) -> Option<Submenu> {
    let sub = Submenu::new(title, true);
    for row in rows {
        match row {
            None => sub.append(&PredefinedMenuItem::separator()).ok()?,
            Some((id, label)) => {
                let acc = (id == "preview").then(|| Accelerator::new(Modifiers::ALT, Code::KeyP));
                sub.append(&item(&id, label, acc)).ok()?
            }
        }
    }
    Some(sub)
}

/// Builds the menu and installs it in the macOS menu bar. Returns None on other systems.
pub fn install(ctx: &egui::Context) -> Option<SystemMenu> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    ITEMS.with(|v| v.borrow_mut().clear());
    let menu = Menu::new();
    let sep = PredefinedMenuItem::separator;

    let app_menu = Submenu::with_items(
        "RayEtch",
        true,
        &[
            &PredefinedMenuItem::about(
                Some(tr("Informazioni su RayEtch")),
                Some(AboutMetadata {
                    name: Some("RayEtch".into()),
                    version: Some("1.0".into()),
                    comments: Some(tr("Software libero per laser, licenza Apache 2.0. Nessuna attivazione richiesta.").into()),
                    ..Default::default()
                }),
            ),
            &sep(),
            &PredefinedMenuItem::services(Some(tr("Servizi"))),
            &sep(),
            &PredefinedMenuItem::hide(Some(tr("Nascondi RayEtch"))),
            &PredefinedMenuItem::hide_others(Some(tr("Nascondi altre"))),
            &PredefinedMenuItem::show_all(Some(tr("Mostra tutte"))),
            &sep(),
            &item("quit", tr("Esci da RayEtch"), cmd(Code::KeyQ)),
        ],
    )
    .ok()?;

    // the original's File menu (reference pictures 26-27), from the same rows as the window's menu
    let file = Submenu::new(tr("File"), true);
    fn add_rows(parent: &Submenu, rows: Vec<crate::filemenu::FileRow>) -> Option<()> {
        use crate::filemenu::FileRow;
        let shift_meta = Modifiers::META | Modifiers::SHIFT;
        for row in rows {
            match row {
                FileRow::Sep => parent.append(&PredefinedMenuItem::separator()).ok()?,
                FileRow::Item(id, label, _, _) => {
                    let acc = match id.as_str() {
                        "new" => cmd(Code::KeyN),
                        "open" => cmd(Code::KeyO),
                        "import" => cmd(Code::KeyI),
                        "save" => cmd(Code::KeyS),
                        "save_as" => Some(Accelerator::new(shift_meta, Code::KeyS)),
                        "notes" => Some(Accelerator::new(Modifiers::META | Modifiers::ALT, Code::KeyN)),
                        "export" => Some(Accelerator::new(Modifiers::ALT, Code::KeyX)),
                        "print_black" => cmd(Code::KeyP),
                        "print_color" => Some(Accelerator::new(shift_meta, Code::KeyP)),
                        _ => None,
                    };
                    parent.append(&item(&id, &label, acc)).ok()?
                }
                FileRow::Sub(title, items) => {
                    let sub = Submenu::new(title, true);
                    add_rows(&sub, items)?;
                    parent.append(&sub).ok()?
                }
            }
        }
        Some(())
    }
    add_rows(&file, crate::filemenu::rows(true, true))?;

    // The original's Modifica menu (reference picture 25). Editing keys (⌘Z, ⌘C, ⌘V…) stay with
    // the window, so they keep working inside text fields: they're shown in the label.
    let edit = Submenu::new(tr("Modifica"), true);
    for row in crate::panels::edit_menu_rows() {
        match row {
            crate::panels::EditRow::Sep => edit.append(&sep()).ok()?,
            crate::panels::EditRow::Item(id, label, keys) => {
                let acc = (id == "settings").then(|| Accelerator::new(Modifiers::META, Code::Comma));
                let text = if keys.is_empty() || id == "settings" { label.to_string() } else { format!("{label}\t{keys}") };
                edit.append(&item(id, &text, acc)).ok()?
            }
            crate::panels::EditRow::Sub(title, items) => {
                let sub = Submenu::new(title, true);
                for (id, label) in items {
                    sub.append(&item(id, label, None)).ok()?;
                }
                edit.append(&sub).ok()?
            }
        }
    }

    // the original's Strumenti menu (reference picture 28), from the same rows as the window's
    let tools = Submenu::new(tr("Strumenti"), true);
    add_rows(&tools, crate::toolsmenu::rows(crate::app::Tool::Select, true, true))?;

    // the original's Organizza menu (reference pictures 29-31)
    let arrange = Submenu::new(tr("Organizza"), true);
    add_rows(&arrange, crate::organize::rows(true, true))?;

    // the original's Strumenti laser menu (reference picture 32); keys shown beside the labels
    let laser = Submenu::new(tr("Strumenti laser"), true);
    fn add_keyed(parent: &Submenu, rows: Vec<crate::filemenu::FileRow>) -> Option<()> {
        use crate::filemenu::FileRow;
        for row in rows {
            match row {
                FileRow::Sep => parent.append(&PredefinedMenuItem::separator()).ok()?,
                FileRow::Item(id, label, keys, _) => {
                    let text = if keys.is_empty() { label } else { format!("{label}\t{keys}") };
                    parent.append(&item(&id, &text, None)).ok()?
                }
                FileRow::Sub(title, items) => {
                    let sub = Submenu::new(title, true);
                    add_keyed(&sub, items)?;
                    parent.append(&sub).ok()?
                }
            }
        }
        Some(())
    }
    add_keyed(&laser, crate::lasertools::rows(&|_| true))?;

    // the original's Strumenti Web menu (reference picture 33)
    let web = Submenu::new(tr("Strumenti Web"), true);
    add_rows(&web, crate::webtools::rows())?;

    let theme_menu = Submenu::new(tr("Tema"), true);
    add_rows(&theme_menu, crate::filemenu::theme_rows(crate::prefs::with(|p| p.theme)))?;
    let window = Submenu::with_items(
        tr("Finestra"),
        true,
        &[
            &item("fit", tr("Adatta alla pagina"), None),
            &item("zoom_in", tr("Ingrandisci"), None),
            &item("zoom_out", tr("Riduci"), None),
            &sep(),
            &item("tab_laser", tr("Laser"), None),
            &item("tab_move", tr("Sposta"), None),
            &item("tab_console", tr("Console"), None),
            &item("tab_shape", tr("Proprietà forma"), None),
            &sep(),
            &item("reset_layout", tr("Reimposta disposizione predefinita"), None),
            &sep(),
            &theme_menu,
            &item("settings_win", tr("Impostazioni"), None),
            &sep(),
            &PredefinedMenuItem::minimize(Some(tr("Riduci a icona"))),
            &PredefinedMenuItem::fullscreen(Some(tr("Schermo intero"))),
        ],
    )
    .ok()?;

    let help = Submenu::with_items(tr("Guida"), true, &[
            &item("updates", tr("Aggiornamenti…"), None),
            &sep(),
            &item("report_bug", tr("Segnala un problema…"), None),
            &item("github", tr("RayEtch su GitHub"), None),
            &sep(),
            &item("about", tr("Informazioni su RayEtch"), None),
        ]).ok()?;

    menu.append_items(&[&app_menu, &file, &edit, &tools, &arrange, &laser, &web, &window, &help]).ok()?;
    #[cfg(target_os = "macos")]
    {
        menu.init_for_nsapp();
        window.set_as_windows_menu_for_nsapp();
    }

    let _ = REPAINT.set(ctx.clone());
    HANDLER.call_once(|| {
        MenuEvent::set_event_handler(Some(|e: MenuEvent| {
            if let Ok(mut q) = QUEUE.lock() {
                q.push(e.id.0.clone());
            }
            if let Some(c) = REPAINT.get() {
                c.request_repaint();
            }
        }));
    });
    Some(SystemMenu { _menu: menu })
}

/// Run the actions chosen in the system menu since the last frame.
pub fn handle(app: &mut RayEtch, ctx: &egui::Context) {
    if app.system_menu.is_none() {
        return;
    }
    let ids: Vec<String> = QUEUE.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
    for id in ids {
        if app.can(&id) {
            run(app, ctx, &id);
        }
    }
    refresh_enabled(app);
}

/// Run one menu action by its id (from the menu bar or the window's menu).
pub fn run(app: &mut RayEtch, ctx: &egui::Context, id: &str) {
    {
        if let Some(t) = id.strip_prefix("tool_").and_then(|n| n.parse::<usize>().ok()).and_then(|n| crate::panels::TOOLBAR_TOOLS.get(n)) {
            app.tool = *t;
            app.line_pts.clear();
            if crate::app::SHAPE_TOOLS.contains(t) {
                app.shape_tool = *t;
            }
            ctx.request_repaint();
            return;
        }
        if crate::webtools::run(app, id) {
            return;
        }
        match id {
            "new" => app.new_doc(ctx),
            "quit" => app.request_close(crate::app::PendingClose::Quit, ctx),
            "open" => app.open_dialog(ctx),
            "save" => app.save(false),
            "save_as" => app.save(true),
            "import" => app.import_dialog(),
            "export_gcode" => app.save_gcode(),
            "export_svg" => app.export_svg(),
            "new_window" => app.new_window(),
            "notes" => app.show_notes = true,
            "save_template" => app.save_template(),
            "export" => app.export_drawing(),
            "prefs_import" => app.import_prefs(ctx, false),
            "prefs_export" => app.export_prefs(),
            "prefs_folder" => {
                if let Some(d) = crate::i18n::settings_file("prefs.json").and_then(|p| p.parent().map(|d| d.to_path_buf())) {
                    crate::filemenu::open_folder(&d);
                }
            }
            "prefs_backup" => app.import_prefs(ctx, true),
            "shortcuts" => app.show_shortcuts = true,
            "bundle_import" => app.import_bundle(ctx),
            "bundle_export" => app.export_bundle(),
            "print_black" => app.print(false),
            "print_color" => app.print(true),
            "recent_prj_clear" => {
                crate::i18n::save_json("recent_projects.json", &Vec::<String>::new());
                app.menu_dirty = true;
            }
            "recent_tpl_clear" => {
                crate::i18n::save_json("recent_templates.json", &Vec::<String>::new());
                app.menu_dirty = true;
            }
            other if other.starts_with("recent_prj_") || other.starts_with("recent_tpl_") => {
                if let Some(i) = other.rsplit('_').next().and_then(|n| n.parse::<usize>().ok()) {
                    app.open_recent(other.starts_with("recent_tpl_"), i, ctx);
                }
            }
            "device" => app.show_device = true,
            "undo" => app.do_undo(),
            "undo_history" => app.show_undo_history = true,
            "invert_selection" => app.invert_selection(),
            "paste_in_place" => app.paste_in_place(),
            "convert_path" => app.convert_to_path(),
            "convert_bitmap" => app.convert_to_bitmap(),
            "cut_path" => app.generate_cut_path(),
            "close_path" => app.close_paths(),
            "close_tolerance" => app.edit_dialog = Some(crate::editops::EditDialog::CloseTolerance { tol: 0.1 }),
            "auto_join" => app.auto_join(),
            "optimize" => app.edit_dialog = Some(crate::editops::EditDialog::Optimize { simplify: 0.025, join: 0.05, remove_dups: true }),
            "delete_dups" => app.delete_duplicates(),
            "reverse" => app.reverse_direction(),
            "select_open" => app.select_open_shapes(),
            "select_open_fill" => app.select_open_fill_shapes(),
            "select_layer" => app.select_layer_shapes(),
            "flash_layer" => app.flash_layer(ctx.input(|i| i.time)),
            "select_contained" => app.select_contained(),
            "select_smaller" => app.select_smaller(),
            "select_circles" => app.edit_dialog = Some(crate::editops::EditDialog::Circles { by_size: false, min: 0.0, max: 100.0 }),
            "img_trace" => app.open_trace(),
            "img_adjust" => app.open_adjust(),
            "img_replace" => app.replace_image(),
            "img_mask" => app.apply_mask(),
            "img_save" => app.save_processed_bitmap(),
            "devices" => app.open_devices(),
            "pnc_wizard" => app.pnc.wizard = true,
            "pnc_first" => app.pnc_set_target(false),
            "pnc_second" => app.pnc_set_target(true),
            "pnc_align" => app.pnc_align(true),
            "pnc_align_noscale" => app.pnc_align(false),
            "pnc_reset" => app.pnc_reset(),
            "cameras" => app.cameras = Some(crate::lasertools::Cameras::scan(ctx)),
            "rotary" => app.rotary_dialog = Some(app.doc.device.rotary.clone()),
            "taper" => {
                let d = app.doc.device.rotary.object_diameter;
                app.taper_dialog = Some(crate::lasertools::TaperDialog { left: d, right: d });
            }
            "auto_group" => app.auto_group(),
            "mirror_line" => app.mirror_across_line(),
            "two_point" => app.start_two_point(),
            "nest" => app.arrange_dialog = Some(crate::organize::ArrangeDialog::nest_default()),
            "svgnest" => {
                let _ = std::process::Command::new(if cfg!(target_os = "macos") { "open" } else if cfg!(windows) { "explorer" } else { "xdg-open" })
                    .arg("https://svgnest.com")
                    .spawn();
            }
            "dock_left" => app.dock(0),
            "dock_right" => app.dock(1),
            "dock_top" => app.dock(2),
            "dock_bottom" => app.dock(3),
            other if other.starts_with("page_") => app.move_to_page(other),
            other if other.starts_with("laser_sel_") => app.laser_to_selection(other),
            "grid_array" => app.arrange_dialog = Some(crate::organize::ArrangeDialog::grid_default()),
            "circular_array" => app.arrange_dialog = Some(crate::organize::ArrangeDialog::circular_default(app)),
            "copy_along_path" => app.arrange_dialog = Some(crate::organize::ArrangeDialog::AlongPath { count: 5, rotate: true }),
            "rubber_band" => app.rubber_band(),
            "break_apart" => app.break_apart(),
            "order_forward" => app.reorder(0),
            "order_backward" => app.reorder(1),
            "order_front" => app.reorder(2),
            "order_back" => app.reorder(3),
            "lock" => app.set_locked(true),
            "unlock_shapes" => app.set_locked(false),
            "tool_nodes" => {
                app.node_edit = Default::default();
                app.tool = crate::app::Tool::Nodes;
            }
            "weld" => app.weld(),
            "bool_union" => app.combine_selection(crate::combine::CombineOp::Union),
            "bool_subtract" => app.combine_selection(crate::combine::CombineOp::Subtract),
            "bool_intersect" => app.combine_selection(crate::combine::CombineOp::Intersect),
            "cut_shapes" => app.cut_shapes(),
            "trace_files" => app.trace_files(),
            "text_on_path" => app.text_on_path(),
            "img_crop" => app.crop_image(),
            "resize_slots" => app.slot_dialog = Some(crate::toolsmenu::SlotDialog { width: 3.0, new_width: 3.2, tol: 0.1 }),
            "warp4" => app.start_warp(2),
            "warp16" => app.start_warp(4),
            "tangent_circle" => {
                app.tangent = Default::default();
                app.tool = crate::app::Tool::Tangent;
            }
            "redo" => app.do_redo(),
            "cut" => app.cut(),
            "copy" => app.copy(),
            "paste" => app.paste(5.0),
            "duplicate" => app.duplicate(),
            "delete" => app.delete_selection(),
            "select_all" => app.select_all(),
            "combine_assist" => app.open_combine_assistant(),
            "fire_on" => {
                let d = &app.doc.device;
                let fire = crate::gcode::fire_on_lines(d.fire_power, d.s_max, d.dynamic_power);
                app.send_lines(fire);
            }
            "fire_off" => app.send_lines(crate::gcode::fire_off_lines()),
            "mirror_h" => app.mirror(true),
            "mirror_v" => app.mirror(false),
            "rot_cw" => app.rotate_sel(-90.0),
            "rot_ccw" => app.rotate_sel(90.0),
            "center_page" => app.center_on_page(),
            "group" => app.group(),
            "ungroup" => app.ungroup(),
            "dist_h_spaced" => app.distribute(0, true),
            "dist_v_spaced" => app.distribute(1, true),
            "dist_h_centered" => app.distribute(0, false),
            "dist_v_centered" => app.distribute(1, false),
            "together_h" => app.move_together(0),
            "together_v" => app.move_together(1),
            "offset" => app.open_offset(),
            "interval_test" => app.open_interval_test(),
            "material_test" => app.open_material_test(),
            "same_w" => app.make_same_size(0, ctx.input(|i| i.modifiers.shift)),
            "same_h" => app.make_same_size(1, ctx.input(|i| i.modifiers.shift)),
            "preview" => app.open_preview(),
            "frame" => app.frame_job(),
            "frame_outline" => app.frame_outline_job(),
            "home" => app.send_lines(vec![crate::gcode::HOME_CMD.into()]),
            "unlock" => app.send_lines(vec!["$X".into()]),
            "fit" => app.view.fit_pending = true,
            "zoom_in" => crate::canvas::zoom_relative(app, crate::canvas::ZOOM_IN_FACTOR),
            "zoom_out" => crate::canvas::zoom_relative(app, crate::canvas::ZOOM_OUT_FACTOR),
            "tab_laser" => app.right_tab = RightTab::Laser,
            "tab_move" => app.right_tab = RightTab::Move,
            "tab_console" => app.right_tab = RightTab::Console,
            "tab_shape" => app.right_tab = RightTab::Shape,
            "reset_layout" => app.reset_layout = true,
            "about" => app.show_about = true,
            "updates" => {
                app.show_updates = true;
                app.updates.start(ctx, false);
            }
            "settings" => app.show_settings = true,
            "lang_auto" => app.set_language(None, ctx),
            "settings_win" => app.show_settings = true,
            // Guida: where to report bugs and the project's repository
            "report_bug" => app.bug_report = Some(crate::bugreport::BugReport::new()),
            "github" => crate::webtools::open_url("https://github.com/areax99/RayEtch"),
            "theme_system" | "theme_light" | "theme_dark" => {
                use crate::prefs::AppTheme;
                let mut p = app.prefs.clone();
                p.theme = match id {
                    "theme_light" => AppTheme::Light,
                    "theme_dark" => AppTheme::Dark,
                    _ => AppTheme::System,
                };
                if p.theme != AppTheme::System {
                    p.dark_background = p.theme == AppTheme::Dark;
                }
                app.apply_prefs(p, ctx);
                app.prefs.save();
                app.menu_dirty = true;
            }
            other if other.starts_with("lang_") => {
                app.set_language(crate::i18n::Lang::from_code(&other[5..]), ctx);
            }
            other => {
                if let Some(n) = other.strip_prefix("align_").and_then(|n| n.parse::<u8>().ok()) {
                    app.align(n);
                }
            }
        }
        ctx.request_repaint();
    }
}
