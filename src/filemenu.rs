//! The File menu (reference pictures 26 and 27): new window, recent projects and templates,
//! project notes, save as template, export (SVG / DXF), the Preferenze and Pacchetti submenus,
//! printing (black only / keeping the layer colours) and saving the processed bitmap.

use crate::app::RayEtch;
use crate::i18n::{tr, trf};
use crate::model::{Geometry, PALETTE};
use eframe::egui;
use std::path::{Path, PathBuf};

/// How many recent projects / templates are remembered (estimated).
pub const RECENT_MAX: usize = 10;
/// A template is a project saved with this extension; opening one starts a new, unnamed project.
pub const TEMPLATE_EXT: &str = "rayetchtpl";
const RECENT_PROJECTS: &str = "recent_projects.json";
const RECENT_TEMPLATES: &str = "recent_templates.json";
/// A bundle: the settings folder's files in one ZIP (the original's "Pacchetti").
pub const BUNDLE_EXT: &str = "rayetchbundle";

/// One row of the File menu.
pub enum FileRow {
    Sep,
    /// action id, label, keys shown beside it, enabled
    Item(String, String, &'static str, bool),
    Sub(String, Vec<FileRow>),
}

pub fn recent(templates: bool) -> Vec<PathBuf> {
    crate::i18n::load_json::<Vec<PathBuf>>(if templates { RECENT_TEMPLATES } else { RECENT_PROJECTS }).unwrap_or_default()
}

/// Put `p` first in the recent list (MainWindow::SaveProject adds to recent files).
pub fn push_recent(p: &Path) {
    let templates = p.extension().is_some_and(|e| e == TEMPLATE_EXT);
    let mut v = recent(templates);
    v.retain(|q| q != p);
    v.insert(0, p.to_path_buf());
    v.truncate(RECENT_MAX);
    crate::i18n::save_json(if templates { RECENT_TEMPLATES } else { RECENT_PROJECTS }, &v);
}

fn settings_dir() -> Option<PathBuf> {
    crate::i18n::settings_file("prefs.json").and_then(|p| p.parent().map(|d| d.to_path_buf()))
}

/// Show a folder in the system's file manager.
pub fn open_folder(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let prog = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(prog).arg(dir).spawn();
}

/// Open a file with the program the system uses for it (the print preview of a PDF).
fn open_with_system(p: &Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("cmd").args(["/C", "start", ""]).arg(p).spawn();
    #[cfg(not(windows))]
    let _ = std::process::Command::new(if cfg!(target_os = "macos") { "open" } else { "xdg-open" }).arg(p).spawn();
}

/// Once a day, keep a copy of the settings in backups/ ("Carica preferenze di backup").
pub fn backup_prefs() {
    let Some(dir) = settings_dir() else { return };
    let src = dir.join("prefs.json");
    if !src.exists() {
        return;
    }
    let day = crate::update::today();
    let dst = dir.join("backups").join(format!("prefs-{day}.json"));
    if !dst.exists() {
        let _ = std::fs::create_dir_all(dir.join("backups"));
        let _ = std::fs::copy(&src, &dst);
    }
}

/// The File menu, in the original's order and words.
pub fn rows(image_selected: bool, has_shapes: bool) -> Vec<FileRow> {
    use FileRow::*;
    let mac = cfg!(target_os = "macos");
    let (cmd, shift_cmd, alt, alt_cmd) = if mac { ("⌘", "⇧⌘", "⌥", "⌥⌘") } else { ("Ctrl+", "Ctrl+Maiusc+", "Alt+", "Ctrl+Alt+") };
    let k = |a: &str, b: &str| -> &'static str { Box::leak(format!("{a}{b}").into_boxed_str()) };
    let it = |id: &str, label: &str, keys: &'static str, on: bool| Item(id.into(), label.into(), keys, on);
    let recent_sub = |templates: bool| {
        let list = recent(templates);
        let mut v: Vec<FileRow> = list
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                it(&format!("{}{i}", if templates { "recent_tpl_" } else { "recent_prj_" }), &format!("{}. {name}", i + 1), "", true)
            })
            .collect();
        if v.is_empty() {
            v.push(it("none", tr("(nessuno)"), "", false));
        } else {
            v.push(Sep);
            v.push(it(if templates { "recent_tpl_clear" } else { "recent_prj_clear" }, tr("Svuota elenco"), "", true));
        }
        v
    };
    let mut v = vec![
        it("new", tr("Nuovo"), k(cmd, "N"), true),
        Sep,
        it("new_window", tr("Nuova finestra"), "", true),
        Sub(tr("Progetti recenti").into(), recent_sub(false)),
        Sub(tr("Modelli recenti").into(), recent_sub(true)),
        it("open", tr("Apri"), k(cmd, "O"), true),
        it("import", tr("Importa"), k(cmd, "I"), true),
        it("notes", tr("Mostra note"), k(alt_cmd, "N"), true),
        Sep,
        it("save", tr("Salva"), k(cmd, "S"), true),
        it("save_as", tr("Salva con nome"), k(shift_cmd, "S"), true),
        it("save_template", tr("Salva come Modello"), "", true),
        it("export", tr("Esporta"), k(alt, "X"), has_shapes),
        Sep,
        Sub(
            tr("Preferenze").into(),
            vec![
                it("prefs_import", tr("Importa preferenze"), "", true),
                it("prefs_export", tr("Esporta preferenze"), "", true),
                Sep,
                it("prefs_folder", tr("Apri cartella Preferiti"), "", true),
                it("prefs_backup", tr("Carica preferenze di backup"), "", true),
                Sep,
                it("shortcuts", tr("Modifica tasti di scelta rapida"), "", true),
            ],
        ),
        Sub(tr("Pacchetti").into(), vec![it("bundle_import", tr("Importa pacchetto"), "", true), it("bundle_export", tr("Esporta pacchetto"), "", true)]),
        Sep,
        it("print_black", tr("Stampa (solo nero)"), k(cmd, "P"), has_shapes),
        it("print_color", tr("Stampa (mantieni i colori)"), k(shift_cmd, "P"), has_shapes),
        Sep,
        it("img_save", tr("Salva bitmap elaborata"), "", image_selected),
        // no camera support in RayEtch yet: there is no background capture to save
        it("save_capture", tr("Salva acquisizione sfondo"), "", false),
    ];
    if !mac {
        v.push(Sep);
        v.push(it("quit", tr("Esci"), "", true));
    }
    v
}

