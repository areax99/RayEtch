//! Bar codes ("Crea codice a barre"), as the original's BarCodeShape / BarCodeTool / BarCodeWidget:
//! drag a box on the canvas (BarCodeTool::EndDrag 0x10077854c), then the "Proprietà codice a
//! barre" window opens beside it (ShowModeless with CreateBarCode_CallBack 0x10068dd00: Cancel
//! takes the new code away again). The original encodes with Zint, built into it (ZBarcode_*
//! from 0x101302018), and draws AprilTag / ArUco markers with OpenCV's dictionaries
//! (BuildSVG_OpenCV 0x1006a7694). RayEtch builds the same Zint (vendor/zint) and carries the same
//! marker codebooks (barcode_dicts.rs), and turns the modules into filled rectangles in mm.

use crate::i18n::tr;
use eframe::egui;
use crate::model::{P2, Polyline};
use serde::{Deserialize, Serialize};

/// How a kind of code is made.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Engine {
    /// Zint symbology number (zint.h BARCODE_*)
    Zint(i32),
    /// marker codebook: (bits per side, codes)
    Marker(&'static (usize, &'static [u64])),
}

/// The error correction choices a kind offers: Zint's option_1 for each step and its name.
pub type EccSteps = &'static [(i32, &'static str)];

pub struct Kind {
    pub name: &'static str,
    pub engine: Engine,
    pub ecc: EccSteps,
    /// most characters the content box takes (the "4000 / 4000" under it)
    pub max_chars: usize,
    /// a two-dimensional code (square modules), not a row of bars
    pub matrix: bool,
}

const QR_ECC: EccSteps = &[(1, "Low"), (2, "Medium"), (3, "Quartile"), (4, "High")];
const MQR_ECC: EccSteps = &[(1, "Low"), (2, "Medium"), (3, "Quartile")];
const RMQR_ECC: EccSteps = &[(2, "Medium"), (4, "High")];
const AZTEC_ECC: EccSteps = &[(1, "10%"), (2, "23%"), (3, "36%"), (4, "50%")];
const HANXIN_ECC: EccSteps = &[(1, "L1"), (2, "L2"), (3, "L3"), (4, "L4")];
const GRID_ECC: EccSteps = &[(1, "10%"), (2, "20%"), (3, "30%"), (4, "40%"), (5, "50%")];
const PDF_ECC: EccSteps = &[(0, "0"), (1, "1"), (2, "2"), (3, "3"), (4, "4"), (5, "5"), (6, "6"), (7, "7"), (8, "8")];
const NO_ECC: EccSteps = &[];

