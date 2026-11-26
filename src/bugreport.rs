//! Guida › Segnala un problema…: a window to describe the problem, sent to the project's GitHub
//! Issues without needing a GitHub account.
//!
//! Creating an issue needs a GitHub key, and a key inside the program could be pulled out by
//! anyone. So the report goes to a small relay (tools/bug-relay, a free Cloudflare Worker) that
//! keeps the key and opens the issue. The relay's address comes from RAYETCH_BUG_RELAY at build
//! time (or in the environment when RayEtch runs). Without a relay, "Invia" opens GitHub's new
//! issue page with the title and text already written (that one needs a GitHub account).

use crate::app::RayEtch;
use crate::i18n::{tr, trf};
use eframe::egui;
use std::sync::{Arc, Mutex};

/// The project's relay (tools/bug-relay), set up by the person on Cloudflare.
pub const DEFAULT_RELAY: &str = "https://rayetch-bug.scuoladidio.workers.dev";

pub const ISSUES: &str = "https://github.com/areax99/RayEtch/issues";

/// The relay's address: built in, or given when RayEtch runs.
pub fn relay() -> Option<String> {
    let built = option_env!("RAYETCH_BUG_RELAY").unwrap_or(DEFAULT_RELAY);
    let u = std::env::var("RAYETCH_BUG_RELAY").unwrap_or_else(|_| built.to_string());
    let u = u.trim().to_string();
    u.starts_with("https://").then_some(u)
}

#[derive(Clone, Debug, PartialEq)]
pub enum Sent {
    /// the issue's number and address
    Done(u64, String),
    Failed(String),
}

#[derive(Default)]
pub struct BugReport {
    pub title: String,
    pub description: String,
    pub steps: String,
    pub email: String,
    pub include_system: bool,
    pub sending: Option<Arc<Mutex<Option<Sent>>>>,
    pub result: Option<Sent>,
}

impl BugReport {
    pub fn new() -> Self {
        BugReport { include_system: true, ..Default::default() }
    }
}

/// What RayEtch and this computer are, to help reproduce the problem (nothing personal).
pub fn system_info(app: &RayEtch) -> String {
    let d = &app.doc.device;
    format!(
        "- RayEtch {}\n- {} {} ({})\n- Laser: {} ({}), {}\n- Lingua: {}",
        crate::update::current(),
        std::env::consts::OS,
        os_version(),
        std::env::consts::ARCH,
        if d.name.is_empty() { "-" } else { &d.name },
        if d.controller.is_empty() { "-" } else { &d.controller },
        if app.connected.is_some() { "collegato" } else { "non collegato" },
        crate::i18n::load_choice().map(|l| l.code()).unwrap_or("auto"),
    )
}

fn os_version() -> String {
    let run = |c: &str, a: &[&str]| std::process::Command::new(c).args(a).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    if cfg!(target_os = "macos") {
        run("sw_vers", &["-productVersion"])
    } else if cfg!(windows) {
        String::new()
    } else {
        std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|s| s.lines().find_map(|l| l.strip_prefix("PRETTY_NAME=").map(|v| v.trim_matches('"').to_string())))
            .unwrap_or_default()
    }
}

/// The issue's text, in Markdown.
pub fn body(r: &BugReport, system: &str) -> String {
    let mut b = format!("### Descrizione\n\n{}\n", r.description.trim());
    if !r.steps.trim().is_empty() {
        b += &format!("\n### Passi per riprodurlo\n\n{}\n", r.steps.trim());
    }
    if r.include_system {
        b += &format!("\n### Sistema\n\n{system}\n");
    }
    if !r.email.trim().is_empty() {
        b += &format!("\n### Contatto\n\n{}\n", r.email.trim());
    }
    b += "\n_Inviata da RayEtch (Guida › Segnala un problema)_\n";
    b
}