impl RayEtch {
    /// "Nuova finestra": another RayEtch, with its own empty project.
    pub fn new_window(&mut self) {
        match std::env::current_exe() {
            Ok(exe) => {
                let _ = std::process::Command::new(exe).spawn();
            }
            Err(e) => self.status = trf("Impossibile aprire una nuova finestra: {}", &[&e]),
        }
    }

    /// "Salva come Modello": the project as a template, which opens as a new unnamed project.
    pub fn save_template(&mut self) {
        let mut d = rfd::FileDialog::new().add_filter(tr("Modello RayEtch"), &[TEMPLATE_EXT]).set_file_name(format!("modello.{TEMPLATE_EXT}"));
        if let Some(dir) = settings_dir().map(|d| d.join("templates")) {
            let _ = std::fs::create_dir_all(&dir);
            d = d.set_directory(dir);
        }
        let Some(p) = d.save_file() else { return };
        self.store_job_state();
        match serde_json::to_string(&self.doc).map_err(|e| e.to_string()).and_then(|s| crate::fileicon::safe_write(&p, s.as_bytes())) {
            Ok(()) => {
                push_recent(&p);
                self.menu_dirty = true;
                self.status = trf("Modello salvato: {}", &[&p.display()]);
            }
            Err(e) => self.status = trf("Salvataggio non riuscito: {}", &[&e]),
        }
    }

    /// "Esporta" (⌥X): the drawing as SVG or DXF, by the extension chosen.
    pub fn export_drawing(&mut self) {
        let Some(p) = rfd::FileDialog::new().add_filter("SVG", &["svg"]).add_filter("DXF", &["dxf"]).set_file_name("design.svg").save_file() else { return };
        let dxf = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dxf"));
        let text = if dxf { self.dxf_text() } else { self.svg_text() };
        match std::fs::write(&p, text) {
            Ok(()) => self.status = trf("Esportato {}", &[&p.display()]),
            Err(e) => self.status = trf("Esportazione non riuscita: {}", &[&e]),
        }
    }

