//! The web tool window: a small browser of RayEtch's own, as the original opens Cuttle and
//! My Laser Tools in its own window (MainWindow::createWebTool, a QtWebEngine view with Back,
//! Forward, Home and Refresh) and not in the system browser.
//!
//! It runs as a second RayEtch process (`rayetch --webtool <json>`), so its window and event loop
//! never get in the way of the main window's. It uses the system's web engine (WebKit on macOS,
//! WebView2 on Windows, WebKitGTK on Linux). Like the original it tells the sites who it is with "LightBurn" at the
//! end of its user agent (webtools.json UserAgentSuffix): that is what makes Cuttle and My Laser
//! Tools show their "Send to LightBurn" button, renamed "Send to RayEtch" on the page. What it reports back goes on its standard output, one line each:
//! `IMPORT <path>` for a finished download, which the main window imports into the design
//! (the original's handleWebToolDownload).

use serde::{Deserialize, Serialize};

/// What the main window asks the tool window to open.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Spec {
    pub name: String,
    pub home: String,
    /// where the person was last time ("LastURL"), unless "Always Start from Home"
    pub start: String,
    pub allowed_domains: Vec<String>,
    pub allow_all: bool,
    pub user_agent: String,
    /// window size in points
    pub size: [f64; 2],
    /// 'it', 'en'… for the toolbar's words
    pub back: String,
    pub forward: String,
    pub home_label: String,
    pub refresh: String,
    pub downloading: String,
    pub copy: String,
    /// the "Inizia sempre da Home" switch and its words
    pub start_home: bool,
    pub start_home_label: String,
    /// shown instead of "LightBurn" on the site's buttons ("Send to LightBurn" → "Send to RayEtch")
    pub rename_to: String,
}

/// What a tool remembers between openings (the original's WebTool_<name> settings).
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct State {
    pub last_url: String,
    pub size: [f64; 2],
    /// the third-party notice has been accepted
    pub disclosed: bool,
    /// "Inizia sempre da Home" (the original's AlwaysStartFromHome)
    pub start_home: bool,
}

pub fn load_states() -> std::collections::BTreeMap<String, State> {
    crate::i18n::load_json("web_tools_state.json").unwrap_or_default()
}

pub fn save_state(name: &str, f: impl FnOnce(&mut State)) {
    let mut all = load_states();
    f(all.entry(name.to_string()).or_default());
    crate::i18n::save_json("web_tools_state.json", &all);
}

/// The host of an address, without "www.".
#[allow(dead_code)]
pub fn host(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let h = rest.split(['/', '?', '#']).next().unwrap_or("");
    let h = h.rsplit('@').next().unwrap_or(h);
    let h = h.split(':').next().unwrap_or(h);
    h.trim_start_matches("www.").to_ascii_lowercase()
}

/// Whether the tool may open `url` inside its window: its own site, the domains it allows (and
/// their subdomains), or everything; pages of the browser itself always.
#[allow(dead_code)]
pub fn allowed(spec: &Spec, url: &str) -> bool {
    if spec.allow_all || !(url.starts_with("http://") || url.starts_with("https://")) {
        return true;
    }
    let h = host(url);
    let ok = |d: &str| {
        let d = host(d.trim());
        !d.is_empty() && (h == d || h.ends_with(&format!(".{d}")))
    };
    ok(&spec.home) || spec.allowed_domains.iter().any(|d| ok(d))
}

/// Builds a web view into the tool window: a child at given bounds on macOS and Windows, packed
/// into the window's GTK box on Linux (the way that works on both X11 and Wayland).
trait BuildIn {
    fn build_in(self, window: &tao::window::Window) -> wry::Result<wry::WebView>;
}

impl BuildIn for wry::WebViewBuilder<'_> {
    #[cfg(not(target_os = "linux"))]
    fn build_in(self, window: &tao::window::Window) -> wry::Result<wry::WebView> {
        self.build_as_child(window)
    }

    #[cfg(target_os = "linux")]
    fn build_in(self, window: &tao::window::Window) -> wry::Result<wry::WebView> {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        match window.default_vbox() {
            Some(vbox) => self.build_gtk(vbox),
            None => Err(wry::Error::MessageSender),
        }
    }
}

