//! The Strumenti Web menu (reference picture 33): Cuttle and My Laser Tools open their sites in
//! the browser, then the person's own web tools, then "Gestisci strumenti utente…".
//!
//! The original (MainWindow::SetupWebTools, Core_WebToolsLoader) downloads its list from
//! webtools.json: Cuttle at https://cuttle.xyz/ and My Laser Tools at
//! https://lightburn.mylasertools.com/lightburn, opened in its own browser window; RayEtch opens
//! them in its own window too (webview.rs). The person's own tools ("UserWebTools") are kept in
//! user_web_tools.json beside the settings.

use crate::app::RayEtch;
use crate::filemenu::FileRow;
use crate::i18n::tr;
use eframe::egui;
use serde::{Deserialize, Serialize};

/// The original's two web tools: (menu id, name, address, other domains they may open), from
/// its webtools.json ("AllowedDomains").
pub const BUILT_IN: [(&str, &str, &str, &str); 2] = [
    ("web_cuttle", "Cuttle", "https://cuttle.xyz/", "stripe.com"),
    ("web_mlt", "My Laser Tools", "https://lightburn.mylasertools.com/lightburn", "accounts.google.com"),
];

/// Files the tool windows downloaded, waiting to be imported by the main window.
static DOWNLOADS: std::sync::Mutex<Vec<std::path::PathBuf>> = std::sync::Mutex::new(Vec::new());

/// A web engine's usual identity, for tools that add their own suffix to it.
fn platform_user_agent() -> &'static str {
    if cfg!(windows) {
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0"
    } else {
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15"
    }
}

fn spec(name: &str, home: &str, domains: Vec<String>, allow_all: bool, suffix: &str, start_home: bool) -> crate::webview::Spec {
    let st = crate::webview::load_states().remove(name).unwrap_or_default();
    let start_home = start_home || st.start_home;
    let start = if start_home || st.last_url.is_empty() || !st.last_url.starts_with("http") { String::new() } else { st.last_url };
    crate::webview::Spec {
        name: name.into(),
        home: if home.contains("://") { home.into() } else { format!("https://{home}") },
        start,
        allowed_domains: domains,
        allow_all,
        user_agent: if suffix.trim().is_empty() { String::new() } else { format!("{} {}", platform_user_agent(), suffix.trim()) },
        size: st.size,
        back: tr("Indietro").into(),
        forward: tr("Avanti").into(),
        home_label: tr("Home").into(),
        refresh: tr("Aggiorna").into(),
        downloading: tr("Download in corso:").into(),
        copy: tr("Copia indirizzo").into(),
        start_home,
        start_home_label: tr("Inizia sempre da Home").into(),
        rename_to: "RayEtch".into(),
    }
}

/// Open a tool in its own window: the third-party notice first, the first time (the original's
/// "Third-Party Service Notice", remembered per tool).
fn open_tool(app: &mut RayEtch, s: crate::webview::Spec) {
    if crate::webview::load_states().get(&s.name).is_some_and(|st| st.disclosed) {
        launch(app, &s);
    } else {
        app.web_notice = Some(s);
    }
}

/// Start the tool window as a second RayEtch process and listen for the files it downloads.
fn launch(app: &mut RayEtch, s: &crate::webview::Spec) {
    let Ok(exe) = std::env::current_exe() else { return };
    let Ok(json) = serde_json::to_string(s) else { return };
    match std::process::Command::new(exe).arg("--webtool").arg(json).stdout(std::process::Stdio::piped()).spawn() {
        Ok(mut child) => {
            app.status = format!("{}: {}", s.name, s.home);
            if let Some(out) = child.stdout.take() {
                let ctx = app.repaint_ctx.clone();
                std::thread::spawn(move || {
                    use std::io::BufRead;
                    for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                        if let Some(p) = line.strip_prefix("IMPORT ") {
                            if let Ok(mut q) = DOWNLOADS.lock() {
                                q.push(std::path::PathBuf::from(p));
                            }
                            if let Some(c) = &ctx {
                                c.request_repaint();
                            }
                        }
                    }
                    let _ = child.wait();
                });
            }
        }
        Err(_) => open_url(&s.home),
    }
}