    /// The drawing as DXF (R12 POLYLINEs, one DXF layer per cut layer, mm).
    pub fn dxf_text(&self) -> String {
        let mut s = String::from("0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n");
        for sh in self.doc.shapes.iter().filter(|s| !matches!(s.geom, Geometry::Image { .. })) {
            let layer = crate::model::layer_name(sh.layer);
            for pl in sh.world_paths() {
                s += &format!("0\nPOLYLINE\n8\n{layer}\n66\n1\n70\n{}\n", if pl.closed { 1 } else { 0 });
                for p in &pl.pts {
                    s += &format!("0\nVERTEX\n8\n{layer}\n10\n{:.4}\n20\n{:.4}\n", p[0], p[1]);
                }
                s += &format!("0\nSEQEND\n8\n{layer}\n");
            }
        }
        s += "0\nENDSEC\n0\nEOF\n";
        s
    }

    /// "Stampa (solo nero)" / "Stampa (mantieni i colori)": the work area 1:1 as a PDF, lines
    /// "Larghezza linea di stampa" wide, opened in the system's viewer to print. Pictures are
    /// left out (estimated: the original prints through Qt's print dialog).
    pub fn print(&mut self, colors: bool) {
        let (w, h) = (self.doc.device.width, self.doc.device.height);
        let k = 72.0 / 25.4;
        let mut c = format!("{:.3} w 1 J 1 j\n", self.prefs.print_line_width.max(0.01) * k);
        for sh in self.doc.shapes.iter().filter(|s| !matches!(s.geom, Geometry::Image { .. })) {
            let col = if colors { PALETTE[sh.layer.min(PALETTE.len() - 1)] } else { [0, 0, 0] };
            c += &format!("{:.3} {:.3} {:.3} RG\n", col[0] as f64 / 255.0, col[1] as f64 / 255.0, col[2] as f64 / 255.0);
            for pl in sh.world_paths() {
                for (i, p) in pl.pts.iter().enumerate() {
                    c += &format!("{:.2} {:.2} {}\n", p[0] * k, p[1] * k, if i == 0 { "m" } else { "l" });
                }
                c += if pl.closed { "h S\n" } else { "S\n" };
            }
        }
        let pdf = pdf_bytes(w * k, h * k, &c);
        let path = std::env::temp_dir().join(if colors { "rayetch-stampa-colori.pdf" } else { "rayetch-stampa.pdf" });
        match std::fs::write(&path, pdf) {
            Ok(()) => {
                open_with_system(&path);
                self.status = tr("Anteprima di stampa aperta: stampa dal visualizzatore (scala 100%).").into();
            }
            Err(e) => self.status = trf("Stampa non riuscita: {}", &[&e]),
        }
    }

