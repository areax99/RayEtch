//! The Strumenti laser menu (reference picture 32): Stampa e taglia, Gestisci telecamere,
//! Impostazione rotazione, the two tests, Distorsione conica, Gestisci dispositivi and
//! Impostazioni periferica.
//!
//! Stampa e taglia follows the original's MainWindow handlers: Set_First_Target 0x100c84e68 and
//! Set_Second_Target 0x100c850e0 (PnCController::SetTargetA/B: the one selected shape and the laser's
//! queried position), Align_Output_to_Targets 0x100c85454 (with or without scaling, then
//! UpdateOutputTransform), ClearPrintAndCut 0x100c4b860. Impostazione rotazione keeps the
//! original's GantryRotaryPrefs defaults (ctor 0x1005d78b0: roller, Y axis, 360 mm per turn,
//! 50 mm diameters). Distorsione conica opens TaperWarpDialog on the selection (0x100c89cc0).

use crate::app::RayEtch;
use crate::filemenu::FileRow;
use crate::i18n::{tr, trf};
use crate::model::{BBox, Geometry, P2, Polyline};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

/// The menu's rows; `can` says which can run now.
pub fn rows(can: &dyn Fn(&str) -> bool) -> Vec<FileRow> {
    let mac = cfg!(target_os = "macos");
    let alt = if mac { "⌥" } else { "Alt+" };
    let k = |a: &str, b: &str| -> &'static str { Box::leak(format!("{a}{b}").into_boxed_str()) };
    let it = |id: &str, label: &str, keys: &'static str| FileRow::Item(id.into(), label.into(), keys, can(id));
    vec![
        FileRow::Sub(
            tr("Stampa e taglia").into(),
            vec![
                it("pnc_wizard", tr("Avvia la procedura guidata"), k(alt, "0")),
                it("pnc_first", tr("Imposta prima posizione target"), k(alt, "1")),
                it("pnc_second", tr("Imposta seconda posizione target"), k(alt, "2")),
                it("pnc_align", tr("Allinea output ai target"), k(alt, "3")),
                it("pnc_align_noscale", tr("Allinea l'output agli obiettivi (senza scalare)"), k(alt, "4")),
                FileRow::Sep,
                it("pnc_reset", tr("Reimposta stampa e taglia"), ""),
            ],
        ),
        FileRow::Sep,
        it("cameras", tr("Gestisci telecamere"), ""),
        FileRow::Sep,
        it("rotary", tr("Impostazione rotazione"), if mac { "⇧⌘R" } else { "Ctrl+Maiusc+R" }),
        FileRow::Sep,
        it("interval_test", tr("Test di intervallo"), ""),
        it("material_test", tr("Test del materiale"), ""),
        it("taper", tr("Distorsione conica"), ""),
        FileRow::Sep,
        it("devices", tr("Gestisci dispositivi"), if mac { "⌥⇧D" } else { "Alt+Maiusc+D" }),
        it("device", tr("Impostazioni periferica"), ""),
    ]
}

// ------------------------------------------------------------------ Stampa e taglia

/// One target: where it is in the design and where the laser found it (both in design millimetres).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    pub design: P2,
    pub laser: P2,
}

#[derive(Clone, Debug, Default)]
pub struct PrintCut {
    pub a: Option<Target>,
    pub b: Option<Target>,
    /// the fit in use: Some(true) scales, Some(false) only turns and moves
    pub fit: Option<bool>,
    pub wizard: bool,
}

/// Targets closer than this can't give a direction (the original's "Insufficient separation").
const MIN_SEPARATION: f64 = 1.0;

impl PrintCut {
    /// Where a design point goes once aligned: the two design targets' midpoint onto the laser
    /// targets' midpoint, turned by the angle between the two lines, scaled by their length ratio
    /// when `scale` (otherwise kept at 1:1).
    pub fn map(&self, p: P2) -> P2 {
        let (Some(a), Some(b), Some(scale)) = (self.a, self.b, self.fit) else { return p };
        let dv = [b.design[0] - a.design[0], b.design[1] - a.design[1]];
        let lv = [b.laser[0] - a.laser[0], b.laser[1] - a.laser[1]];
        let (dl, ll) = (dv[0].hypot(dv[1]), lv[0].hypot(lv[1]));
        if dl < 1e-9 || ll < 1e-9 {
            return p;
        }
        let ang = lv[1].atan2(lv[0]) - dv[1].atan2(dv[0]);
        let s = if scale { ll / dl } else { 1.0 };
        let (c, sn) = (ang.cos() * s, ang.sin() * s);
        let dm = [(a.design[0] + b.design[0]) / 2.0, (a.design[1] + b.design[1]) / 2.0];
        let lm = [(a.laser[0] + b.laser[0]) / 2.0, (a.laser[1] + b.laser[1]) / 2.0];
        let (x, y) = (p[0] - dm[0], p[1] - dm[1]);
        [lm[0] + c * x - sn * y, lm[1] + sn * x + c * y]
    }
}

