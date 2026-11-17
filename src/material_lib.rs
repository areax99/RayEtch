//! "Libreria dei materiali": the original's CutLibraryWidget (the person's picture 17), a tab beside
//! Laser. A library holds cut settings filed by material, thickness and a short description
//! ("Compensato 3 mm — Taglio passante"). From it a setting is copied onto the selected layer
//! (Assegna, or a double click), or the layer is linked to it (Collega: later changes to the entry
//! reach every linked layer); new entries are made from the selected layer (Crea nuovo da livello),
//! refreshed from it (Aggiorna), edited in the cut-settings editor (Modifica taglio), renamed
//! (Modifica descrizione), duplicated and deleted. "Gestisci libreria" creates, loads and saves
//! library files; several can be open and one is picked in "Seleziona libreria".
//!
//! Rules read in the original: CutLibraryWidget::on_pbAssign / on_pbLink / on_pbCreate /
//! on_pbUpdateFromSelectedLayer / on_pbEditMat / on_pbEditDesc / on_pbDuplicate / on_pbDelete,
//! PushToLayer(bool link), RepointLinkedLayers, on_twCuts_itemDoubleClicked; texts from
//! LightBurn_it.qm (CutLibraryWidget). RayEtch keeps its libraries as .rayetchlib (JSON) and also
//! reads LightBurn's .clb (XML: LightBurnLibrary > Material name > Entry Thickness/Desc >
//! CutSetting; the field names it reads are estimated).

use crate::app::RayEtch;
use crate::i18n::{tr, trf};
use crate::model::{CutMode, CutSetting};
use eframe::egui::{self, vec2};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The open libraries and the current one, beside the settings.
const LIST_FILE: &str = "material_libraries.json";
pub const LIB_EXT: &str = "rayetchlib";

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct LibEntry {
    pub material: String,
    /// mm; None = "Nessuno spessore" (surface marking, scans…)
    pub thickness: Option<f64>,
    pub desc: String,
    pub cs: CutSetting,
}

impl LibEntry {
    fn thickness_text(&self) -> String {
        match self.thickness {
            Some(t) => format!("{} mm", fmt_num(t)),
            None => tr("(nessuno spessore)").to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Library {
    pub display_name: String,
    pub entries: Vec<LibEntry>,
    /// where it lives (not saved inside the file)
    #[serde(skip)]
    pub path: PathBuf,
}

impl Library {
    fn shown_name(&self) -> String {
        if !self.display_name.trim().is_empty() {
            self.display_name.clone()
        } else {
            self.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| tr("Libreria").into())
        }
    }

    /// "library file|material|thickness|description": what a linked layer remembers.
    fn link_key(&self, e: &LibEntry) -> String {
        format!("{}|{}|{}|{}", self.path.display(), e.material, e.thickness.map(fmt_num).unwrap_or_default(), e.desc)
    }

    fn save(&self) -> Result<(), String> {
        if self.path.as_os_str().is_empty() {
            return Err(tr("Nessuna libreria selezionata").into());
        }
        let s = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        crate::fileicon::safe_write(&self.path, s.as_bytes())
    }

    fn load(path: &std::path::Path) -> Result<Library, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let is_clb = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("clb")) || text.trim_start().starts_with('<');
        let mut lib = if is_clb { read_clb(&text)? } else { serde_json::from_str::<Library>(&text).map_err(|e| e.to_string())? };
        lib.path = path.to_path_buf();
        Ok(lib)
    }
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct SavedList {
    paths: Vec<PathBuf>,
    current: Option<usize>,
}

/// The fields while an entry is being made or renamed (the original swaps its buttons for them).
#[derive(Clone)]
struct EntryForm {
    /// None: a new entry from the selected layer; Some(i): Modifica descrizione of entry i
    editing: Option<usize>,
    material: String,
    thickness: f64,
    no_thickness: bool,
    desc: String,
}

#[derive(Default)]
pub struct LibState {
    pub libs: Vec<Library>,
    pub current: Option<usize>,
    /// the picked entry of the current library
    pub selected: Option<usize>,
    form: Option<EntryForm>,
    /// Rinomina: the new display name being typed
    rename: Option<String>,
    confirm_delete: bool,
    pub(crate) loaded: bool,
}

impl LibState {
    /// Bring back the libraries that were open last time (once).
    fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let saved: SavedList = crate::i18n::load_json(LIST_FILE).unwrap_or_default();
        for p in saved.paths {
            match Library::load(&p) {
                Ok(l) => self.libs.push(l),
                // a missing file stays in the list, as the original's "(File mancante)"
                Err(_) => self.libs.push(Library { display_name: String::new(), entries: Vec::new(), path: p }),
            }
        }
        self.current = saved.current.filter(|&i| i < self.libs.len());
    }