/// The 29 kinds in the original's list (reference picture 19), in its order.
pub static KINDS: [Kind; 29] = [
    Kind { name: "2 of 5", engine: Engine::Zint(2), ecc: NO_ECC, max_chars: 112, matrix: false },
    Kind { name: "2 of 5 IATA", engine: Engine::Zint(4), ecc: NO_ECC, max_chars: 80, matrix: false },
    Kind { name: "2 of 5 Industrial", engine: Engine::Zint(7), ecc: NO_ECC, max_chars: 79, matrix: false },
    Kind { name: "AprilTag 16h5", engine: Engine::Marker(&crate::barcode_dicts::APRILTAG_16H5), ecc: NO_ECC, max_chars: 4, matrix: true },
    Kind { name: "AprilTag 25h9", engine: Engine::Marker(&crate::barcode_dicts::APRILTAG_25H9), ecc: NO_ECC, max_chars: 4, matrix: true },
    Kind { name: "AprilTag 36h10", engine: Engine::Marker(&crate::barcode_dicts::APRILTAG_36H10), ecc: NO_ECC, max_chars: 4, matrix: true },
    Kind { name: "AprilTag 36h11", engine: Engine::Marker(&crate::barcode_dicts::APRILTAG_36H11), ecc: NO_ECC, max_chars: 4, matrix: true },
    Kind { name: "ArUco MIP 36h11", engine: Engine::Marker(&crate::barcode_dicts::ARUCO_MIP_36H12), ecc: NO_ECC, max_chars: 4, matrix: true },
    Kind { name: "ArUco OpenCV 7x7", engine: Engine::Marker(&crate::barcode_dicts::ARUCO_7X7_1000), ecc: NO_ECC, max_chars: 4, matrix: true },
    Kind { name: "Aztec", engine: Engine::Zint(92), ecc: AZTEC_ECC, max_chars: 3000, matrix: true },
    Kind { name: "Chinese Sensible Code", engine: Engine::Zint(116), ecc: HANXIN_ECC, max_chars: 3000, matrix: true },
    Kind { name: "CodaBar", engine: Engine::Zint(18), ecc: NO_ECC, max_chars: 103, matrix: false },
    Kind { name: "Code 128", engine: Engine::Zint(20), ecc: NO_ECC, max_chars: 102, matrix: false },
    Kind { name: "Code 128 GS1", engine: Engine::Zint(16), ecc: NO_ECC, max_chars: 102, matrix: false },
    Kind { name: "Code 93", engine: Engine::Zint(25), ecc: NO_ECC, max_chars: 123, matrix: false },
    Kind { name: "DataMatrix", engine: Engine::Zint(71), ecc: NO_ECC, max_chars: 3116, matrix: true },
    Kind { name: "EAN", engine: Engine::Zint(13), ecc: NO_ECC, max_chars: 19, matrix: false },
    Kind { name: "Grid Matrix", engine: Engine::Zint(142), ecc: GRID_ECC, max_chars: 2751, matrix: true },
    Kind { name: "ISBN", engine: Engine::Zint(69), ecc: NO_ECC, max_chars: 19, matrix: false },
    Kind { name: "PDF417", engine: Engine::Zint(55), ecc: PDF_ECC, max_chars: 2710, matrix: false },
    Kind { name: "PDF417 Compact", engine: Engine::Zint(56), ecc: PDF_ECC, max_chars: 2710, matrix: false },
    Kind { name: "PDF417 Micro", engine: Engine::Zint(84), ecc: NO_ECC, max_chars: 366, matrix: false },
    Kind { name: "Plessey", engine: Engine::Zint(86), ecc: NO_ECC, max_chars: 67, matrix: false },
    Kind { name: "QR Code", engine: Engine::Zint(58), ecc: QR_ECC, max_chars: 4000, matrix: true },
    Kind { name: "QR Micro", engine: Engine::Zint(97), ecc: MQR_ECC, max_chars: 35, matrix: true },
    Kind { name: "QR Micro Rectangular", engine: Engine::Zint(145), ecc: RMQR_ECC, max_chars: 361, matrix: true },
    Kind { name: "UPC A", engine: Engine::Zint(34), ecc: NO_ECC, max_chars: 18, matrix: false },
    Kind { name: "UPC E", engine: Engine::Zint(37), ecc: NO_ECC, max_chars: 13, matrix: false },
    Kind { name: "VIN", engine: Engine::Zint(73), ecc: NO_ECC, max_chars: 17, matrix: false },
];

/// The list shows QR Code first chosen (reference picture 18).
pub const DEFAULT_KIND: usize = 23;

/// "Modalità di input": Zint's input modes (DATA_MODE, UNICODE_MODE, GS1_MODE).
pub const INPUT_MODES: [(i32, &str); 3] = [(0, "Dati"), (1, "Unicode"), (2, "GS1")];
/// The GS1 entry of INPUT_MODES (control characters aren't processed with it).
const GS1_INPUT: usize = 2;

/// What a bar code shape is made from; kept with the shape so it can be edited again.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BarcodeSpec {
    /// index into KINDS
    pub kind: usize,
    pub content: String,
    /// index into the kind's error correction steps
    #[serde(default)]
    pub ecc: usize,
    /// index into INPUT_MODES
    #[serde(default)]
    pub input_mode: usize,
    #[serde(default)]
    pub control_chars: bool,
    /// the human-readable line under a row of bars
    #[serde(default = "yes")]
    pub show_text: bool,
    /// "Valutazione del testo": index into vartext::EVAL_TYPES
    #[serde(default)]
    pub eval: usize,
    /// "Offset variabile": added to the project's counter for this code
    #[serde(default)]
    pub var_offset: i64,
    /// the box the code fills, mm
    pub w: f64,
    pub h: f64,
}

fn yes() -> bool {
    true
}

impl BarcodeSpec {
    pub fn new(w: f64, h: f64) -> Self {
        BarcodeSpec { kind: DEFAULT_KIND, content: String::new(), ecc: 0, input_mode: 0, control_chars: false, show_text: true, eval: 0, var_offset: 0, w, h }
    }

    pub fn kind(&self) -> &'static Kind {
        &KINDS[self.kind.min(KINDS.len() - 1)]
    }
}

/// The dark modules of a code: a grid (rows × width), and each row's height in modules.
pub struct Modules {
    pub rows: usize,
    pub width: usize,
    pub dark: Vec<bool>,
    pub heights: Vec<f32>,
    pub text: String,
}