/// Machine position → design position (the inverse of the absolute-coordinates output: the same
/// flips for the origin corner).
fn machine_to_design(app: &RayEtch, m: P2) -> P2 {
    use crate::model::OriginCorner::*;
    let d = &app.doc.device;
    let x = match d.origin {
        FrontRight | RearRight => d.width - m[0],
        _ => m[0],
    };
    let y = match d.origin {
        RearLeft | RearRight => d.height - m[1],
        _ => m[1],
    };
    [x, y]
}

impl RayEtch {
    /// Imposta prima / seconda posizione target: the one selected shape (or group) is the target in
    /// the design; the laser, moved over its printed copy, gives where it really is.
    pub fn pnc_set_target(&mut self, second: bool) {
        let items = self.selection_items();
        if items.len() != 1 {
            self.pnc_message(tr("Deve essere selezionata una sola forma. Raggruppa più forme per usarle come target"));
            return;
        }
        let Some(head) = self.head_pos else {
            self.pnc_message(tr("Comunicazione non riuscita: impossibile leggere la posizione attuale del laser"));
            self.laser.send(crate::laser::Cmd::Realtime(b'?'));
            return;
        };
        let mut b = BBox::empty();
        for s in self.doc.shapes.iter().filter(|s| items[0].contains(&s.id)) {
            b.union(&s.bbox());
        }
        let t = Target { design: b.center(), laser: machine_to_design(self, head) };
        if !second {
            self.pnc.a = Some(t);
            self.pnc.fit = None;
            self.status = tr("Primo target impostato: ora seleziona il secondo target e sposta il laser sul suo centro").into();
            return;
        }
        let Some(a) = self.pnc.a else {
            self.pnc_message(tr("Imposta prima il primo target"));
            return;
        };
        let d = |p: P2, q: P2| (p[0] - q[0]).hypot(p[1] - q[1]);
        if d(a.design, t.design) < 1e-6 {
            self.pnc_message(tr("Seleziona un target diverso per la seconda posizione"));
        } else if d(a.laser, t.laser) < 1e-6 {
            self.pnc_message(tr("Devi spostare il laser sulla posizione del secondo target"));
        } else if d(a.design, t.design) < MIN_SEPARATION || d(a.laser, t.laser) < MIN_SEPARATION {
            self.pnc_message(tr("Separazione insufficiente tra i target"));
        } else {
            self.pnc.b = Some(t);
            self.pnc.fit = None;
            self.status = tr("Secondo target impostato: scegli Allinea output ai target").into();
        }
    }

    fn pnc_message(&mut self, m: &str) {
        self.status = m.into();
        self.push_console(trf("Stampa e taglia: {}", &[&m]));
    }

    /// Allinea output ai target (with or without scaling): from now on the job is sent turned and
    /// moved onto the targets, in absolute coordinates.
    pub fn pnc_align(&mut self, scale: bool) {
        if self.pnc.a.is_none() || self.pnc.b.is_none() {
            return;
        }
        self.pnc.fit = Some(scale);
        let (a, b) = (self.pnc.a.unwrap(), self.pnc.b.unwrap());
        let dl = (b.design[0] - a.design[0]).hypot(b.design[1] - a.design[1]);
        let ll = (b.laser[0] - a.laser[0]).hypot(b.laser[1] - a.laser[1]);
        let ang = ((b.laser[1] - a.laser[1]).atan2(b.laser[0] - a.laser[0]) - (b.design[1] - a.design[1]).atan2(b.design[0] - a.design[0])).to_degrees();
        self.status = if scale {
            trf("Output allineato ai target: rotazione {}°, scala {}%", &[&format!("{ang:.2}"), &format!("{:.2}", ll / dl * 100.0)])
        } else {
            trf("Output allineato ai target senza scalare: rotazione {}°", &[&format!("{ang:.2}")])
        };
    }

    pub fn pnc_reset(&mut self) {
        let wizard = self.pnc.wizard;
        self.pnc = PrintCut { wizard, ..Default::default() };
        self.status = tr("Stampa e taglia reimpostato").into();
    }