    fn remember(&self) {
        let list = SavedList { paths: self.libs.iter().map(|l| l.path.clone()).collect(), current: self.current };
        crate::i18n::save_json(LIST_FILE, &list);
    }

    fn lib(&self) -> Option<&Library> {
        self.current.and_then(|i| self.libs.get(i))
    }

    fn lib_mut(&mut self) -> Option<&mut Library> {
        self.current.and_then(|i| self.libs.get_mut(i))
    }
}

fn fmt_num(v: f64) -> String {
    let s = format!("{:.2}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    s.replace('.', &crate::i18n::decimal_char().to_string())
}

/// The layer the buttons act on: the row picked in Tagli / Livelli, else the current layer.
fn target_layer(app: &RayEtch) -> usize {
    app.selected_row.unwrap_or(app.current_layer).min(app.doc.layers.len().saturating_sub(1))
}

/// Copy an entry's settings onto a layer, keeping the layer's name; `link` ties it to the entry.
fn push_to_layer(app: &mut RayEtch, entry: &LibEntry, key: &str, link: bool) {
    let l = target_layer(app);
    app.checkpoint_now();
    let name = app.doc.layers[l].name.clone();
    let mut cs = entry.cs.clone();
    cs.name = name;
    cs.lib_link = if link { Some(key.to_string()) } else { None };
    // a picture keeps its Image mode, a vector layer can't take it (the person's rule)
    cs.mode = app.doc.layer_art(l).clamp(cs.mode);
    app.doc.layers[l] = cs;
    app.status = if link {
        trf("Livello {} collegato a «{} — {}»", &[&crate::model::layer_name(l), &entry.material, &entry.desc])
    } else {
        trf("Impostazioni «{} — {}» assegnate al livello {}", &[&entry.material, &entry.desc, &crate::model::layer_name(l)])
    };
}

/// After an entry changed: the layers linked to it take its new settings (RepointLinkedLayers).
fn refresh_linked(app: &mut RayEtch, old_key: &str, new_key: &str, cs: &CutSetting) {
    for l in 0..app.doc.layers.len() {
        if app.doc.layers[l].lib_link.as_deref() == Some(old_key) {
            let name = app.doc.layers[l].name.clone();
            let mode = app.doc.layer_art(l).clamp(cs.mode);
            app.doc.layers[l] = CutSetting { name, mode, lib_link: Some(new_key.to_string()), ..cs.clone() };
        }
    }
}

fn save_current(app: &mut RayEtch) {
    if let Some(lib) = app.material_lib.lib() {
        match lib.save() {
            Ok(()) => {}
            Err(e) => app.status = trf("Impossibile salvare la libreria: {}", &[&e]),
        }
    }
}

fn mode_short(m: CutMode) -> &'static str {
    match m {
        CutMode::Line => tr("Linea"),
        CutMode::Fill => tr("Riempimento"),
        CutMode::OffsetFill => tr("Riempimento offset"),
        CutMode::Image => tr("Immagine"),
    }
}