unsafe extern "C" {
    fn rayetch_zint_encode(
        symbology: i32,
        data: *const u8,
        len: i32,
        option_1: i32,
        option_2: i32,
        option_3: i32,
        input_mode: i32,
        modules: *mut u8,
        cap: i32,
        rows: *mut i32,
        width: *mut i32,
        heights: *mut f32,
        text: *mut u8,
        err: *mut u8,
    ) -> i32;
}

fn c_text(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

/// Encode the spec's content into modules, or say why it can't be.
pub fn encode(spec: &BarcodeSpec) -> Result<Modules, String> {
    let kind = spec.kind();
    let text = evaluated(spec);
    if text.is_empty() {
        return Err(tr("Inserisci il contenuto del codice").into());
    }
    match kind.engine {
        Engine::Marker(&(n, codes)) => {
            // BuildSVG_OpenCV: the content is the tag number ("Invalid Number", "Invalid Tag ID")
            let id: usize = text.trim().parse().map_err(|_| tr("Numero non valido").to_string())?;
            let code = *codes.get(id).ok_or_else(|| crate::i18n::trf("ID del tag non valido (0–{})", &[&(codes.len() - 1)]))?;
            // generateImageMarker with a one-module black border; bit 1 is white
            let side = n + 2;
            let mut dark = vec![true; side * side];
            for r in 0..n {
                for c in 0..n {
                    let bit = (code >> (n * n - 1 - (r * n + c))) & 1;
                    dark[(r + 1) * side + c + 1] = bit == 0;
                }
            }
            Ok(Modules { rows: side, width: side, dark, heights: vec![1.0; side], text: String::new() })
        }
        Engine::Zint(sym) => {
            let data = text.as_bytes();
            let option_1 = kind.ecc.get(spec.ecc).map(|e| e.0).unwrap_or(-1);
            let mode = INPUT_MODES[spec.input_mode.min(INPUT_MODES.len() - 1)].0;
            const CAP: usize = 200 * 1152;
            let mut modules = vec![0u8; CAP];
            let (mut rows, mut width) = (0i32, 0i32);
            let mut heights = vec![0f32; 200];
            let mut text = vec![0u8; 256];
            let mut err = vec![0u8; 128];
            let ret = unsafe {
                rayetch_zint_encode(
                    sym,
                    data.as_ptr(),
                    data.len() as i32,
                    option_1,
                    0,
                    0,
                    mode,
                    modules.as_mut_ptr(),
                    CAP as i32,
                    &mut rows,
                    &mut width,
                    heights.as_mut_ptr(),
                    text.as_mut_ptr(),
                    err.as_mut_ptr(),
                )
            };
            if ret >= 5 {
                let msg = c_text(&err);
                return Err(if msg.is_empty() { tr("Contenuto non valido per questo tipo di codice").into() } else { msg });
            }
            let (rows, width) = (rows.max(0) as usize, width.max(0) as usize);
            heights.truncate(rows);
            Ok(Modules { rows, width, dark: modules[..rows * width].iter().map(|&b| b != 0).collect(), heights, text: c_text(&text) })
        }
    }
}

/// The code as filled rectangles in mm, centred on the origin, Y up, fitted in the spec's box:
/// a two-dimensional code keeps square modules; a row of bars fills the box, with its text line
/// under it when show_text is on; stacked codes (PDF417) keep Zint's row heights.
pub fn outline(spec: &BarcodeSpec) -> Result<Vec<Polyline>, String> {
    let m = encode(spec)?;
    if m.rows == 0 || m.width == 0 {
        return Ok(Vec::new());
    }
    let kind = spec.kind();
    let (bw, bh) = (spec.w.abs().max(0.1), spec.h.abs().max(0.1));
    let mut out = Vec::new();
    let rect = |x0: f64, y0: f64, x1: f64, y1: f64| Polyline { pts: vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]], closed: true };
    // each row's runs of dark modules, as (row, first column, past-the-end column)
    let runs = |r: usize| {
        let mut v = Vec::new();
        let mut c = 0;
        while c < m.width {
            if m.dark[r * m.width + c] {
                let s = c;
                while c < m.width && m.dark[r * m.width + c] {
                    c += 1;
                }
                v.push((s, c));
            } else {
                c += 1;
            }
        }
        v
    };
    if kind.matrix {
        let cell = (bw / m.width as f64).min(bh / m.rows as f64);
        let (x0, y0) = (-cell * m.width as f64 / 2.0, cell * m.rows as f64 / 2.0);
        for r in 0..m.rows {
            for (a, b) in runs(r) {
                let top = y0 - r as f64 * cell;
                out.push(rect(x0 + a as f64 * cell, top - cell, x0 + b as f64 * cell, top));
            }
        }
        return Ok(out);
    }
    // rows of bars: one row (linear) fills the height; stacked rows share it by Zint's heights
    let text_band = if spec.show_text && m.rows == 1 && !m.text.is_empty() { (bh * 0.18).min(bw * 0.12) } else { 0.0 };
    let bars_h = bh - text_band;
    let cell_w = bw / m.width as f64;
    let total: f64 = if m.rows == 1 { 1.0 } else { m.heights.iter().map(|h| (*h as f64).max(0.01)).sum() };
    let (x0, mut top) = (-bw / 2.0, bh / 2.0);
    for r in 0..m.rows {
        let rh = if m.rows == 1 { bars_h } else { bars_h * (m.heights[r] as f64).max(0.01) / total };
        for (a, b) in runs(r) {
            out.push(rect(x0 + a as f64 * cell_w, top - rh, x0 + b as f64 * cell_w, top));
        }
        top -= rh;
    }
    if text_band > 0.0 {
        let size = text_band * 0.8;
        let mut lines = crate::text::text_outline(&m.text, size, "");
        if let Some((lo, hi)) = bounds(&lines) {
            // centred under the bars, scaled down if wider than the code
            let k = ((bw * 0.98) / (hi[0] - lo[0]).max(1e-9)).min(1.0);
            let (cx, cy) = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
            let ty = -bh / 2.0 + text_band / 2.0;
            for l in &mut lines {
                for p in &mut l.pts {
                    *p = [(p[0] - cx) * k, (p[1] - cy) * k + ty];
                }
            }
            out.extend(lines);
        }
    }
    Ok(out)
}