/// Percent-encode for a URL query value.
fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// GitHub's new-issue page with the text written in (the fallback without a relay).
pub fn prefilled_url(title: &str, body: &str) -> String {
    // keep the address within what browsers and GitHub accept
    let mut body = body.to_string();
    while enc(&body).len() > 6000 {
        let cut = body.char_indices().rev().nth(200).map(|(i, _)| i).unwrap_or(0);
        body.truncate(cut);
        body += "\n…";
    }
    format!("{ISSUES}/new?title={}&body={}", enc(title), enc(&body))
}

/// Send to the relay in the background with curl (as the update check does).
fn send(relay: String, payload: serde_json::Value) -> Arc<Mutex<Option<Sent>>> {
    let slot: Arc<Mutex<Option<Sent>>> = Arc::default();
    let out = slot.clone();
    std::thread::spawn(move || {
        let r = (|| -> Result<(u64, String), String> {
            let mut cmd = std::process::Command::new("curl");
            cmd.args(["-sS", "--max-time", "30", "-X", "POST", "-H", "Content-Type: application/json", "--data-binary", "@-"])
                .arg("-H")
                .arg(format!("User-Agent: RayEtch/{}", crate::update::current()))
                .arg(&relay)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x0800_0000);
            }
            let mut child = cmd.spawn().map_err(|e| e.to_string())?;
            if let Some(mut i) = child.stdin.take() {
                use std::io::Write;
                i.write_all(payload.to_string().as_bytes()).map_err(|e| e.to_string())?;
            }
            let o = child.wait_with_output().map_err(|e| e.to_string())?;
            if !o.status.success() {
                return Err(String::from_utf8_lossy(&o.stderr).trim().to_string());
            }
            let v: serde_json::Value = serde_json::from_slice(&o.stdout).map_err(|_| String::from_utf8_lossy(&o.stdout).chars().take(200).collect::<String>())?;
            match (v.get("number").and_then(|n| n.as_u64()), v.get("url").and_then(|u| u.as_str())) {
                (Some(n), Some(u)) => Ok((n, u.to_string())),
                _ => Err(v.get("error").and_then(|e| e.as_str()).unwrap_or("risposta inattesa").to_string()),
            }
        })();
        if let Ok(mut s) = out.lock() {
            *s = Some(match r {
                Ok((n, u)) => Sent::Done(n, u),
                Err(e) => Sent::Failed(e),
            });
        }
    });
    slot
}