pub fn library_tab(app: &mut RayEtch, ui: &mut egui::Ui) {
    app.material_lib.ensure_loaded();
    let st = &mut app.material_lib;
    // ---- Seleziona libreria, Rinomina, Scarica
    let mut unload = false;
    ui.horizontal(|ui| {
        ui.label(tr("Seleziona libreria"));
        let shown = st.lib().map(|l| {
            let missing = !l.path.exists();
            format!("{}{}", if missing { tr("(File mancante) ") } else { "" }, l.shown_name())
        }).unwrap_or_else(|| tr("<Nessuna libreria>").into());
        let mut pick = st.current;
        egui::ComboBox::from_id_salt("lib_pick").width((ui.available_width() - 190.0).max(120.0)).selected_text(shown).show_ui(ui, |ui| {
            ui.selectable_value(&mut pick, None, tr("<Nessuna libreria>"));
            for (i, l) in st.libs.iter().enumerate() {
                ui.selectable_value(&mut pick, Some(i), l.shown_name());
            }
        });
        if pick != st.current {
            st.current = pick;
            st.selected = None;
            st.form = None;
            st.remember();
        }
        let has = st.current.is_some();
        if ui.add_enabled(has, egui::Button::new(tr("Rinomina"))).on_hover_text(tr("Rinomina la libreria dei materiali corrente. Sarà modificato solo il nome visualizzato, non il nome del file.")).clicked() {
            st.rename = st.lib().map(|l| l.shown_name());
        }
        if ui.add_enabled(has, egui::Button::new(tr("Scarica"))).on_hover_text(tr("Scarica la libreria dei materiali corrente dall'elenco delle opzioni. Il file della libreria non sarà eliminato.")).clicked() {
            unload = true;
        }
    });
    if unload {
        if let Some(i) = st.current.take() {
            st.libs.remove(i);
            st.selected = None;
            st.form = None;
            st.remember();
        }
    }
    // Rinomina: the new display name
    if let Some(mut name) = st.rename.take() {
        let mut done = None;
        ui.horizontal(|ui| {
            let r = ui.text_edit_singleline(&mut name);
            if ui.button(tr("Ok")).clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                done = Some(true);
            }
            if ui.button(crate::i18n::cancel()).clicked() {
                done = Some(false);
            }
        });
        match done {
            Some(true) => {
                if let Some(l) = st.lib_mut() {
                    l.display_name = name.trim().to_string();
                }
                save_current(app);
            }
            Some(false) => {}
            None => app.material_lib.rename = Some(name),
        }
    }

    let mut action: Option<&'static str> = None;
    let mut double_clicked: Option<usize> = None;
    let has_lib = app.material_lib.current.is_some();
    // a layer picked in Tagli / Livelli: a row of the list (layers that hold shapes)
    let layer_picked = app.selected_row.is_some_and(|r| app.doc.used_layers().contains(&r));
    let sel = app.material_lib.selected.filter(|&i| app.material_lib.lib().is_some_and(|l| i < l.entries.len()));
    app.material_lib.selected = sel;
    ui.columns(2, |c| {
        // ---- the entries, by material and thickness
        let ui = &mut c[0];
        egui::Frame::new().fill(crate::theme::p().field).stroke(egui::Stroke::new(1.0, crate::theme::p().line)).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
            ui.set_min_height(360.0);
            ui.set_width(ui.available_width());
            let st = &mut app.material_lib;
            if let Some(lib) = st.lib() {
                if lib.entries.is_empty() {
                    ui.label(egui::RichText::new(tr("Libreria vuota: seleziona un livello e premi «Crea nuovo da livello».")).color(crate::theme::p().text_dim));
                }
                // materials in order, thicknesses ascending (no thickness first)
                let mut order: Vec<usize> = (0..lib.entries.len()).collect();
                order.sort_by(|&a, &b| {
                    let (x, y) = (&lib.entries[a], &lib.entries[b]);
                    x.material.to_lowercase().cmp(&y.material.to_lowercase())
                        .then(x.thickness.unwrap_or(-1.0).total_cmp(&y.thickness.unwrap_or(-1.0)))
                        .then(x.desc.to_lowercase().cmp(&y.desc.to_lowercase()))
                });
                let mut i = 0;
                let mut clicked = None;
                while i < order.len() {
                    let mat = lib.entries[order[i]].material.clone();
                    let start = i;
                    while i < order.len() && lib.entries[order[i]].material == mat {
                        i += 1;
                    }
                    let group: Vec<usize> = order[start..i].to_vec();
                    egui::CollapsingHeader::new(egui::RichText::new(&mat).strong()).id_salt(("lib_mat", &mat)).default_open(true).show(ui, |ui| {
                        let mut j = 0;
                        while j < group.len() {
                            let th = lib.entries[group[j]].thickness_text();
                            let s2 = j;
                            while j < group.len() && lib.entries[group[j]].thickness_text() == th {
                                j += 1;
                            }
                            egui::CollapsingHeader::new(&th).id_salt(("lib_th", &mat, &th)).default_open(true).show(ui, |ui| {
                                for &k in &group[s2..j] {
                                    let e = &lib.entries[k];
                                    let text = format!("{}  ·  {} · {} · {}%", e.desc, mode_short(e.cs.mode), crate::panels::speed_text(e.cs.speed), fmt_num(e.cs.max_power));
                                    let r = ui.selectable_label(sel == Some(k), text);
                                    if r.clicked() {
                                        clicked = Some(k);
                                    }
                                    if r.double_clicked() {
                                        double_clicked = Some(k);
                                    }
                                }
                            });
                        }
                    });
                }
                if clicked.is_some() {
                    st.selected = clicked;
                    st.form = None;
                }
            } else {
                ui.label(egui::RichText::new(tr("Nessuna libreria: seleziona un livello nella finestra Tagli / Livelli e premi «Crea nuovo da livello» (la libreria si crea da sola), oppure usa «Gestisci libreria».")).color(crate::theme::p().text_dim));
            }
        });
        // ---- Gestisci libreria
        ui.menu_button(tr("Gestisci libreria"), |ui| {
            if ui.button(tr("Crea nuova libreria")).clicked() {
                action = Some("new_lib");
                ui.close();
            }
            if ui.button(tr("Carica libreria")).clicked() {
                action = Some("load_lib");
                ui.close();
            }
            if ui.add_enabled(has_lib, egui::Button::new(tr("Salva libreria"))).clicked() {
                action = Some("save_lib");
                ui.close();
            }
            if ui.add_enabled(has_lib, egui::Button::new(tr("Salva libreria con nome"))).clicked() {
                action = Some("save_lib_as");
                ui.close();
            }
            if ui.add_enabled(has_lib, egui::Button::new(tr("Unisci libreria con"))).clicked() {
                action = Some("merge_lib");
                ui.close();
            }
        });

        // ---- Materiale: the buttons, or the fields of the entry being made / renamed
        let ui = &mut c[1];
        ui.label(egui::RichText::new(tr("Materiale")).strong());
        let st = &mut app.material_lib;
        if let Some(mut f) = st.form.clone() {
            let mut done = None;
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(tr("Nome materiale"));
                ui.add(egui::TextEdit::singleline(&mut f.material).hint_text(tr("Nome del materiale da tagliare (multistrato di betulla, acrilico, ecc..)")));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(tr("Spessore"));
                    ui.add_enabled(!f.no_thickness, crate::i18n::dv(&mut f.thickness).speed(0.1).range(0.0..=500.0).max_decimals(2).suffix(" mm"));
                });
                ui.checkbox(&mut f.no_thickness, tr("Nessuno spessore"))
                    .on_hover_text(tr("Le preimpostazioni di 'Nessuno spessore' non hanno uno spessore specifico e sono generalmente utilizzate per la marcatura di superfici, scansioni e così via"));
                ui.add_space(4.0);
                ui.label(tr("Descrizione"));
                ui.add(egui::TextEdit::singleline(&mut f.desc).hint_text(tr("Breve descrizione del taglio (leggera incisione, taglio, ecc..)")));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button(tr("Ok")).on_hover_text(tr("Completa la creazione/modifica di questa voce")).clicked() {
                        done = Some(true);
                    }
                    if ui.button(crate::i18n::cancel()).on_hover_text(tr("Annulla la creazione/modifica di questa voce")).clicked() {
                        done = Some(false);
                    }
                });
            });
            match done {
                Some(true) if f.material.trim().is_empty() || f.desc.trim().is_empty() => {
                    app.status = tr("Nome o descrizione mancanti: servono entrambi.").into();
                    app.material_lib.form = Some(f);
                }
                Some(true) => {
                    app.material_lib.form = None;
                    finish_form(app, f);
                }
                Some(false) => app.material_lib.form = None,
                None => app.material_lib.form = Some(f),
            }
        } else {
            let b = |ui: &mut egui::Ui, on: bool, text: &str, tip: &str| -> bool {
                ui.add_enabled(on, egui::Button::new(text).min_size(vec2(ui.available_width(), 28.0))).on_hover_text(tip).clicked()
            };
            let picked = sel.is_some();
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if b(ui, picked, tr("Assegna"), tr("Copia queste impostazioni di taglio della libreria sul livello attualmente selezionato")) {
                    action = Some("assign");
                }
                if b(ui, picked, tr("Collega"), tr("Collega il livello selezionato a questa voce: le modifiche alla libreria si riflettono sul livello")) {
                    action = Some("link");
                }
                ui.add_space(30.0);
                // on only with a layer picked in Tagli / Livelli (the person's rule); without a
                // library open, the first entry makes one (see "create")
                if b(ui, layer_picked, tr("Crea nuovo da livello"), if layer_picked {
                    tr("Crea una voce della libreria dalle impostazioni del livello selezionato")
                } else {
                    tr("Seleziona prima un livello nella finestra Tagli / Livelli")
                }) {
                    action = Some("create");
                }
                if b(ui, picked, tr("Modifica taglio"), tr("Modifica le impostazioni di taglio di questa voce")) {
                    action = Some("edit_cut");
                }
                if b(ui, picked, tr("Aggiorna"), tr("Aggiorna la voce della libreria dal livello attualmente selezionato")) {
                    action = Some("update");
                }
                if b(ui, picked, tr("Modifica descrizione"), tr("Modifica materiale, spessore e descrizione di questa voce")) {
                    action = Some("edit_desc");
                }
                ui.add_space(30.0);
                if b(ui, picked, tr("Duplica"), tr("Crea una copia di questa voce")) {
                    action = Some("duplicate");
                }
                if b(ui, picked, tr("Elimina"), tr("Elimina la voce della libreria")) {
                    // Shift skips the question, as in the original
                    action = Some(if ui.input(|i| i.modifiers.shift) { "delete_now" } else { "delete" });
                }
            });
            let l = target_layer(app);
            ui.label(egui::RichText::new(trf("Livello selezionato: {}", &[&crate::model::layer_name(l)])).color(crate::theme::p().text_dim));
            if let Some(key) = &app.doc.layers[l].lib_link {
                let parts: Vec<&str> = key.split('|').collect();
                if parts.len() == 4 {
                    ui.label(egui::RichText::new(trf("Collegato a: {} > {} > {}", &[&parts[1], &if parts[2].is_empty() { tr("(nessuno spessore)").to_string() } else { format!("{} mm", parts[2]) }, &parts[3]])).color(crate::theme::ACCENT));
                    if ui.small_button(tr("Scollega")).clicked() {
                        action = Some("unlink");
                    }
                }
            }
        }
    });
    // Elimina: "Sei sicuro…"
    if app.material_lib.confirm_delete {
        let mut answer = None;
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(tr("Sei sicuro di voler eliminare questa voce? (premi Maiusc per ignorare questo messaggio)"));
            ui.horizontal(|ui| {
                if ui.button(tr("Elimina")).clicked() {
                    answer = Some(true);
                }
                if ui.button(crate::i18n::cancel()).clicked() {
                    answer = Some(false);
                }
            });
        });
        if let Some(a) = answer {
            app.material_lib.confirm_delete = false;
            if a {
                action = Some("delete_now");
            }
        }
    }
    if let Some(k) = double_clicked {
        app.material_lib.selected = Some(k);
        action = Some("assign");
    }
    if let Some(a) = action {
        run_action(app, a);
    }
}