fn bounds(ls: &[Polyline]) -> Option<(P2, P2)> {
    let mut it = ls.iter().flat_map(|l| l.pts.iter());
    let f = *it.next()?;
    Some(it.fold((f, f), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])])))
}

/// The WI-FI tab's fields.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wifi {
    pub ssid: String,
    pub password: String,
    /// 0 WPA, 1 WEP, 2 none
    pub auth: usize,
    pub hidden: bool,
}

pub const WIFI_AUTH: [&str; 3] = ["WPA", "WEP", "nopass"];

impl Wifi {
    /// BarCodeWidget::RebuildWifiContent (0x100819074): "WIFI:S:<ssid>;", "P:<password>;" when
    /// there is one and the network uses WPA or WEP, "T:<WPA|WEP|nopass>;", "H:true;" for a
    /// hidden network, then the closing ";". Nothing when both fields are empty.
    pub fn content(&self) -> String {
        if self.ssid.is_empty() && self.password.is_empty() {
            return String::new();
        }
        let mut s = format!("WIFI:S:{};", self.ssid);
        let auth = self.auth.min(2);
        if !self.password.is_empty() && auth <= 1 {
            s += &format!("P:{};", self.password);
        }
        s += &format!("T:{};", WIFI_AUTH[auth]);
        if self.hidden {
            s += "H:true;";
        }
        s.push(';');
        s
    }

    /// FillWifiControls (0x1008176ec): read the fields back from a "WIFI:" content.
    pub fn parse(c: &str) -> Option<Wifi> {
        let body = c.strip_prefix("WIFI:")?;
        let mut w = Wifi { auth: 2, ..Default::default() };
        for part in body.split(';') {
            if let Some(v) = part.strip_prefix("S:") {
                w.ssid = v.into();
            } else if let Some(v) = part.strip_prefix("P:") {
                w.password = v.into();
            } else if let Some(v) = part.strip_prefix("T:") {
                w.auth = WIFI_AUTH.iter().position(|a| *a == v).unwrap_or(2);
            } else if part == "H:true" {
                w.hidden = true;
            }
        }
        Some(w)
    }
}

/// The Contatto tab's fields.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Contact {
    pub first: String,
    pub last: String,
    pub address: String,
    pub phone: String,
    pub email: String,
    pub note: String,
    pub url: String,
}