/// Import what the tool windows downloaded (the original's handleWebToolDownload), and show the
/// third-party notice when a tool waits for it.
pub fn pump(app: &mut RayEtch, ctx: &egui::Context) {
    if app.repaint_ctx.is_none() {
        app.repaint_ctx = Some(ctx.clone());
    }
    let files: Vec<std::path::PathBuf> = DOWNLOADS.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
    for f in files {
        let ext = f.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if crate::formats::all_exts().iter().any(|e| e.eq_ignore_ascii_case(&ext)) {
            app.import_path(&f);
            // "Send to RayEtch": the design comes forward with what was sent
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        } else {
            app.status = crate::i18n::trf("Scaricato {} (non importabile)", &[&f.display()]);
        }
    }
    let Some(s) = app.web_notice.clone() else { return };
    let mut open = true;
    let mut act: Option<bool> = None;
    crate::theme::centered(ctx, "web_notice", egui::Window::new(tr("Avviso sul servizio di terze parti")).open(&mut open).resizable(false).collapsible(false)).show(ctx, |ui| {
        ui.set_max_width(460.0);
        ui.label(tr("Questa funzione può dare accesso a siti o servizi di terze parti gestiti in modo indipendente da RayEtch. RayEtch non controlla e non è responsabile della disponibilità, dei contenuti, del funzionamento, della sicurezza, della privacy, delle transazioni o di qualsiasi operazione di tali siti."));
        ui.add_space(6.0);
        ui.label(tr("L'uso di questi servizi può essere soggetto a termini e informative sulla privacy separati, che sei tenuto a leggere e rispettare."));
        ui.add_space(6.0);
        ui.label(tr("Continuando, riconosci che (a) questa funzione è regolata dai termini e dall'informativa sulla privacy della terza parte; e (b) RayEtch non è responsabile di perdite, danni, violazioni di dati, interruzioni o altri problemi derivanti dall'uso del sito o dei servizi di terze parti."));
        ui.add_space(10.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(egui::Button::new(tr("Continua")).min_size(egui::vec2(90.0, 26.0))).clicked() {
                act = Some(true);
            }
            if ui.add(egui::Button::new(crate::i18n::cancel()).min_size(egui::vec2(90.0, 26.0))).clicked() {
                act = Some(false);
            }
        });
    });
    match act {
        Some(true) => {
            crate::webview::save_state(&s.name, |st| st.disclosed = true);
            app.web_notice = None;
            launch(app, &s);
        }
        Some(false) => app.web_notice = None,
        None if !open => app.web_notice = None,
        None => {}
    }
}

/// One of the person's own web tools, with the original's fields (picture 35; WebTool:
/// Name, URL, Icon, Category, and under Avanzate the user agent suffix, allowed domains,
/// "Allow all domains" and "Always Start from Home").
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct UserTool {
    pub name: String,
    pub url: String,
    /// a picture file chosen for it
    pub icon: String,
    /// "Categoria": a name puts the tool in a submenu
    pub category: String,
    pub user_agent: String,
    /// other domains the tool may open, one per line
    pub allowed_domains: String,
    pub allow_all: bool,
    pub start_home: bool,
}

pub fn load_user_tools() -> Vec<UserTool> {
    crate::i18n::load_json("user_web_tools.json").unwrap_or_default()
}