fn finish_form(app: &mut RayEtch, f: EntryForm) {
    let thickness = if f.no_thickness { None } else { Some(f.thickness) };
    let layer_cs = app.doc.layers[target_layer(app)].clone();
    let Some(ci) = app.material_lib.current else { return };
    let (old_key, new_key, cs) = {
        let lib = &mut app.material_lib.libs[ci];
        match f.editing {
            None => {
                let mut cs = layer_cs;
                cs.lib_link = None;
                lib.entries.push(LibEntry { material: f.material.trim().into(), thickness, desc: f.desc.trim().into(), cs });
                app.material_lib.selected = Some(lib.entries.len() - 1);
                (String::new(), String::new(), None)
            }
            Some(i) => {
                let old = lib.link_key(&lib.entries[i]);
                let e = &mut lib.entries[i];
                e.material = f.material.trim().into();
                e.thickness = thickness;
                e.desc = f.desc.trim().into();
                let e = lib.entries[i].clone();
                (old, lib.link_key(&e), Some(e.cs))
            }
        }
    };
    if let Some(cs) = cs {
        refresh_linked(app, &old_key, &new_key, &cs);
    }
    save_current(app);
}

fn run_action(app: &mut RayEtch, a: &str) {
    let st = &mut app.material_lib;
    let sel = st.selected;
    match a {
        "assign" | "link" => {
            let Some((e, key)) = st.lib().zip(sel).and_then(|(l, i)| l.entries.get(i).map(|e| (e.clone(), l.link_key(e)))) else { return };
            push_to_layer(app, &e, &key, a == "link");
        }
        "unlink" => {
            let l = target_layer(app);
            app.checkpoint_now();
            app.doc.layers[l].lib_link = None;
        }
        "create" => {
            // no library open yet: one is made in the settings folder, so the first entry
            // can be saved straight away (the person's request: the button stayed grey)
            if st.current.is_none() {
                ensure_default_library(app);
            }
            let st = &mut app.material_lib;
            st.form = Some(EntryForm { editing: None, material: String::new(), thickness: 3.0, no_thickness: false, desc: String::new() });
        }
        "edit_desc" => {
            if let Some(e) = st.lib().zip(sel).and_then(|(l, i)| l.entries.get(i)) {
                st.form = Some(EntryForm { editing: sel, material: e.material.clone(), thickness: e.thickness.unwrap_or(3.0), no_thickness: e.thickness.is_none(), desc: e.desc.clone() });
            }
        }
        "update" => {
            let l = target_layer(app);
            let cs = app.doc.layers[l].clone();
            let st = &mut app.material_lib;
            let Some((ci, i)) = st.current.zip(sel) else { return };
            let lib = &mut st.libs[ci];
            let key = lib.link_key(&lib.entries[i]);
            lib.entries[i].cs = CutSetting { lib_link: None, ..cs };
            let new_cs = lib.entries[i].cs.clone();
            refresh_linked(app, &key, &key, &new_cs);
            save_current(app);
            app.status = trf("Voce aggiornata dal livello {}", &[&crate::model::layer_name(l)]);
        }
        "edit_cut" => {
            if sel.is_some() {
                app.lib_edit = sel;
                app.editor_layer = Some(0);
                app.editor_orig = None;
                app.editor_tab = 0;
            }
        }
        "duplicate" => {
            let Some((ci, i)) = st.current.zip(sel) else { return };
            let lib = &mut st.libs[ci];
            let mut e = lib.entries[i].clone();
            e.desc = trf("{} (copia)", &[&e.desc]);
            lib.entries.push(e);
            st.selected = Some(lib.entries.len() - 1);
            save_current(app);
        }
        "delete" => st.confirm_delete = true,
        "delete_now" => {
            let Some((ci, i)) = st.current.zip(sel) else { return };
            st.libs[ci].entries.remove(i);
            st.selected = None;
            save_current(app);
        }
        "new_lib" => {
            let Some(p) = rfd::FileDialog::new().add_filter(tr("Libreria dei materiali RayEtch"), &[LIB_EXT]).set_file_name(format!("materiali.{LIB_EXT}")).save_file() else { return };
            let p = with_ext(p);
            let name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let lib = Library { display_name: name, entries: Vec::new(), path: p };
            add_library(app, lib);
            save_current(app);
        }
        "load_lib" => {
            let Some(p) = rfd::FileDialog::new().add_filter(tr("Librerie dei materiali"), &[LIB_EXT, "clb"]).pick_file() else { return };
            if let Some(i) = st.libs.iter().position(|l| l.path == p) {
                st.current = Some(i);
                st.remember();
                return;
            }
            match Library::load(&p) {
                Ok(lib) => {
                    let n = lib.entries.len();
                    // a LightBurn .clb is kept as a RayEtch library beside it, so saving never
                    // rewrites the original file in another format
                    let lib = if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("clb")) {
                        Library { path: p.with_extension(LIB_EXT), ..lib }
                    } else {
                        lib
                    };
                    add_library(app, lib);
                    save_current(app);
                    app.status = trf("Libreria caricata: {} voci", &[&n]);
                }
                Err(e) => app.status = trf("Impossibile caricare la libreria ({})", &[&e]),
            }
        }
        "save_lib" => {
            save_current(app);
            app.status = tr("Libreria dei materiali salvata.").into();
        }
        "save_lib_as" => {
            let Some(p) = rfd::FileDialog::new().add_filter(tr("Libreria dei materiali RayEtch"), &[LIB_EXT]).save_file() else { return };
            if let Some(l) = st.lib_mut() {
                l.path = with_ext(p);
            }
            save_current(app);
            app.material_lib.remember();
            app.status = tr("Libreria dei materiali salvata.").into();
        }
        "merge_lib" => {
            // "Unisci libreria con": entries of another library are added; an entry with the
            // same material, thickness and description keeps the current one's settings
            let Some(p) = rfd::FileDialog::new().add_filter(tr("Librerie dei materiali"), &[LIB_EXT, "clb"]).pick_file() else { return };
            match Library::load(&p) {
                Ok(other) => {
                    let Some(lib) = st.lib_mut() else { return };
                    let mut added = 0;
                    for e in other.entries {
                        let same = lib.entries.iter().any(|x| x.material == e.material && x.thickness == e.thickness && x.desc == e.desc);
                        if !same {
                            lib.entries.push(e);
                            added += 1;
                        }
                    }
                    save_current(app);
                    app.status = trf("Unite {} voci nuove", &[&added]);
                }
                Err(e) => app.status = trf("Impossibile caricare la libreria ({})", &[&e]),
            }
        }
        _ => {}
    }
}