    /// The job as the laser should get it: aligned to the Stampa e taglia targets, and with the Y
    /// axis turned into rotations when a rotary is in use.
    pub fn output_transform(&self, job: &mut crate::gcode::Job) {
        let pnc = self.pnc.fit.is_some();
        let r = &self.doc.device.rotary;
        if !pnc && !r.enabled {
            return;
        }
        let y0 = if self.job_opts.start_from == crate::gcode::StartFrom::AbsoluteCoords { 0.0 } else { job.start[1] };
        let k = r.y_scale();
        let f = |p: P2| -> P2 {
            let q = if pnc { self.pnc.map(p) } else { p };
            if r.enabled { [q[0], y0 + (q[1] - y0) * k] } else { q }
        };
        for m in &mut job.moves {
            m.to = f(m.to);
        }
        if let Some(b) = job.bounds {
            let mut nb = BBox::empty();
            for p in [b.min, b.max, [b.min[0], b.max[1]], [b.max[0], b.min[1]]] {
                nb.add(f(p));
            }
            job.bounds = Some(nb);
        }
    }

    /// The job options the output uses: Stampa e taglia places the job where the targets are on the
    /// machine, so it always goes in absolute coordinates.
    pub fn output_opts(&self) -> crate::gcode::JobOptions {
        let mut o = self.job_opts.clone();
        if self.pnc.fit.is_some() {
            o.start_from = crate::gcode::StartFrom::AbsoluteCoords;
        }
        o
    }
}

fn pnc_wizard_window(app: &mut RayEtch, ctx: &egui::Context) {
    if !app.pnc.wizard {
        return;
    }
    let mut open = true;
    let mut act: Option<&str> = None;
    crate::theme::centered(ctx, "pnc_wizard", egui::Window::new(tr("Procedura guidata Stampa e taglia")).open(&mut open).resizable(false).collapsible(false)).show(ctx, |ui| {
        ui.set_max_width(440.0);
        let done = |ui: &mut egui::Ui, ok: bool, text: &str| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(if ok { "✔" } else { "•" }).color(if ok { crate::theme::ACCENT } else { crate::theme::p().text_dim }));
                ui.label(text);
            });
        };
        ui.label(tr("1. Stampa il disegno con i due target (per esempio due crocette) e mettilo nel laser."));
        ui.add_space(4.0);
        done(ui, app.pnc.a.is_some(), tr("2. Seleziona il primo target nel disegno, sposta il laser sul suo centro stampato e premi «Imposta primo target»."));
        if ui.add_enabled(app.connected.is_some(), egui::Button::new(tr("Imposta primo target"))).clicked() {
            act = Some("pnc_first");
        }
        ui.add_space(4.0);
        done(ui, app.pnc.b.is_some(), tr("3. Seleziona il secondo target, sposta il laser sul suo centro e premi «Imposta secondo target»."));
        if ui.add_enabled(app.connected.is_some() && app.pnc.a.is_some(), egui::Button::new(tr("Imposta secondo target"))).clicked() {
            act = Some("pnc_second");
        }
        ui.add_space(4.0);
        done(ui, app.pnc.fit.is_some(), tr("4. Allinea l'output: il lavoro verrà ruotato e spostato sui target stampati."));
        ui.horizontal(|ui| {
            let both = app.pnc.a.is_some() && app.pnc.b.is_some();
            if ui.add_enabled(both, egui::Button::new(tr("Allinea output ai target"))).clicked() {
                act = Some("pnc_align");
            }
            if ui.add_enabled(both, egui::Button::new(tr("Senza scalare"))).clicked() {
                act = Some("pnc_align_noscale");
            }
        });
        if let Some(h) = app.head_pos {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(trf("Posizione laser: X {}  Y {}", &[&format!("{:.2}", h[0]), &format!("{:.2}", h[1])])).color(crate::theme::p().text_dim));
        }
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(tr("Chiudi")).clicked() {
                act = Some("close");
            }
            if ui.button(tr("Reimposta")).clicked() {
                act = Some("pnc_reset");
            }
        });
    });
    match act {
        Some("close") => app.pnc.wizard = false,
        Some(id) => crate::sysmenu::run(app, ctx, id),
        None => {}
    }
    if !open {
        app.pnc.wizard = false;
    }
}

// ------------------------------------------------------------------ Impostazione rotazione