impl Contact {
    /// BarCodeWidget::RebuildContactContent (0x10081950c): "MECARD:", then "N:<last>,<first>;"
    /// when there is a name, "ADR:", "TEL:", "EMAIL:", "NOTE:", "URL:" for the fields filled in,
    /// then the closing ";". Nothing when every field is empty.
    pub fn content(&self) -> String {
        let fields = [&self.first, &self.last, &self.address, &self.phone, &self.email, &self.note, &self.url];
        if fields.iter().all(|f| f.is_empty()) {
            return String::new();
        }
        let mut s = String::from("MECARD:");
        if !self.first.is_empty() || !self.last.is_empty() {
            s += &format!("N:{},{};", self.last, self.first);
        }
        for (tag, v) in [("ADR:", &self.address), ("TEL:", &self.phone), ("EMAIL:", &self.email), ("NOTE:", &self.note), ("URL:", &self.url)] {
            if !v.is_empty() {
                s += tag;
                s += v;
                s.push(';');
            }
        }
        s.push(';');
        s
    }

    /// FillContactControls (0x100817c28): read the fields back from a "MECARD:" content.
    pub fn parse(c: &str) -> Option<Contact> {
        let body = c.strip_prefix("MECARD:")?;
        let mut k = Contact::default();
        for part in body.split(';') {
            if let Some(v) = part.strip_prefix("N:") {
                let mut it = v.splitn(2, ',');
                k.last = it.next().unwrap_or("").into();
                k.first = it.next().unwrap_or("").into();
            } else if let Some(v) = part.strip_prefix("ADR:") {
                k.address = v.into();
            } else if let Some(v) = part.strip_prefix("TEL:") {
                k.phone = v.into();
            } else if let Some(v) = part.strip_prefix("EMAIL:") {
                k.email = v.into();
            } else if let Some(v) = part.strip_prefix("NOTE:") {
                k.note = v.into();
            } else if let Some(v) = part.strip_prefix("URL:") {
                k.url = v.into();
            }
        }
        Some(k)
    }
}

/// The open "Proprietà codice a barre" window.
pub struct Dialog {
    pub shape: u64,
    /// the shape was made by the drag that opened this window: Cancel removes it
    pub created: bool,
    pub original: BarcodeSpec,
    pub spec: BarcodeSpec,
    /// 0 Contenuto grezzo, 1 WI-FI, 2 Contatto
    pub tab: usize,
    pub wifi: Wifi,
    pub contact: Contact,
}

impl Dialog {
    pub fn new(shape: u64, spec: BarcodeSpec, created: bool) -> Self {
        // InitControlsFromShape (0x10081474c): a WIFI: or MECARD: content opens on its own tab
        let wifi = Wifi::parse(&spec.content);
        let contact = Contact::parse(&spec.content);
        let tab = if wifi.is_some() {
            1
        } else if contact.is_some() {
            2
        } else {
            0
        };
        Dialog { shape, created, original: spec.clone(), spec, tab, wifi: wifi.unwrap_or_default(), contact: contact.unwrap_or_default() }
    }
}

/// The options the original remembers for the next code (Preferences LastBarCode_CodeType,
/// _ShowText, _Opt1, _ProcessControlChars, _InputMode).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LastOptions {
    pub kind: usize,
    pub ecc: usize,
    pub input_mode: usize,
    pub control_chars: bool,
    pub show_text: bool,
    #[serde(default)]
    pub eval: usize,
    #[serde(default)]
    pub var_offset: i64,
}

const LAST_FILE: &str = "last_barcode.json";

pub fn new_spec(w: f64, h: f64) -> BarcodeSpec {
    let mut s = BarcodeSpec::new(w, h);
    if let Some(l) = crate::i18n::load_json::<LastOptions>(LAST_FILE) {
        s.kind = l.kind.min(KINDS.len() - 1);
        s.ecc = l.ecc;
        s.input_mode = l.input_mode.min(INPUT_MODES.len() - 1);
        s.control_chars = l.control_chars;
        s.show_text = l.show_text;
        s.eval = l.eval.min(crate::vartext::EVAL_TYPES.len() - 1);
        s.var_offset = l.var_offset;
    }
    s
}

pub fn remember(s: &BarcodeSpec) {
    crate::i18n::save_json(LAST_FILE, &LastOptions { kind: s.kind, ecc: s.ecc, input_mode: s.input_mode, control_chars: s.control_chars, show_text: s.show_text, eval: s.eval, var_offset: s.var_offset });
}

/// Make a bar code shape in the dragged box (BarCodeTool::EndDrag 0x10077854c: a new BarCodeShape
/// with the remembered options on the current layer, added as one undo step "Create Bar Code"),
/// then open its window.
pub fn create(app: &mut crate::app::RayEtch, centre: P2, w: f64, h: f64) {
    let spec = new_spec(w, h);
    let geom = crate::model::Geometry::Path { subpaths: outline(&spec).unwrap_or_default() };
    let mut s = crate::model::Shape::new(0, geom, app.current_layer, centre[0], centre[1]);
    s.barcode = Some(spec.clone());
    app.checkpoint_now();
    let id = app.doc.add_shape(s);
    app.selection = vec![id];
    app.barcode_dialog = Some(Dialog::new(id, spec, true));
}