/// "La mia libreria" in the settings folder, opened (or made) when an entry is created with no
/// library open.
fn ensure_default_library(app: &mut RayEtch) {
    let Some(path) = crate::i18n::settings_file(&format!("La mia libreria.{LIB_EXT}")) else { return };
    if let Some(i) = app.material_lib.libs.iter().position(|l| l.path == path) {
        app.material_lib.current = Some(i);
        app.material_lib.remember();
        return;
    }
    let lib = Library::load(&path).unwrap_or(Library { display_name: tr("La mia libreria").into(), entries: Vec::new(), path: path.clone() });
    add_library(app, Library { path, ..lib });
    save_current(app);
    app.status = tr("Creata la libreria «La mia libreria»").into();
}

fn with_ext(p: PathBuf) -> PathBuf {
    if p.extension().is_some_and(|e| e.eq_ignore_ascii_case(LIB_EXT)) { p } else { p.with_extension(LIB_EXT) }
}

fn add_library(app: &mut RayEtch, lib: Library) {
    let st = &mut app.material_lib;
    st.libs.push(lib);
    st.current = Some(st.libs.len() - 1);
    st.selected = None;
    st.form = None;
    st.remember();
}

/// "Modifica taglio": the cut-settings editor on the picked entry, through a stand-in layer 00,
/// so the project and its undo history stay as they are. The layers linked to it follow.
pub fn entry_editor(app: &mut RayEtch, ctx: &egui::Context) {
    let Some(i) = app.lib_edit else { return };
    let Some(entry) = app.material_lib.lib().and_then(|l| l.entries.get(i)).cloned() else {
        app.lib_edit = None;
        app.editor_layer = None;
        return;
    };
    let mut stand_in = crate::model::Document::default();
    stand_in.device = app.doc.device.clone();
    stand_in.layers[0] = CutSetting { name: format!("{} — {}", entry.material, entry.desc), ..entry.cs.clone() };
    std::mem::swap(&mut app.doc, &mut stand_in);
    let undo = std::mem::take(&mut app.undo);
    let redo = std::mem::take(&mut app.redo);
    let dirty = app.dirty;
    app.editor_layer = Some(0);
    crate::panels::layer_editor(app, ctx);
    let edited = app.doc.layers[0].clone();
    std::mem::swap(&mut app.doc, &mut stand_in);
    app.undo = undo;
    app.redo = redo;
    app.dirty = dirty;
    let changed = serde_json::to_string(&edited).ok() != serde_json::to_string(&CutSetting { name: format!("{} — {}", entry.material, entry.desc), ..entry.cs.clone() }).ok();
    if changed {
        let cs = CutSetting { name: entry.cs.name.clone(), lib_link: None, ..edited };
        let key = app.material_lib.lib().map(|l| l.link_key(&entry)).unwrap_or_default();
        if let Some(l) = app.material_lib.lib_mut() {
            l.entries[i].cs = cs.clone();
        }
        refresh_linked(app, &key, &key, &cs);
    }
    if app.editor_layer.is_none() {
        app.lib_edit = None;
        save_current(app);
    }
}