    /// "Importa preferenze": settings from a file chosen by the person.
    pub fn import_prefs(&mut self, ctx: &egui::Context, from_backup: bool) {
        let mut d = rfd::FileDialog::new().add_filter(tr("Preferenze"), &["json"]);
        if from_backup {
            if let Some(dir) = settings_dir().map(|d| d.join("backups")) {
                d = d.set_directory(dir);
            }
        }
        let Some(p) = d.pick_file() else { return };
        match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str::<crate::prefs::Prefs>(&s).map_err(|e| e.to_string())) {
            Ok(new) => {
                self.apply_prefs(new, ctx);
                self.status = trf("Preferenze caricate da {}", &[&p.display()]);
            }
            Err(e) => self.status = trf("File di preferenze non valido: {}", &[&e]),
        }
    }

    /// "Esporta preferenze": the current settings to a file.
    pub fn export_prefs(&mut self) {
        let Some(p) = rfd::FileDialog::new().add_filter(tr("Preferenze"), &["json"]).set_file_name("rayetch-preferenze.json").save_file() else { return };
        match serde_json::to_string_pretty(&self.prefs).map_err(|e| e.to_string()).and_then(|s| std::fs::write(&p, s).map_err(|e| e.to_string())) {
            Ok(()) => self.status = trf("Preferenze esportate in {}", &[&p.display()]),
            Err(e) => self.status = trf("Esportazione non riuscita: {}", &[&e]),
        }
    }

    /// "Esporta pacchetto": every settings file (preferences, devices, material libraries, cut
    /// defaults, templates...) in one ZIP to move to another computer.
    pub fn export_bundle(&mut self) {
        let Some(dir) = settings_dir() else { return };
        let Some(p) = rfd::FileDialog::new().add_filter(tr("Pacchetto RayEtch"), &[BUNDLE_EXT]).set_file_name(format!("rayetch.{BUNDLE_EXT}")).save_file() else { return };
        let res = (|| -> Result<usize, String> {
            let f = std::fs::File::create(&p).map_err(|e| e.to_string())?;
            let mut z = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            let mut n = 0;
            let mut stack = vec![dir.clone()];
            while let Some(d) = stack.pop() {
                for e in std::fs::read_dir(&d).map_err(|e| e.to_string())?.flatten() {
                    let path = e.path();
                    let rel = path.strip_prefix(&dir).map_err(|e| e.to_string())?.to_string_lossy().replace('\\', "/");
                    // the backups and autosave stay behind
                    if rel.starts_with("backups") || rel.starts_with("autosave") || rel.starts_with('.') {
                        continue;
                    }
                    if path.is_dir() {
                        stack.push(path);
                    } else {
                        z.start_file(rel, opts).map_err(|e| e.to_string())?;
                        std::io::copy(&mut std::fs::File::open(&path).map_err(|e| e.to_string())?, &mut z).map_err(|e| e.to_string())?;
                        n += 1;
                    }
                }
            }
            z.finish().map_err(|e| e.to_string())?;
            Ok(n)
        })();
        match res {
            Ok(n) => self.status = trf("Pacchetto esportato ({} file): {}", &[&n, &p.display()]),
            Err(e) => self.status = trf("Esportazione non riuscita: {}", &[&e]),
        }
    }

    /// "Importa pacchetto": a bundle's files go into the settings folder (existing ones are
    /// replaced), then the preferences are applied.
    pub fn import_bundle(&mut self, ctx: &egui::Context) {
        let Some(dir) = settings_dir() else { return };
        let Some(p) = rfd::FileDialog::new().add_filter(tr("Pacchetto RayEtch"), &[BUNDLE_EXT, "zip"]).pick_file() else { return };
        // a copy of the current settings first, in case the bundle wasn't the right one
        backup_prefs();
        let res = (|| -> Result<usize, String> {
            let mut z = zip::ZipArchive::new(std::fs::File::open(&p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            let mut n = 0;
            for i in 0..z.len() {
                let mut f = z.by_index(i).map_err(|e| e.to_string())?;
                let Some(rel) = f.enclosed_name() else { continue };
                if f.is_dir() {
                    continue;
                }
                let out = dir.join(rel);
                if let Some(parent) = out.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                std::io::copy(&mut f, &mut std::fs::File::create(&out).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
                n += 1;
            }
            Ok(n)
        })();
        match res {
            Ok(n) => {
                self.apply_prefs(crate::prefs::Prefs::load(), ctx);
                self.menu_dirty = true;
                self.status = trf("Pacchetto importato ({} file). Alcune impostazioni si applicano al prossimo avvio.", &[&n]);
            }
            Err(e) => self.status = trf("Importazione non riuscita: {}", &[&e]),
        }
    }

    /// Open a recent project or template (a template starts a new unnamed project).
    pub fn open_recent(&mut self, templates: bool, i: usize, ctx: &egui::Context) {
        if let Some(p) = recent(templates).get(i).cloned() {
            if p.exists() {
                self.open_path(p, ctx);
            } else {
                self.status = trf("Il file non esiste più: {}", &[&p.display()]);
            }
        }
    }
}

/// A one-page PDF of the given size (points) drawing the content stream.
fn pdf_bytes(w: f64, h: f64, content: &str) -> Vec<u8> {
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w:.2} {h:.2}] /Contents 4 0 R >>"),
        format!("<< /Length {} >>\nstream\n{content}endstream", content.len()),
    ];
    let mut out = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out += &format!("{} 0 obj\n{o}\nendobj\n", i + 1);
    }
    let xref = out.len();
    out += &format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1);
    for o in offsets {
        out += &format!("{o:010} 00000 n \n");
    }
    out += &format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1);
    out.into_bytes()
}