pub fn window(app: &mut RayEtch, ctx: &egui::Context) {
    let Some(mut r) = app.bug_report.take() else { return };
    // a send finished?
    if let Some(slot) = &r.sending {
        if let Some(done) = slot.lock().ok().and_then(|mut s| s.take()) {
            r.result = Some(done);
            r.sending = None;
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }
    let relay = relay();
    let system = system_info(app);
    let mut open = true;
    let mut act: Option<&str> = None;
    crate::theme::centered(ctx, "bug_report", egui::Window::new(tr("Segnala un problema")).open(&mut open).resizable(false).collapsible(false)).show(ctx, |ui| {
        ui.set_width(560.0);
        let pal = crate::theme::p();
        if let Some(Sent::Done(n, url)) = &r.result {
            ui.label(egui::RichText::new(trf("Grazie! La segnalazione è stata inviata (n. {}).", &[n])).strong());
            ui.add_space(6.0);
            ui.hyperlink_to(url.as_str(), url.as_str());
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(tr("Chiudi")).min_size(egui::vec2(90.0, 26.0))).clicked() {
                    act = Some("close");
                }
            });
            return;
        }
        let intro = if relay.is_some() {
            tr("Descrivi cosa non funziona: la segnalazione arriva agli sviluppatori su GitHub, non serve un account.")
        } else {
            tr("Descrivi cosa non funziona: la segnalazione arriva agli sviluppatori su GitHub.")
        };
        ui.label(egui::RichText::new(intro).color(pal.text_dim));
        ui.add_space(6.0);
        let busy = r.sending.is_some();
        ui.add_enabled_ui(!busy, |ui| {
            egui::Grid::new("bug_grid").num_columns(2).spacing(egui::vec2(10.0, 8.0)).show(ui, |ui| {
                ui.label(tr("Titolo"));
                ui.add(egui::TextEdit::singleline(&mut r.title).desired_width(440.0).hint_text(tr("In breve: cosa succede?")));
                ui.end_row();
                ui.label(tr("Descrizione"));
                ui.add(egui::TextEdit::multiline(&mut r.description).desired_width(440.0).desired_rows(6).hint_text(tr("Cosa ti aspettavi e cosa è successo invece")));
                ui.end_row();
                ui.label(tr("Passi per riprodurlo"));
                ui.add(egui::TextEdit::multiline(&mut r.steps).desired_width(440.0).desired_rows(3).hint_text(tr("1. Apri…  2. Clicca…  (facoltativo)")));
                ui.end_row();
                ui.label(tr("Email"));
                ui.add(egui::TextEdit::singleline(&mut r.email).desired_width(440.0).hint_text(tr("Facoltativa, per essere ricontattato: sarà visibile pubblicamente")));
                ui.end_row();
            });
            ui.add_space(4.0);
            ui.checkbox(&mut r.include_system, tr("Includi le informazioni sul sistema"));
            if r.include_system {
                ui.label(egui::RichText::new(&system).small().monospace().color(pal.text_dim));
            }
        });
        if relay.is_none() {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(tr("Questa versione non ha il servizio di invio: «Invia» apre GitHub con il testo già scritto (lì serve un account).")).small().color(pal.text_dim));
        }
        if let Some(Sent::Failed(e)) = &r.result {
            ui.add_space(6.0);
            ui.colored_label(egui::Color32::from_rgb(220, 80, 60), trf("Invio non riuscito: {}", &[e]));
            if ui.small_button(tr("Apri GitHub con il testo già scritto")).clicked() {
                act = Some("github");
            }
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if busy {
                ui.spinner();
                ui.label(tr("Invio in corso…"));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ok = !busy && r.title.trim().len() >= 3 && r.description.trim().len() >= 10;
                if ui
                    .add_enabled(ok, egui::Button::new(tr("Invia")).min_size(egui::vec2(90.0, 26.0)))
                    .on_disabled_hover_text(tr("Scrivi un titolo e una descrizione"))
                    .clicked()
                {
                    act = Some("send");
                }
                if ui.add(egui::Button::new(crate::i18n::cancel()).min_size(egui::vec2(90.0, 26.0))).clicked() {
                    act = Some("close");
                }
            });
        });
    });
    let text = body(&r, &system);
    match act {
        Some("close") => return,
        Some("send") => {
            r.result = None;
            match relay {
                Some(url) => {
                    let payload = serde_json::json!({ "title": r.title.trim(), "body": text, "version": crate::update::current() });
                    r.sending = Some(send(url, payload));
                }
                None => {
                    crate::webtools::open_url(&prefilled_url(r.title.trim(), &text));
                    return;
                }
            }
        }
        Some("github") => {
            crate::webtools::open_url(&prefilled_url(r.title.trim(), &text));
            return;
        }
        _ => {}
    }
    if open {
        app.bug_report = Some(r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_text_and_fallback_address() {
        let r = BugReport { title: "Crash".into(), description: "Si chiude & basta".into(), include_system: true, ..Default::default() };
        let b = body(&r, "- RayEtch 1");
        assert!(b.contains("### Descrizione\n\nSi chiude & basta") && b.contains("### Sistema") && !b.contains("Contatto"));
        let u = prefilled_url("Crash ò", &b);
        assert!(u.starts_with("https://github.com/areax99/RayEtch/issues/new?title=Crash%20%C3%B2&body="));
        assert!(!u.contains(' ') && !u.contains('&') || u.matches('&').count() == 1);
        let long = "x".repeat(20000);
        assert!(prefilled_url("t", &long).len() < 6300);
    }
}