/// The rotary attachment (saved with the device). Defaults from GantryRotaryPrefs (0x1005d78b0).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Rotary {
    pub enabled: bool,
    /// false: rulli, true: mandrino
    pub chuck: bool,
    /// Y travel that turns the rotary once, mm
    pub mm_per_rotation: f64,
    pub roller_diameter: f64,
    pub object_diameter: f64,
}

impl Default for Rotary {
    fn default() -> Self {
        Rotary { enabled: false, chuck: false, mm_per_rotation: 360.0, roller_diameter: 50.0, object_diameter: 50.0 }
    }
}

impl Rotary {
    /// How much a design millimetre along Y becomes on the Y axis: one turn moves the surface by
    /// π × the diameter that touches it (the roller's on rollers, the object's in a chuck).
    pub fn y_scale(&self) -> f64 {
        let d = if self.chuck { self.object_diameter } else { self.roller_diameter };
        if d <= 0.0 { 1.0 } else { self.mm_per_rotation / (std::f64::consts::PI * d) }
    }
}

fn rotary_window(app: &mut RayEtch, ctx: &egui::Context) {
    let Some(mut r) = app.rotary_dialog.take() else { return };
    let mut open = true;
    let mut act: Option<&str> = None;
    crate::theme::centered(ctx, "rotary", egui::Window::new(tr("Impostazione rotazione")).open(&mut open).resizable(false).collapsible(false)).show(ctx, |ui| {
        ui.checkbox(&mut r.enabled, tr("Abilita unità rotante"));
        ui.add_space(6.0);
        egui::Grid::new("rotary").num_columns(2).spacing(egui::vec2(10.0, 6.0)).show(ui, |ui| {
            ui.label(tr("Tipo di unità rotante"));
            ui.horizontal(|ui| {
                ui.radio_value(&mut r.chuck, false, tr("Rulli"));
                ui.radio_value(&mut r.chuck, true, tr("Mandrino"));
            });
            ui.end_row();
            ui.label(tr("Asse rotante"));
            ui.label("Y");
            ui.end_row();
            ui.label(tr("mm per rotazione"));
            ui.add(crate::i18n::dv(&mut r.mm_per_rotation).speed(1.0).range(0.1..=100000.0).max_decimals(3));
            ui.end_row();
            ui.add_enabled(!r.chuck, egui::Label::new(tr("Diametro rullo")));
            ui.add_enabled(!r.chuck, crate::i18n::dv(&mut r.roller_diameter).speed(0.1).range(0.1..=10000.0).max_decimals(3).suffix(" mm"));
            ui.end_row();
            ui.label(tr("Diametro oggetto"));
            ui.add(crate::i18n::dv(&mut r.object_diameter).speed(0.1).range(0.1..=10000.0).max_decimals(3).suffix(" mm"));
            ui.end_row();
            ui.label(tr("Circonferenza"));
            ui.label(format!("{:.2} mm", std::f64::consts::PI * r.object_diameter));
            ui.end_row();
        });
        ui.add_space(4.0);
        ui.label(egui::RichText::new(trf("Ogni mm del disegno lungo Y diventa {} mm sull'asse Y.", &[&format!("{:.4}", r.y_scale())])).color(crate::theme::p().text_dim));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.add_enabled(app.connected.is_some(), egui::Button::new(tr("Prova rotazione"))).on_hover_text(tr("Fa fare un giro completo all'unità rotante")).clicked() {
                act = Some("test");
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr("OK")).clicked() {
                    act = Some("ok");
                }
                if ui.button(crate::i18n::cancel()).clicked() {
                    act = Some("cancel");
                }
            });
        });
    });
    match act {
        Some("ok") => {
            app.checkpoint_now();
            app.doc.device.rotary = r.clone();
            // keep the saved laser in Periferiche in step, as Impostazioni periferica does
            if let Some(slot) = app.device_sel.and_then(|i| app.device_list.devices.get_mut(i)) {
                slot.rotary = r;
                app.device_list.save();
            }
        }
        Some("cancel") => {}
        Some("test") => {
            // one full turn forward at the jog speed, in relative moves
            let f = app.jog_speed;
            app.send_lines(vec!["G91".into(), format!("G1 Y{:.3} F{:.0}", r.mm_per_rotation, f), "G90".into()]);
            app.rotary_dialog = Some(r);
        }
        _ if open => app.rotary_dialog = Some(r),
        _ => {}
    }
}

// ------------------------------------------------------------------ Distorsione conica