/// "Mostra note": the project's notes, kept in the project, shown on opening when asked.
pub fn notes_window(app: &mut RayEtch, ctx: &egui::Context) {
    if !app.show_notes {
        return;
    }
    let mut open = true;
    let before = (app.doc.notes.clone(), app.doc.show_notes_on_open);
    crate::theme::centered(ctx, "notes", egui::Window::new(tr("Note")).open(&mut open).resizable(false).collapsible(false)).show(ctx, |ui| {
        ui.add(egui::TextEdit::multiline(&mut app.doc.notes).desired_rows(12).desired_width(460.0).hint_text(tr("Note del progetto: materiale, impostazioni, istruzioni…")));
        ui.checkbox(&mut app.doc.show_notes_on_open, tr("Mostra le note all'apertura del file"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(tr("Chiudi")).clicked() {
                app.show_notes = false;
            }
        });
    });
    if (app.doc.notes.clone(), app.doc.show_notes_on_open) != before {
        app.dirty = true;
    }
    if !open {
        app.show_notes = false;
    }
}

/// "Modifica tasti di scelta rapida": the keyboard shortcuts RayEtch answers to.
pub fn shortcuts_window(app: &mut RayEtch, ctx: &egui::Context) {
    if !app.show_shortcuts {
        return;
    }
    let mac = cfg!(target_os = "macos");
    let (c, s, a) = if mac { ("⌘", "⇧", "⌥") } else { ("Ctrl+", "Maiusc+", "Alt+") };
    let rows: Vec<(String, &str)> = vec![
        (format!("{c}N"), tr("Nuovo")),
        (format!("{c}O"), tr("Apri")),
        (format!("{c}I"), tr("Importa")),
        (format!("{a}{c}N"), tr("Mostra note")),
        (format!("{c}S"), tr("Salva")),
        (format!("{s}{c}S"), tr("Salva con nome")),
        (format!("{a}X"), tr("Esporta")),
        (format!("{c}P"), tr("Stampa (solo nero)")),
        (format!("{s}{c}P"), tr("Stampa (mantieni i colori)")),
        (format!("{c}Z"), tr("Annulla")),
        (format!("{s}{c}Z"), tr("Ripristina")),
        (format!("{c}A"), tr("Seleziona tutto")),
        (format!("{s}{c}I"), tr("Inverti selezione")),
        (format!("{c}X / {c}C / {c}V"), tr("Taglia / Copia / Incolla")),
        (format!("{a}V"), tr("Incolla in posizione")),
        (format!("{c}D"), tr("Duplica")),
        (format!("{s}{c}C"), tr("Converti in percorso")),
        (format!("{s}{c}B"), tr("Converti in bitmap")),
        (format!("{a}J"), tr("Unisci automaticamente le forme selezionate")),
        (format!("{a}{s}O"), tr("Ottimizza le forme selezionate")),
        (format!("{a}D"), tr("Elimina duplicati")),
        (format!("{c}G / {s}{c}G"), tr("Raggruppa / Separa")),
        (format!("{c}K"), tr("Ritaglia")),
        (format!("{a}P"), tr("Anteprima")),
        ("+ / −".into(), tr("Ingrandisci / Riduci")),
    ];
    let mut open = true;
    crate::theme::centered(ctx, "shortcuts", egui::Window::new(tr("Tasti di scelta rapida")).open(&mut open).resizable(false).collapsible(false)).show(ctx, |ui| {
        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
            egui::Grid::new("keys").num_columns(2).striped(true).spacing(egui::vec2(24.0, 4.0)).show(ui, |ui| {
                for (k, what) in &rows {
                    ui.label(*what);
                    ui.label(egui::RichText::new(k).monospace());
                    ui.end_row();
                }
            });
        });
        ui.add_space(6.0);
        ui.label(egui::RichText::new(tr("Per ora i tasti non si possono cambiare: questo è l'elenco di quelli attivi.")).small().color(crate::theme::p().text_dim));
    });
    if !open {
        app.show_shortcuts = false;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn pdf_has_a_valid_cross_reference() {
        let pdf = String::from_utf8(super::pdf_bytes(100.0, 50.0, "0 0 m 10 10 l S\n")).unwrap();
        let at = pdf.find("4 0 obj").unwrap();
        assert!(pdf.contains(&format!("{at:010} 00000 n")));
        assert!(pdf.ends_with("%%EOF\n"));
    }
}