pub fn run(spec_json: &str) {
    use std::sync::{Arc, Mutex};
    use tao::dpi::{LogicalPosition, LogicalSize};
    use tao::event::{Event, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
    use tao::window::WindowBuilder;
    use wry::{NewWindowResponse, Rect, WebViewBuilder};

    let Ok(spec) = serde_json::from_str::<Spec>(spec_json) else { return };
    const BAR: f64 = 38.0;
    #[derive(Debug)]
    enum Ev {
        Bar(String),
        Url(String),
        Downloading(String),
        Downloaded,
    }
    let event_loop = EventLoopBuilder::<Ev>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let size = if spec.size[0] > 200.0 && spec.size[1] > 200.0 { spec.size } else { [1200.0, 820.0] };
    let Ok(window) = WindowBuilder::new().with_title(&spec.name).with_inner_size(LogicalSize::new(size[0], size[1])).build(&event_loop) else { return };

    // the toolbar as in the original (picture 36): Back, Forward, Home, the address, a copy
    // button and the "Inizia sempre da Home" switch
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;");
    let start = if spec.start.is_empty() { spec.home.clone() } else { spec.start.clone() };
    let bar_html = format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><style>
html,body{{margin:0;height:100%;background:#ececec;font:13px -apple-system,Segoe UI,sans-serif;color:#222;overflow:hidden;user-select:none}}
#b{{display:flex;gap:5px;align-items:center;height:100%;padding:0 6px;box-sizing:border-box;border-bottom:1px solid #c8c8c8}}
button{{background:#fff;border:1px solid #b8b8b8;border-radius:4px;width:34px;height:26px;font-size:15px;cursor:pointer;padding:0}}
button:hover{{background:#f4f4f4}}
#u{{flex:1;height:22px;border:1px solid #b8b8b8;border-radius:3px;padding:0 5px;font:13px -apple-system,Segoe UI,sans-serif}}
label{{display:flex;align-items:center;gap:5px;white-space:nowrap;cursor:pointer}}
.sw{{width:26px;height:14px;border-radius:7px;background:#c0392b;position:relative;border:1px solid #888}}
.sw:after{{content:'';position:absolute;left:1px;top:1px;width:12px;height:12px;border-radius:6px;background:#fff}}
.on .sw{{background:#2ecc40}} .on .sw:after{{left:13px}}
#d{{color:#c0601a;white-space:nowrap}}</style></head><body><div id="b">
<button title="{back}" onclick="ipc.postMessage('back')">&#10094;</button><button title="{fwd}" onclick="ipc.postMessage('forward')">&#10095;</button>
<button title="{home}" onclick="ipc.postMessage('home')">&#8962;</button>
<input id="u" value="{url}" onkeydown="if(event.key==='Enter')ipc.postMessage('go:'+this.value)">
<button title="{copy}" onclick="var u=document.getElementById('u');u.select();document.execCommand('copy');u.blur()">&#10697;</button>
<span id="d"></span>
<label id="h" class="{on}" onclick="this.classList.toggle('on');ipc.postMessage('starthome:'+(this.classList.contains('on')?1:0))"><span class="sw"></span>{start_home}</label>
</div></body></html>"#,
        back = esc(&spec.back),
        fwd = esc(&spec.forward),
        home = esc(&spec.home_label),
        copy = esc(&spec.copy),
        url = esc(&start),
        on = if spec.start_home { "on" } else { "" },
        start_home = esc(&spec.start_home_label),
    );
    let bounds = |w: f64, h: f64, top: bool| Rect {
        position: LogicalPosition::new(0.0, if top { 0.0 } else { BAR }).into(),
        size: LogicalSize::new(w, if top { BAR } else { (h - BAR).max(1.0) }).into(),
    };
    let p = proxy.clone();
    let Ok(bar) = WebViewBuilder::new()
        .with_html(bar_html)
        .with_bounds(bounds(size[0], size[1], true))
        .with_ipc_handler(move |req| {
            let _ = p.send_event(Ev::Bar(req.body().clone()));
        })
        .build_in(&window)
    else {
        return;
    };
    // Linux: the toolbar keeps its height at the top of the window's box, the page takes the rest
    #[cfg(target_os = "linux")]
    if let Some(vbox) = {
        use tao::platform::unix::WindowExtUnix;
        window.default_vbox()
    } {
        use gtk::prelude::*;
        use wry::WebViewExtUnix;
        let w = bar.webview();
        w.set_size_request(-1, BAR as i32);
        vbox.set_child_packing(&w, false, false, 0, gtk::PackType::Start);
    }

    let dl_dir = std::env::temp_dir().join("RayEtch-webtools");
    let _ = std::fs::create_dir_all(&dl_dir);
    let pending: Arc<Mutex<Option<std::path::PathBuf>>> = Arc::default();
    let (p_nav, p_dl, p_done) = (proxy.clone(), proxy.clone(), proxy.clone());
    let (pend_a, pend_b) = (pending.clone(), pending.clone());
    let mut b = WebViewBuilder::new()
        .with_url(&start)
        .with_bounds(bounds(size[0], size[1], false))
        .with_devtools(false)
        // every page opens here, sign-in pages included: nothing goes to the system browser
        .with_navigation_handler(move |url: String| {
            if url.starts_with("http") {
                let _ = p_nav.send_event(Ev::Url(url));
            }
            true
        })
        // sign-in pop-ups (window.open) open as windows of their own, linked to the page
        .with_new_window_req_handler(|_url, _features| NewWindowResponse::Allow)
        .with_download_started_handler(move |url: String, path: &mut std::path::PathBuf| {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| url.rsplit('/').next().unwrap_or("download").split('?').next().unwrap_or("download").to_string());
            // a fresh name each time, so a second "Send to" never meets the first file
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
            let dest = dl_dir.join(format!("{stamp}-{name}"));
            *path = dest.clone();
            if let Ok(mut g) = pend_a.lock() {
                *g = Some(dest);
            }
            let _ = p_dl.send_event(Ev::Downloading(name));
            true
        })
        .with_download_completed_handler(move |_url: String, path: Option<std::path::PathBuf>, ok: bool| {
            let path = path.or_else(|| pend_b.lock().ok().and_then(|mut g| g.take()));
            if ok {
                if let Some(p) = path {
                    println!("IMPORT {}", p.display());
                }
            }
            let _ = p_done.send_event(Ev::Downloaded);
        });
    if !spec.user_agent.is_empty() {
        b = b.with_user_agent(&spec.user_agent);
    }
    if !spec.rename_to.is_empty() {
        // the site's "Send to LightBurn" button says where it really goes
        let js = format!(
            r#"(function(){{var to={to};function fix(n){{var w=document.createTreeWalker(n,NodeFilter.SHOW_TEXT);var t;while((t=w.nextNode())){{if(/LightBurn/.test(t.nodeValue)&&t.parentElement&&t.parentElement.closest('button,a,[role=button]'))t.nodeValue=t.nodeValue.replace(/LightBurn/g,to);}}}}
function go(){{fix(document.body);new MutationObserver(function(m){{m.forEach(function(r){{r.addedNodes.forEach(function(x){{if(x.nodeType===1)fix(x);else if(x.nodeType===3&&x.parentNode)fix(x.parentNode);}});if(r.type==='characterData'&&r.target.parentNode)fix(r.target.parentNode);}});}}).observe(document.body,{{childList:true,subtree:true,characterData:true}});}}
if(document.body)go();else document.addEventListener('DOMContentLoaded',go);}})();"#,
            to = serde_json::to_string(&spec.rename_to).unwrap_or_default()
        );
        b = b.with_initialization_script(js);
    }
    let Ok(view) = b.build_in(&window) else { return };
    // tells the main window the tool window is up (otherwise it falls back to the system browser)
    println!("READY");

    let name = spec.name.clone();
    let home = spec.home.clone();
    let downloading = spec.downloading.clone();
    let mut last_url = start;
    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent { event: WindowEvent::Resized(s), .. } => {
                let s = s.to_logical::<f64>(window.scale_factor());
                // on Linux the window's box lays them out by itself
                if !cfg!(target_os = "linux") {
                    let _ = bar.set_bounds(bounds(s.width, s.height, true));
                    let _ = view.set_bounds(bounds(s.width, s.height, false));
                }
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                let s = window.inner_size().to_logical::<f64>(window.scale_factor());
                let url = last_url.clone();
                save_state(&name, |st| {
                    st.last_url = url;
                    st.size = [s.width, s.height];
                });
                *flow = ControlFlow::Exit;
            }
            Event::UserEvent(Ev::Bar(cmd)) => {
                if let Some(v) = cmd.strip_prefix("starthome:") {
                    let on = v == "1";
                    save_state(&name, |st| st.start_home = on);
                } else if let Some(u) = cmd.strip_prefix("go:") {
                    let u = u.trim();
                    let u = if u.contains("://") { u.to_string() } else { format!("https://{u}") };
                    let _ = view.load_url(&u);
                } else {
                    let _ = match cmd.as_str() {
                        "back" => view.evaluate_script("history.back()"),
                        "forward" => view.evaluate_script("history.forward()"),
                        _ => view.load_url(&home),
                    };
                }
            }
            Event::UserEvent(Ev::Url(u)) => {
                let shown = serde_json::to_string(&u).unwrap_or_default();
                let _ = bar.evaluate_script(&format!("var e=document.getElementById('u');if(document.activeElement!==e)e.value={shown}"));
                last_url = u;
            }
            Event::UserEvent(Ev::Downloading(n)) => {
                let t = serde_json::to_string(&format!("{downloading} {n}")).unwrap_or_default();
                let _ = bar.evaluate_script(&format!("document.getElementById('d').textContent={t}"));
            }
            Event::UserEvent(Ev::Downloaded) => {
                let _ = bar.evaluate_script("document.getElementById('d').textContent=''");
            }
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_tool_keeps_to_its_domains() {
        let s = Spec { home: "https://cuttle.xyz/".into(), allowed_domains: vec!["stripe.com".into()], ..Default::default() };
        assert!(allowed(&s, "https://cuttle.xyz/@cuttle/x"));
        assert!(allowed(&s, "https://www.cuttle.xyz/"));
        assert!(allowed(&s, "https://checkout.stripe.com/pay"));
        assert!(!allowed(&s, "https://example.com/"));
        assert!(allowed(&s, "about:blank"));
        assert_eq!(host("https://user@Sub.Example.com:8080/a?b"), "sub.example.com");
    }
}