/// Open the window of an existing bar code (on_actionEdit_BarCode_triggered 0x100c86448, also on
/// a double click: SelectionTool::DoubleClick).
pub fn edit(app: &mut crate::app::RayEtch, id: u64) -> bool {
    let Some(spec) = app.doc.shapes.iter().find(|s| s.id == id).and_then(|s| s.barcode.clone()) else { return false };
    app.barcode_dialog = Some(Dialog::new(id, spec, false));
    true
}

/// Put a spec on its shape and rebuild the outlines; a content that can't be encoded leaves the
/// shape empty until it can.
pub fn apply(app: &mut crate::app::RayEtch, id: u64, spec: &BarcodeSpec) {
    if let Some(s) = app.doc.shapes.iter_mut().find(|s| s.id == id) {
        s.geom = crate::model::Geometry::Path { subpaths: outline(spec).unwrap_or_default() };
        s.barcode = Some(spec.clone());
    }
}

/// The text the code will hold (BarCodeShape::GetEvaluatedString 0x1006a657c): the content
/// evaluated by its type (date/time, CSV merge or serial number, with the project's counter plus
/// the code's own offset), then <RS>, <GS>... turned into those characters when "Elabora
/// caratteri di controllo" is on and the input mode isn't GS1.
pub fn evaluated(spec: &BarcodeSpec) -> String {
    let s = crate::vartext::evaluate(&spec.content, spec.eval, spec.var_offset);
    if spec.control_chars && spec.input_mode != GS1_INPUT {
        crate::vartext::control_chars(&s)
    } else {
        s
    }
}

/// Rebuild every code whose text depends on the counter, the CSV file or the clock (after the
/// counter moves, a CSV file is loaded, or before a job is made).
pub fn refresh_variable(app: &mut crate::app::RayEtch) {
    crate::vartext::set(&app.doc.vars);
    for s in app.doc.shapes.iter_mut() {
        if let Some(spec) = s.barcode.as_ref().filter(|b| b.eval != crate::vartext::EVAL_NORMAL) {
            s.geom = crate::model::Geometry::Path { subpaths: outline(spec).unwrap_or_default() };
        }
    }
}