pub fn rows() -> Vec<FileRow> {
    let mut v: Vec<FileRow> = BUILT_IN.iter().map(|(id, name, _, _)| FileRow::Item(id.to_string(), name.to_string(), "", true)).collect();
    let user = load_user_tools();
    if !user.is_empty() {
        v.push(FileRow::Sep);
        // tools without a category first, then one submenu per category, in first-seen order
        let mut cats: Vec<(String, Vec<FileRow>)> = Vec::new();
        for (i, t) in user.iter().enumerate() {
            let row = FileRow::Item(format!("web_user_{i}"), t.name.clone(), "", true);
            let c = t.category.trim();
            if c.is_empty() {
                v.push(row);
            } else if let Some(e) = cats.iter_mut().find(|e| e.0 == c) {
                e.1.push(row);
            } else {
                cats.push((c.to_string(), vec![row]));
            }
        }
        for (c, rows) in cats {
            v.push(FileRow::Sub(c, rows));
        }
    }
    v.push(FileRow::Sep);
    v.push(FileRow::Item("web_manage".into(), tr("Gestisci strumenti utente…").into(), "", true));
    v
}

/// Open an address in the system browser.
pub fn open_url(url: &str) {
    let url = if url.contains("://") { url.to_string() } else { format!("https://{url}") };
    #[cfg(windows)]
    let r = std::process::Command::new("cmd").args(["/C", "start", "", &url]).spawn();
    #[cfg(not(windows))]
    let r = std::process::Command::new(if cfg!(target_os = "macos") { "open" } else { "xdg-open" }).arg(&url).spawn();
    let _ = r;
}

/// Run a Strumenti Web item; false when `id` isn't one.
pub fn run(app: &mut RayEtch, id: &str) -> bool {
    if let Some((_, name, url, domains)) = BUILT_IN.iter().find(|(i, _, _, _)| *i == id) {
        // webtools.json gives both tools the user agent suffix "LightBurn": the sites show their
        // "Send to LightBurn" button only then
        let s = spec(name, url, vec![domains.to_string()], false, "LightBurn", false);
        open_tool(app, s);
        return true;
    }
    if let Some(i) = id.strip_prefix("web_user_").and_then(|n| n.parse::<usize>().ok()) {
        if let Some(t) = load_user_tools().get(i) {
            let domains = t.allowed_domains.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
            let s = spec(&t.name, &t.url, domains, t.allow_all, &t.user_agent, t.start_home);
            open_tool(app, s);
        }
        return true;
    }
    if id == "web_manage" {
        let l = load_user_tools();
        let sel = if l.is_empty() { None } else { Some(0) };
        app.web_tools_edit = Some((l, sel));
        return true;
    }
    false
}