// ---------------------------------------------------------------- LightBurn .clb

/// Read a LightBurn cut library: <LightBurnLibrary DisplayName> <Material name> <Entry Thickness
/// Desc NoThickTitle> <CutSetting type> with one child per value (<speed Value=…>, mm/s).
/// Only the common values are taken; tag names other than the library's own are estimated.
fn read_clb(text: &str) -> Result<Library, String> {
    let doc = roxmltree::Document::parse(text).map_err(|e| e.to_string())?;
    let root = doc.root_element();
    if root.tag_name().name() != "LightBurnLibrary" {
        return Err(tr("non è una libreria di LightBurn").into());
    }
    let mut lib = Library { display_name: root.attribute("DisplayName").unwrap_or("").to_string(), ..Default::default() };
    for mat in root.children().filter(|n| n.has_tag_name("Material")) {
        let material = mat.attribute("name").unwrap_or("?").to_string();
        for ent in mat.children().filter(|n| n.has_tag_name("Entry")) {
            let thickness = ent.attribute("Thickness").and_then(|t| t.parse::<f64>().ok()).filter(|t| *t >= 0.0);
            let desc = ent.attribute("Desc").unwrap_or("").to_string();
            let Some(cut) = ent.children().find(|n| n.is_element() && n.tag_name().name().starts_with("CutSetting")) else { continue };
            let mut cs = CutSetting::default();
            cs.mode = match cut.attribute("type").unwrap_or("Cut") {
                "Scan" => CutMode::Fill,
                "Offset" | "Offset Fill" => CutMode::OffsetFill,
                "Image" => CutMode::Image,
                _ => CutMode::Line,
            };
            let val = |name: &str| cut.children().find(|n| n.has_tag_name(name)).and_then(|n| n.attribute("Value")).and_then(|v| v.parse::<f64>().ok());
            if let Some(v) = val("speed") {
                cs.speed = v * 60.0;
            }
            if let Some(v) = val("maxPower") {
                cs.max_power = v;
            }
            cs.min_power = val("minPower").unwrap_or(cs.max_power).min(cs.max_power);
            if let Some(v) = val("interval") {
                cs.interval = v;
            }
            if let Some(v) = val("numPasses") {
                cs.passes = (v as u32).max(1);
            }
            if let Some(v) = val("angle") {
                cs.angle = v;
            }
            if let Some(v) = val("bidir") {
                cs.bidir = v != 0.0;
            }
            if let Some(v) = val("crossHatch") {
                cs.crosshatch = v != 0.0;
            }
            lib.entries.push(LibEntry { material: material.clone(), thickness, desc, cs });
        }
    }
    Ok(lib)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_lightburn_library() {
        let xml = r#"<?xml version="1.0"?>
<LightBurnLibrary DisplayName="Diodo">
  <Material name="Compensato">
    <Entry Thickness="3" Desc="Taglio passante" NoThickTitle="">
      <CutSetting type="Cut">
        <speed Value="5"/><maxPower Value="90"/><minPower Value="10"/><numPasses Value="2"/>
      </CutSetting>
    </Entry>
    <Entry Thickness="-1" Desc="Incisione" NoThickTitle="">
      <CutSetting type="Scan"><speed Value="100"/><maxPower Value="25"/><interval Value="0.08"/></CutSetting>
    </Entry>
  </Material>
</LightBurnLibrary>"#;
        let lib = read_clb(xml).unwrap();
        assert_eq!(lib.display_name, "Diodo");
        assert_eq!(lib.entries.len(), 2);
        let cut = &lib.entries[0];
        assert_eq!((cut.material.as_str(), cut.thickness, cut.desc.as_str()), ("Compensato", Some(3.0), "Taglio passante"));
        assert_eq!(cut.cs.mode, CutMode::Line);
        assert_eq!(cut.cs.speed, 300.0);
        assert_eq!(cut.cs.passes, 2);
        let scan = &lib.entries[1];
        assert_eq!(scan.thickness, None);
        assert_eq!(scan.cs.mode, CutMode::Fill);
        assert!((scan.cs.interval - 0.08).abs() < 1e-9);
    }

    #[test]
    fn library_round_trips_as_json() {
        let lib = Library { display_name: "Prova".into(), entries: vec![LibEntry { material: "Acrilico".into(), thickness: Some(3.0), desc: "Taglio".into(), cs: CutSetting::default() }], path: PathBuf::new() };
        let s = serde_json::to_string(&lib).unwrap();
        let back: Library = serde_json::from_str(&s).unwrap();
        assert_eq!(back.entries.len(), 1);
        assert_eq!(back.entries[0].material, "Acrilico");
    }
}