/// "Proprietà codice a barre" (BarCodeWidget, reference picture 18): the kind, the content in
/// three tabs (raw, WI-FI, contact), the evaluated text, input options, the kind's error
/// correction, Cancel and OK. Changes show on the canvas at once; Cancel puts back what was there
/// (and takes away a code just made), OK keeps it and remembers the options for the next code.
pub fn window(app: &mut crate::app::RayEtch, ctx: &egui::Context) {
    use egui::{RichText, vec2};
    let Some(mut d) = app.barcode_dialog.take() else { return };
    if !app.doc.shapes.iter().any(|s| s.id == d.shape) {
        return;
    }
    let before = d.spec.clone();
    let mut open = true;
    let mut choice: Option<bool> = None;
    let err = encode(&d.spec).err();
    crate::theme::centered(ctx, "barcode", egui::Window::new(tr("Proprietà codice a barre")).open(&mut open).resizable(false).collapsible(false)).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.horizontal(|ui| {
            ui.label(tr("Tipo di codice a barre"));
            egui::ComboBox::from_id_salt("bc_kind").width(300.0).selected_text(d.spec.kind().name).height(600.0).show_ui(ui, |ui| {
                for (i, k) in KINDS.iter().enumerate() {
                    if ui.selectable_label(d.spec.kind == i, k.name).clicked() {
                        d.spec.kind = i;
                        d.spec.ecc = d.spec.ecc.min(k.ecc.len().saturating_sub(1));
                    }
                }
            });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            for (i, l) in [tr("Contenuto grezzo"), tr("WI-FI"), tr("Contatto")].into_iter().enumerate() {
                if ui.selectable_label(d.tab == i, RichText::new(l).size(14.0)).clicked() {
                    d.tab = i;
                }
            }
        });
        egui::Frame::new().fill(crate::theme::p().field).corner_radius(4.0).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            match d.tab {
                1 => {
                    let mut w = d.wifi.clone();
                    egui::Grid::new("bc_wifi").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                        ui.label(tr("Nome rete (SSID)"));
                        ui.add(egui::TextEdit::singleline(&mut w.ssid).desired_width(320.0));
                        ui.end_row();
                        ui.label(tr("Password"));
                        ui.add(egui::TextEdit::singleline(&mut w.password).desired_width(320.0));
                        ui.end_row();
                        ui.label(tr("Autenticazione"));
                        let names = ["WPA/WPA2", "WEP", tr("Nessuna")];
                        egui::ComboBox::from_id_salt("bc_auth").width(160.0).selected_text(names[w.auth.min(2)]).show_ui(ui, |ui| {
                            for (i, n) in names.iter().enumerate() {
                                ui.selectable_value(&mut w.auth, i, *n);
                            }
                        });
                        ui.end_row();
                        ui.label("");
                        ui.checkbox(&mut w.hidden, tr("Rete nascosta"));
                        ui.end_row();
                    });
                    if w != d.wifi {
                        d.spec.content = w.content();
                        d.wifi = w;
                    }
                }
                2 => {
                    let mut k = d.contact.clone();
                    egui::Grid::new("bc_contact").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                        for (l, f) in [
                            (tr("Nome"), &mut k.first),
                            (tr("Cognome"), &mut k.last),
                            (tr("Indirizzo"), &mut k.address),
                            (tr("Telefono"), &mut k.phone),
                            (tr("Email"), &mut k.email),
                            (tr("Nota"), &mut k.note),
                            (tr("URL"), &mut k.url),
                        ] {
                            ui.label(l);
                            ui.add(egui::TextEdit::singleline(f).desired_width(360.0));
                            ui.end_row();
                        }
                    });
                    if k != d.contact {
                        d.spec.content = k.content();
                        d.contact = k;
                    }
                }
                _ => {
                    let max = d.spec.kind().max_chars;
                    let resp = ui.add(egui::TextEdit::multiline(&mut d.spec.content).desired_rows(7).desired_width(f32::INFINITY).char_limit(max));
                    if resp.changed() {
                        d.wifi = Wifi::parse(&d.spec.content).unwrap_or_default();
                        d.contact = Contact::parse(&d.spec.content).unwrap_or_default();
                    }
                    // UpdateCharactersRemaining (0x1008157b0): characters left / most allowed
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        ui.label(format!("{} / {}", max.saturating_sub(d.spec.content.chars().count()), max));
                    });
                }
            }
            ui.add_space(6.0);
            ui.label(tr("Testo valutato"));
            egui::Frame::new().fill(crate::theme::p().panel).corner_radius(3.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(40.0);
                ui.label(RichText::new(evaluated(&d.spec)).monospace());
            });
            if let Some(e) = &err {
                ui.label(RichText::new(e.as_str()).color(egui::Color32::from_rgb(230, 90, 80)));
            }
            ui.add_space(6.0);
            egui::Grid::new("bc_opts").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                // "Text type for variable text serial numbers, dates, or CSV merge feature"
                ui.label(tr("Valutazione del testo"));
                let names = crate::vartext::EVAL_TYPES;
                egui::ComboBox::from_id_salt("bc_eval").width(300.0).selected_text(tr(names[d.spec.eval.min(names.len() - 1)])).show_ui(ui, |ui| {
                    for (i, n) in names.iter().enumerate() {
                        ui.selectable_value(&mut d.spec.eval, i, tr(n));
                    }
                })
                .response
                .on_hover_text(tr("Testo variabile: numeri di serie (d, h, H; es. SN-0000d), data e ora (es. yyyy-MM-dd hh:mm) o unione CSV (%0, %1… = colonne della riga attuale). Il contatore si imposta nella scheda Testo variabile."));
                ui.end_row();
                ui.label(tr("Offset variabile:"));
                ui.add_enabled(d.spec.eval != crate::vartext::EVAL_NORMAL, egui::DragValue::new(&mut d.spec.var_offset).speed(0.2).range(-100000..=100000));
                ui.end_row();
                ui.label(tr("Modalità di input"));
                egui::ComboBox::from_id_salt("bc_input").width(300.0).selected_text(tr(INPUT_MODES[d.spec.input_mode.min(2)].1)).show_ui(ui, |ui| {
                    for (i, (_, n)) in INPUT_MODES.iter().enumerate() {
                        ui.selectable_value(&mut d.spec.input_mode, i, tr(n));
                    }
                });
                ui.end_row();
            });
            ui.add_space(4.0);
            ui.add_enabled_ui(d.spec.input_mode != GS1_INPUT, |ui| {
                crate::panels::switch(ui, &mut d.spec.control_chars, tr("Elabora caratteri di controllo"))
                    .on_hover_text(tr("Converte i caratteri di controllo (<RS>, <GS>, ecc.) nei valori ASCII. Non si applica con la modalità di input GS1."));
            });
            if !d.spec.kind().matrix {
                crate::panels::switch(ui, &mut d.spec.show_text, tr("Mostra testo"));
            }
            let steps = d.spec.kind().ecc;
            if !steps.is_empty() {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Error Correction Level").strong());
                    let mut v = d.spec.ecc.min(steps.len() - 1);
                    ui.add_space(20.0);
                    if ui.add(egui::Slider::new(&mut v, 0..=steps.len() - 1).show_value(false)).changed() {
                        d.spec.ecc = v;
                    }
                    ui.label(steps[v].1);
                });
            }
        });
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(egui::Button::new(RichText::new(tr("OK")).strong()).min_size(vec2(110.0, 28.0))).clicked() {
                choice = Some(true);
            }
            if ui.add(egui::Button::new(crate::i18n::cancel()).min_size(vec2(110.0, 28.0))).clicked() {
                choice = Some(false);
            }
        });
    });
    if !open && choice.is_none() {
        choice = Some(false);
    }
    if d.spec != before {
        apply(app, d.shape, &d.spec);
    }
    match choice {
        Some(true) => {
            remember(&d.spec);
            if !d.created && d.spec != d.original {
                // one undo step back to the code as it was (UndoChangeBarCodeSettings)
                let new = d.spec.clone();
                apply(app, d.shape, &d.original);
                app.checkpoint_now();
                apply(app, d.shape, &new);
            }
            app.dirty = true;
        }
        Some(false) => {
            if d.created {
                // CreateBarCode_CallBack(0): Undo, then PurgeRedo
                app.doc.shapes.retain(|s| s.id != d.shape);
                app.undo.pop();
                app.selection.retain(|x| *x != d.shape);
            } else {
                apply(app, d.shape, &d.original);
            }
        }
        None => app.barcode_dialog = Some(d),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wifi_and_contact_contents() {
        let w = Wifi { ssid: "Casa".into(), password: "segreta".into(), auth: 0, hidden: true };
        assert_eq!(w.content(), "WIFI:S:Casa;P:segreta;T:WPA;H:true;;");
        assert_eq!(Wifi::parse(&w.content()), Some(w));
        let open = Wifi { ssid: "Ospiti".into(), password: "x".into(), auth: 2, hidden: false };
        assert_eq!(open.content(), "WIFI:S:Ospiti;T:nopass;;");
        let c = Contact { first: "Mario".into(), last: "Rossi".into(), phone: "123".into(), ..Default::default() };
        assert_eq!(c.content(), "MECARD:N:Rossi,Mario;TEL:123;;");
        assert_eq!(Contact::parse(&c.content()), Some(c));
    }

    #[test]
    fn every_kind_encodes_a_sample() {
        let samples = [
            "12345", "12345", "12345", "3", "3", "3", "3", "3", "3", "RayEtch", "RayEtch", "A12345B", "RayEtch-128", "[01]09501101530003", "RAYETCH93", "RayEtch", "590123412345",
            "RayEtch", "9780306406157", "RayEtch", "RayEtch", "RayEtch", "1234ABCD", "https://example.com", "12345", "12345", "01234567890", "0123456", "1M8GDM9AXKP042788",
        ];
        assert_eq!(samples.len(), KINDS.len());
        for (i, k) in KINDS.iter().enumerate() {
            let mut s = BarcodeSpec::new(40.0, 20.0);
            s.kind = i;
            s.content = samples[i].into();
            let m = encode(&s).unwrap_or_else(|e| panic!("{}: {e}", k.name));
            assert!(m.rows > 0 && m.width > 0 && m.dark.iter().any(|d| *d), "{}", k.name);
            let o = outline(&s).unwrap();
            assert!(!o.is_empty(), "{}", k.name);
        }
    }

    #[test]
    fn qr_is_square_and_markers_have_a_border() {
        let mut s = BarcodeSpec::new(30.0, 10.0);
        s.content = "RayEtch".into();
        let m = encode(&s).unwrap();
        assert_eq!(m.rows, m.width);
        assert_eq!(m.rows, 21);
        s.kind = 6; // AprilTag 36h11
        s.content = "0".into();
        let m = encode(&s).unwrap();
        assert_eq!((m.rows, m.width), (8, 8));
        assert!(m.dark[..8].iter().all(|d| *d));
        s.content = "587".into();
        assert!(encode(&s).is_err());
    }
}