/// Distorsione conica: on a tapered object (a tumbler) the circumference changes along its length,
/// so the design is squeezed across Y where the object is thinner, to come out straight.
#[derive(Clone, Debug)]
pub struct TaperDialog {
    pub left: f64,
    pub right: f64,
}

/// Where the taper sends `p`: across Y about `cy`, by the reference diameter over the diameter at
/// that point (straight between the left and right edges of `b`).
pub fn taper_point(p: P2, b: &BBox, cy: f64, left: f64, right: f64, reference: f64) -> P2 {
    let u = ((p[0] - b.min[0]) / b.w().max(1e-9)).clamp(0.0, 1.0);
    let d = (left + (right - left) * u).max(1e-6);
    [p[0], cy + (p[1] - cy) * reference / d]
}

impl RayEtch {
    pub fn apply_taper(&mut self, left: f64, right: f64) {
        let mut b = BBox::empty();
        for s in self.doc.shapes.iter().filter(|s| self.selection.contains(&s.id)) {
            b.union(&s.bbox());
        }
        if !b.is_valid() {
            return;
        }
        let reference = left.max(right);
        let cy = b.center()[1];
        self.checkpoint_now();
        for s in self.doc.shapes.iter_mut().filter(|s| self.selection.contains(&s.id)) {
            if matches!(s.geom, Geometry::Image { .. }) {
                continue;
            }
            let paths: Vec<Polyline> = s
                .world_paths()
                .iter()
                .map(|p| {
                    let d = crate::toolsmenu::densify(p, 0.5);
                    Polyline { pts: d.pts.iter().map(|q| taper_point(*q, &b, cy, left, right, reference)).collect(), closed: d.closed }
                })
                .collect();
            crate::toolsmenu::world_path_shape(s, paths);
        }
        self.dirty = true;
    }
}

fn taper_window(app: &mut RayEtch, ctx: &egui::Context) {
    let Some(mut d) = app.taper_dialog.take() else { return };
    let mut open = true;
    let mut act: Option<bool> = None;
    crate::theme::centered(ctx, "taper", egui::Window::new(tr("Distorsione conica")).open(&mut open).resizable(false).collapsible(false)).show(ctx, |ui| {
        ui.set_max_width(360.0);
        ui.label(tr("Diametri dell'oggetto ai bordi sinistro e destro della selezione: il disegno viene stretto dove l'oggetto è più sottile."));
        ui.add_space(6.0);
        egui::Grid::new("taper").num_columns(2).spacing(egui::vec2(10.0, 6.0)).show(ui, |ui| {
            ui.label(tr("Diametro a sinistra"));
            ui.add(crate::i18n::dv(&mut d.left).speed(0.1).range(0.1..=10000.0).max_decimals(3).suffix(" mm"));
            ui.end_row();
            ui.label(tr("Diametro a destra"));
            ui.add(crate::i18n::dv(&mut d.right).speed(0.1).range(0.1..=10000.0).max_decimals(3).suffix(" mm"));
            ui.end_row();
        });
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(tr("OK")).clicked() {
                act = Some(true);
            }
            if ui.button(crate::i18n::cancel()).clicked() {
                act = Some(false);
            }
        });
    });
    match act {
        Some(true) => app.apply_taper(d.left, d.right),
        Some(false) => {}
        None if open => app.taper_dialog = Some(d),
        None => {}
    }
}

// ------------------------------------------------------------------ Gestisci telecamere

/// The cameras this computer has, found in the background (the system's own list).
#[derive(Clone, Default)]
pub struct Cameras {
    pub list: Arc<Mutex<Option<Vec<String>>>>,
}

impl Cameras {
    pub fn scan(ctx: &egui::Context) -> Self {
        let c = Cameras::default();
        let list = c.list.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let found = find_cameras();
            if let Ok(mut l) = list.lock() {
                *l = Some(found);
            }
            ctx.request_repaint();
        });
        c
    }
}