/// "Gestisci strumenti utente…" (WebToolsManageDialog, picture 35): the list on the left with
/// Aggiungi / Rimuovi, the picked tool's fields on the right.
pub fn manage_window(app: &mut RayEtch, ctx: &egui::Context) {
    let Some((mut list, mut sel)) = app.web_tools_edit.take() else { return };
    let mut open = true;
    let mut act: Option<bool> = None;
    crate::theme::centered(ctx, "web_tools", egui::Window::new(tr("Gestisci strumenti utente")).open(&mut open).resizable(false).collapsible(false)).show(ctx, |ui| {
        let pal = crate::theme::p();
        ui.horizontal_top(|ui| {
            // the list
            ui.vertical(|ui| {
                egui::Frame::new().fill(pal.field).stroke(egui::Stroke::new(1.0, pal.line)).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                    ui.set_min_size(egui::vec2(250.0, 360.0));
                    ui.set_max_width(250.0);
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        for (i, t) in list.iter().enumerate() {
                            let name = if t.name.trim().is_empty() { tr("(senza nome)").to_string() } else { t.name.clone() };
                            if ui.add_sized([240.0, 22.0], egui::Button::selectable(sel == Some(i), name)).clicked() {
                                sel = Some(i);
                            }
                        }
                    });
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(tr("Aggiungi")).min_size(egui::vec2(123.0, 26.0))).clicked() {
                        list.push(UserTool { url: "https://".into(), ..Default::default() });
                        sel = Some(list.len() - 1);
                    }
                    if ui.add_enabled(sel.is_some(), egui::Button::new(tr("Rimuovi")).min_size(egui::vec2(123.0, 26.0))).clicked() {
                        if let Some(i) = sel.take() {
                            list.remove(i);
                            if !list.is_empty() {
                                sel = Some(i.min(list.len() - 1));
                            }
                        }
                    }
                });
            });
            ui.add_space(8.0);
            // the picked tool
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(tr("Strumento")).color(pal.text_dim));
                egui::Frame::new().fill(pal.panel).stroke(egui::Stroke::new(1.0, pal.line)).corner_radius(3).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                    ui.set_min_size(egui::vec2(430.0, 334.0));
                    ui.set_max_width(430.0);
                    let mut blank = UserTool { url: "https://".into(), ..Default::default() };
                    let on = sel.is_some();
                    let t = match sel {
                        Some(i) => &mut list[i],
                        None => &mut blank,
                    };
                    ui.add_enabled_ui(on, |ui| {
                        egui::Grid::new("web_tool").num_columns(2).spacing(egui::vec2(10.0, 8.0)).show(ui, |ui| {
                            ui.label(tr("Nome:"));
                            ui.add(egui::TextEdit::singleline(&mut t.name).desired_width(320.0));
                            ui.end_row();
                            ui.label(tr("URL:"));
                            ui.add(egui::TextEdit::singleline(&mut t.url).desired_width(320.0));
                            ui.end_row();
                            ui.label(tr("Icona:"));
                            ui.horizontal(|ui| {
                                let shown = std::path::Path::new(&t.icon).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                                ui.label(shown);
                                if ui.small_button(tr("Scegli…")).clicked() {
                                    if let Some(f) = rfd::FileDialog::new().add_filter(tr("Immagini"), &["png", "jpg", "jpeg", "ico", "svg"]).pick_file() {
                                        t.icon = f.display().to_string();
                                    }
                                }
                                if !t.icon.is_empty() && ui.small_button("✕").clicked() {
                                    t.icon.clear();
                                }
                            });
                            ui.end_row();
                            ui.label(tr("Categoria:"));
                            ui.add(egui::TextEdit::singleline(&mut t.category).desired_width(320.0).hint_text(tr("Digita un nome per inserire questo strumento in un sottomenu")));
                            ui.end_row();
                        });
                        ui.add_space(4.0);
                        egui::CollapsingHeader::new(tr("Avanzate")).id_salt("web_tool_adv").show(ui, |ui| {
                            egui::Grid::new("web_tool_adv_grid").num_columns(2).spacing(egui::vec2(10.0, 8.0)).show(ui, |ui| {
                                ui.label(tr("Suffisso user agent:"));
                                ui.add(egui::TextEdit::singleline(&mut t.user_agent).desired_width(250.0));
                                ui.end_row();
                                ui.label(tr("Domini consentiti:"));
                                ui.add_enabled(!t.allow_all, egui::TextEdit::multiline(&mut t.allowed_domains).desired_width(250.0).desired_rows(3).hint_text(tr("Un dominio per riga")));
                                ui.end_row();
                                ui.label("");
                                ui.checkbox(&mut t.allow_all, tr("Consenti tutti i domini"));
                                ui.end_row();
                                ui.label("");
                                ui.checkbox(&mut t.start_home, tr("Avvia sempre dalla home"));
                                ui.end_row();
                            });
                            ui.label(egui::RichText::new(tr("I domini consentiti si aprono nella finestra dello strumento; gli altri nel browser del computer.")).small().color(pal.text_dim));
                        });
                    });
                });
            });
        });
        ui.add_space(14.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(egui::Button::new(tr("OK")).min_size(egui::vec2(90.0, 26.0))).clicked() {
                act = Some(true);
            }
            if ui.add(egui::Button::new(crate::i18n::cancel()).min_size(egui::vec2(90.0, 26.0))).clicked() {
                act = Some(false);
            }
        });
    });
    match act {
        Some(true) => {
            list.retain(|t| !t.name.trim().is_empty() && t.url.trim().len() > "https://".len());
            crate::i18n::save_json("user_web_tools.json", &list);
            app.menu_dirty = true;
        }
        Some(false) => {}
        None if open => app.web_tools_edit = Some((list, sel)),
        None => {}
    }
}