/// A sample library in memory, for screenshots (RAYETCH_OPEN=library).
pub fn sample(app: &mut RayEtch) {
    let mk = |material: &str, t: Option<f64>, desc: &str, mode: CutMode, speed: f64, power: f64| {
        let mut cs = CutSetting::default();
        cs.mode = mode;
        cs.speed = speed;
        cs.max_power = power;
        cs.min_power = power;
        LibEntry { material: material.into(), thickness: t, desc: desc.into(), cs }
    };
    let lib = Library {
        display_name: "Laser a diodo".into(),
        entries: vec![
            mk("Compensato", Some(3.0), "Taglio passante", CutMode::Line, 300.0, 100.0),
            mk("Compensato", Some(3.0), "Incisione leggera", CutMode::Fill, 6000.0, 25.0),
            mk("Compensato", Some(6.0), "Taglio passante", CutMode::Line, 150.0, 100.0),
            mk("Acrilico nero", Some(3.0), "Taglio", CutMode::Line, 200.0, 100.0),
            mk("Cuoio", None, "Marcatura", CutMode::Fill, 3000.0, 30.0),
        ],
        path: PathBuf::from("/tmp/esempio.rayetchlib"),
    };
    let st = &mut app.material_lib;
    st.loaded = true;
    st.libs = vec![lib];
    st.current = Some(0);
    st.selected = Some(0);
}