fn find_cameras() -> Vec<String> {
    let run = |cmd: &str, args: &[&str]| std::process::Command::new(cmd).args(args).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    if cfg!(target_os = "macos") {
        // system_profiler lists each camera as an indented "Name:" heading
        run("system_profiler", &["SPCameraDataType"])
            .lines()
            .filter(|l| l.starts_with("    ") && !l.starts_with("      ") && l.trim_end().ends_with(':'))
            .map(|l| l.trim().trim_end_matches(':').to_string())
            .collect()
    } else if cfg!(windows) {
        run("powershell", &["-NoProfile", "-Command", "Get-CimInstance Win32_PnPEntity | Where-Object { $_.PNPClass -eq 'Camera' -or $_.PNPClass -eq 'Image' } | ForEach-Object { $_.Name }"])
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect()
    } else {
        let mut v: Vec<String> = std::fs::read_dir("/sys/class/video4linux")
            .map(|d| {
                d.flatten()
                    .map(|e| {
                        let name = std::fs::read_to_string(e.path().join("name")).unwrap_or_default();
                        format!("{} ({})", name.trim(), e.file_name().to_string_lossy())
                    })
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }
}

fn cameras_window(app: &mut RayEtch, ctx: &egui::Context) {
    let Some(c) = app.cameras.clone() else { return };
    let mut open = true;
    let mut close = false;
    crate::theme::centered(ctx, "cameras", egui::Window::new(tr("Gestisci telecamere")).open(&mut open).resizable(false).collapsible(false)).show(ctx, |ui| {
        ui.set_min_width(380.0);
        ui.label(egui::RichText::new(tr("Telecamere trovate su questo computer")).strong());
        ui.add_space(4.0);
        egui::Frame::new().fill(crate::theme::p().field).corner_radius(4).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
            ui.set_min_height(120.0);
            ui.set_width(ui.available_width());
            match c.list.lock().ok().and_then(|l| l.clone()) {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(tr("Ricerca in corso…"));
                    });
                }
                Some(l) if l.is_empty() => {
                    ui.label(tr("Nessuna telecamera trovata."));
                }
                Some(l) => {
                    for n in l {
                        ui.label(format!("📷  {n}"));
                    }
                }
            }
        });
        ui.add_space(6.0);
        ui.label(egui::RichText::new(tr("RayEtch elenca le telecamere ma non mostra ancora la loro immagine sul piano di lavoro.")).color(crate::theme::p().text_dim));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button(tr("Aggiorna")).clicked() {
                app.cameras = Some(Cameras::scan(ui.ctx()));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr("Chiudi")).clicked() {
                    close = true;
                }
            });
        });
    });
    if close || !open {
        app.cameras = None;
    }
}

pub fn windows(app: &mut RayEtch, ctx: &egui::Context) {
    pnc_wizard_window(app, ctx);
    rotary_window(app, ctx);
    taper_window(app, ctx);
    cameras_window(app, ctx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_and_cut_maps_targets_and_rotary_scales_y() {
        // the paper is turned 90° and moved: design (0,0)-(100,0) found at (50,50)-(50,150)
        let mut p = PrintCut {
            a: Some(Target { design: [0.0, 0.0], laser: [50.0, 50.0] }),
            b: Some(Target { design: [100.0, 0.0], laser: [50.0, 150.0] }),
            fit: Some(true),
            wizard: false,
        };
        for (d, l) in [([0.0, 0.0], [50.0, 50.0]), ([100.0, 0.0], [50.0, 150.0]), ([50.0, 10.0], [40.0, 100.0])] {
            let q = p.map(d);
            assert!((q[0] - l[0]).abs() < 1e-9 && (q[1] - l[1]).abs() < 1e-9, "{d:?} -> {q:?}");
        }
        // found 2% further apart: scaled fits both ends, unscaled keeps the size about the middle
        p.b = Some(Target { design: [100.0, 0.0], laser: [50.0, 152.0] });
        let q = p.map([100.0, 0.0]);
        assert!((q[1] - 152.0).abs() < 1e-9);
        p.fit = Some(false);
        let (q0, q1) = (p.map([0.0, 0.0]), p.map([100.0, 0.0]));
        assert!(((q1[1] - q0[1]) - 100.0).abs() < 1e-9 && ((q0[1] + q1[1]) / 2.0 - 101.0).abs() < 1e-9);
        // rollers of 50 mm, 360 mm per turn: one turn moves the surface by π·50 mm
        let r = Rotary::default();
        assert!((r.y_scale() * std::f64::consts::PI * 50.0 - 360.0).abs() < 1e-9);
        // taper: full height where the object is widest, half where it is half as wide
        let mut b = BBox::empty();
        b.add([0.0, 0.0]);
        b.add([100.0, 20.0]);
        assert!((taper_point([0.0, 20.0], &b, 10.0, 80.0, 40.0, 80.0)[1] - 20.0).abs() < 1e-9);
        assert!((taper_point([100.0, 20.0], &b, 10.0, 80.0, 40.0, 80.0)[1] - 30.0).abs() < 1e-9);
    }
}
