//! Variable text ("Valutazione del testo"), as the original's Core_VariableText and
//! VariableTextManager: a code's content can be a date/time pattern, a serial number pattern or
//! a CSV merge pattern, filled in from one project-wide counter ("Attuale") plus the code's own
//! "Offset variabile" (BarCodeShape::GetEvaluatedString 0x1006a657c).

use serde::{Deserialize, Serialize};
use std::sync::RwLock;

/// How a content is read (TEXT_EVAL_TYPE; the combo's order in BarCodeWidget 0x10085d864).
pub const EVAL_TYPES: [&str; 4] = ["Normale", "Data/Ora", "Unione/CSV", "Numero di serie"];
pub const EVAL_NORMAL: usize = 0;
pub const EVAL_DATE: usize = 1;
pub const EVAL_MERGE: usize = 2;
pub const EVAL_SERIAL: usize = 3;

/// The project's counter and merge file (VariableTextManager: current +0x34, start +0x38,
/// end +0x3c, increment +0x40; Reset loads 0, 0, 999, 1 from 0x1018f8400; auto-advance off).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct VarText {
    pub current: i64,
    pub start: i64,
    pub end: i64,
    pub increment: i64,
    pub auto_advance: bool,
    /// the CSV file's name, for showing, and its rows of fields
    pub csv_name: String,
    pub csv: Vec<Vec<String>>,
}

impl Default for VarText {
    fn default() -> Self {
        VarText { current: 0, start: 0, end: 999, increment: 1, auto_advance: false, csv_name: String::new(), csv: Vec::new() }
    }
}

impl VarText {
    /// NextPage (0x1002c7388): current += increment; past the end it goes back to the start.
    pub fn next(&mut self) {
        self.current += self.increment.max(1);
        if self.current > self.end {
            self.current = self.start;
        }
    }

    /// PrevPage (0x1002c73ac): current −= increment; below the start it goes to the last value
    /// the increments reach before the end.
    pub fn prev(&mut self) {
        let inc = self.increment.max(1);
        self.current -= inc;
        if self.current < self.start {
            self.current = ((self.end - self.start + 1) / inc - 1) * inc;
        }
    }

    /// FirstPage (0x1002c737c): back to the start.
    pub fn first(&mut self) {
        self.current = self.start;
    }
}

static CURRENT: RwLock<Option<VarText>> = RwLock::new(None);

/// Make `v` the state the codes are evaluated with (the open project's).
pub fn set(v: &VarText) {
    if let Ok(mut g) = CURRENT.write() {
        *g = Some(v.clone());
    }
}

/// Keep the evaluation state in step with the open project, cheaply: it is copied only when the
/// counter, the range or the merge file changed (a new project, undo, the panel).
pub fn sync(v: &VarText) {
    let key = |v: &VarText| (v.current, v.start, v.end, v.increment, v.csv.len(), v.csv_name.clone());
    let same = CURRENT.read().ok().is_some_and(|g| g.as_ref().is_some_and(|c| key(c) == key(v)));
    if !same {
        set(v);
    }
}

fn with<R>(f: impl FnOnce(&VarText) -> R) -> R {
    let g = CURRENT.read().ok();
    match g.as_ref().and_then(|g| g.as_ref()) {
        Some(v) => f(v),
        None => f(&VarText::default()),
    }
}

/// The content as it will be encoded: evaluated by its type with the counter + `offset`.
/// Whether a text already holds codes for a variable type (a date pattern, a serial run, a CSV
/// column), so switching to that type keeps it.
pub fn looks_like_pattern(text: &str, eval: usize) -> bool {
    match eval {
        EVAL_DATE => ["yy", "MM", "dd", "hh", "HH", "mm", "ss"].iter().any(|c| text.contains(c)),
        EVAL_SERIAL => {
            let c: Vec<char> = text.chars().collect();
            c.windows(2).any(|w| (w[0] == '0' || w[0] == w[1]) && matches!(w[1], 'd' | 'h' | 'H'))
        }
        EVAL_MERGE => text.as_bytes().windows(2).any(|w| w[0] == b'%' && w[1].is_ascii_digit()),
        _ => true,
    }
}

/// The pattern a text starts from when it becomes Data/Ora, Numero di serie or Unione/CSV.
pub fn starter_pattern(eval: usize) -> &'static str {
    match eval {
        EVAL_DATE => "dd/MM/yyyy",
        EVAL_SERIAL => "000d",
        EVAL_MERGE => "%0",
        _ => "",
    }
}

pub fn evaluate(content: &str, eval: usize, offset: i64) -> String {
    match eval {
        EVAL_DATE => format_datetime(content, &chrono::Local::now()),
        EVAL_MERGE => with(|v| format_merge(content, v.current + offset, &v.csv)),
        EVAL_SERIAL => with(|v| format_serial(content, v.current + offset)),
        _ => content.to_string(),
    }
}

/// Core_VariableText::FormatSerial (0x10071c510): `d` a decimal digit, `h`/`H` a lower/upper
/// case hex digit; a run of them is one number that many digits wide (the last digits kept
/// when it is longer), zeros just before the run pad it with zeros; text in single quotes stays
/// as written ('' is a quote); a second number in one pattern is "bad serial format".
pub fn format_serial(fmt: &str, n: i64) -> String {
    let c: Vec<char> = fmt.chars().collect();
    let mut out = String::new();
    let mut done = false;
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch == '\'' {
            let mut j = i + 1;
            while j < c.len() && c[j] != '\'' {
                j += 1;
            }
            if j == i + 1 {
                out.push('\'');
            } else {
                out.extend(&c[i + 1..j]);
            }
            i = j + 1;
            continue;
        }
        // zeros: padding when a number run follows, plain zeros otherwise
        let mut zeros = 0;
        while i + zeros < c.len() && c[i + zeros] == '0' {
            zeros += 1;
        }
        let k = i + zeros;
        if k < c.len() && matches!(c[k], 'd' | 'h' | 'H') {
            if done {
                return "bad serial format".into();
            }
            let f = c[k];
            let mut run = 0;
            while k + run < c.len() && c[k + run] == f {
                run += 1;
            }
            let width = zeros + run;
            let mut s = match f {
                'd' => n.to_string(),
                'h' => format!("{:x}", n),
                _ => format!("{:X}", n),
            };
            if s.len() > width {
                s = s[s.len() - width..].to_string();
            } else if zeros > 0 {
                s = format!("{}{}", "0".repeat(width - s.len()), s);
            }
            out += &s;
            done = true;
            i = k + run;
            continue;
        }
        if zeros > 0 {
            out += &"0".repeat(zeros);
            i = k;
            continue;
        }
        out.push(ch);
        i += 1;
    }
    out
}

/// Core_VariableText::FormatMerge (0x10071ca0c): `%n` is field n (counted from 0) of CSV row
/// `row` (VariableTextManager::GetString: empty when out of range), `%%` a percent sign; a
/// trailing line break of a field is dropped.
pub fn format_merge(fmt: &str, row: i64, csv: &[Vec<String>]) -> String {
    let c: Vec<char> = fmt.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '%' {
            if i + 1 < c.len() && c[i + 1] == '%' {
                out.push('%');
                i += 2;
                continue;
            }
            let mut j = i + 1;
            while j < c.len() && c[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 {
                let col: usize = c[i + 1..j].iter().collect::<String>().parse().unwrap_or(usize::MAX);
                let field = (row >= 0).then(|| csv.get(row as usize).and_then(|r| r.get(col))).flatten().cloned().unwrap_or_default();
                out += field.strip_suffix('\n').unwrap_or(&field);
                i = j;
                continue;
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// Core_VariableText::FormatDateTime (0x10071c174): Qt's date/time pattern (yyyy, yy, MMMM,
/// MMM, MM, M, dddd, ddd, dd, d, HH, H, hh, h, mm, m, ss, s, zzz, z, AP/ap), plus `w` the week
/// number and `W` year/week; text in single quotes stays as written.
pub fn format_datetime(fmt: &str, t: &chrono::DateTime<chrono::Local>) -> String {
    use chrono::{Datelike, Timelike};
    let it = crate::i18n::current() == crate::i18n::Lang::It;
    let months_it = ["gennaio", "febbraio", "marzo", "aprile", "maggio", "giugno", "luglio", "agosto", "settembre", "ottobre", "novembre", "dicembre"];
    let months_en = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    let days_it = ["lunedì", "martedì", "mercoledì", "giovedì", "venerdì", "sabato", "domenica"];
    let days_en = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
    let month = if it { months_it } else { months_en }[t.month0() as usize];
    let day = if it { days_it } else { days_en }[t.weekday().num_days_from_monday() as usize];
    let short = |s: &str| s.chars().take(3).collect::<String>();
    let c: Vec<char> = fmt.chars().collect();
    let ampm = fmt.contains("AP") || fmt.contains("ap") || fmt.contains('A') || fmt.contains('a');
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch == '\'' {
            let mut j = i + 1;
            while j < c.len() && c[j] != '\'' {
                j += 1;
            }
            if j == i + 1 {
                out.push('\'');
            } else {
                out.extend(&c[i + 1..j]);
            }
            i = j + 1;
            continue;
        }
        let mut run = 1;
        while i + run < c.len() && c[i + run] == ch {
            run += 1;
        }
        let h12 = { let h = t.hour() % 12; if h == 0 { 12 } else { h } };
        let s = match (ch, run) {
            ('y', 4..) => format!("{:04}", t.year()),
            ('y', 2..=3) => format!("{:02}", t.year() % 100),
            ('M', 4..) => month.to_string(),
            ('M', 3) => short(month),
            ('M', 2) => format!("{:02}", t.month()),
            ('M', 1) => t.month().to_string(),
            ('d', 4..) => day.to_string(),
            ('d', 3) => short(day),
            ('d', 2) => format!("{:02}", t.day()),
            ('d', 1) => t.day().to_string(),
            ('H', 2..) => format!("{:02}", t.hour()),
            ('H', 1) => t.hour().to_string(),
            ('h', 2..) => format!("{:02}", if ampm { h12 } else { t.hour() }),
            ('h', 1) => (if ampm { h12 } else { t.hour() }).to_string(),
            ('m', 2..) => format!("{:02}", t.minute()),
            ('m', 1) => t.minute().to_string(),
            ('s', 2..) => format!("{:02}", t.second()),
            ('s', 1) => t.second().to_string(),
            ('z', 3..) => format!("{:03}", t.timestamp_subsec_millis()),
            ('z', _) => t.timestamp_subsec_millis().to_string(),
            ('A', _) => (if t.hour() < 12 { "AM" } else { "PM" }).to_string(),
            ('a', _) => (if t.hour() < 12 { "am" } else { "pm" }).to_string(),
            ('P' | 'p', _) if i > 0 && matches!(c[i - 1], 'A' | 'a') => String::new(),
            ('w', _) => t.iso_week().week().to_string().repeat(run),
            ('W', _) => format!("{}/{}", t.iso_week().year(), t.iso_week().week()).repeat(run),
            _ => std::iter::repeat_n(ch, run).collect(),
        };
        out += &s;
        i += run;
    }
    out
}

/// "Elabora caratteri di controllo": <NUL>…<US>, <DEL> become those ASCII characters
/// (GetEvaluatedString: names of up to four letters between < and >; off with GS1 input).
pub fn control_chars(s: &str) -> String {
    const NAMES: [&str; 32] = [
        "NUL", "SOH", "STX", "ETX", "EOT", "ENQ", "ACK", "BEL", "BS", "HT", "LF", "VT", "FF", "CR", "SO", "SI", "DLE", "DC1", "DC2", "DC3", "DC4", "NAK", "SYN", "ETB", "CAN", "EM", "SUB", "ESC", "FS", "GS", "RS", "US",
    ];
    let mut out = String::new();
    let mut rest = s;
    while let Some(p) = rest.find('<') {
        out += &rest[..p];
        let after = &rest[p + 1..];
        let code = after.find('>').filter(|e| *e <= 4).and_then(|e| {
            let name = &after[..e];
            NAMES.iter().position(|n| *n == name).map(|i| (i as u8 as char, e)).or_else(|| (name == "DEL").then_some(('\u{7f}', e)))
        });
        match code {
            Some((ch, e)) => {
                out.push(ch);
                rest = &after[e + 1..];
            }
            None => {
                out.push('<');
                rest = after;
            }
        }
    }
    out += rest;
    out
}

/// Split a CSV file into rows of fields: commas, semicolons or tabs (whichever the first line
/// uses most), double quotes around fields that hold them (VariableTextManager::LoadCSV /
/// SplitFileContent; the separator guess is RayEtch's).
pub fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let first = text.lines().next().unwrap_or("");
    let sep = [',', ';', '\t'].into_iter().max_by_key(|s| first.matches(*s).count()).unwrap_or(',');
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut it = text.chars().peekable();
    while let Some(ch) = it.next() {
        if quoted {
            if ch == '"' {
                if it.peek() == Some(&'"') {
                    field.push('"');
                    it.next();
                } else {
                    quoted = false;
                }
            } else {
                field.push(ch);
            }
        } else if ch == '"' {
            quoted = true;
        } else if ch == sep {
            row.push(std::mem::take(&mut field));
        } else if ch == '\n' || ch == '\r' {
            if ch == '\r' && it.peek() == Some(&'\n') {
                it.next();
            }
            row.push(std::mem::take(&mut field));
            if !(row.len() == 1 && row[0].is_empty()) {
                rows.push(std::mem::take(&mut row));
            } else {
                row.clear();
            }
        } else {
            field.push(ch);
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// The "Testo variabile" tab (VariableTextWidget 0x1008cbacc): Attuale, Inizio, Fine,
/// Incremento, Avanzamento automatico, Precedente / Successivo / Reimposta, and the CSV file.
pub fn panel(app: &mut crate::app::RayEtch, ui: &mut eframe::egui::Ui) {
    use crate::i18n::tr;
    use eframe::egui;
    let before = app.doc.vars.clone();
    let v = &mut app.doc.vars;
    egui::Grid::new("vt_grid").num_columns(2).spacing(egui::vec2(12.0, 6.0)).show(ui, |ui| {
        ui.label(tr("Attuale"));
        ui.add(egui::DragValue::new(&mut v.current).speed(0.2));
        ui.end_row();
        ui.label(tr("Inizio"));
        ui.add(egui::DragValue::new(&mut v.start).speed(0.2));
        ui.end_row();
        ui.label(tr("Fine"));
        ui.add(egui::DragValue::new(&mut v.end).speed(0.2));
        ui.end_row();
        ui.label(tr("Incremento"));
        ui.add(egui::DragValue::new(&mut v.increment).speed(0.1).range(1..=100000));
        ui.end_row();
    });
    ui.add_space(4.0);
    crate::panels::switch(ui, &mut v.auto_advance, tr("Avanzamento automatico"))
        .on_hover_text(tr("Dopo ogni lavoro inviato al laser, Attuale avanza di Incremento; oltre Fine torna a Inizio."));
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui.button(tr("◀ Precedente")).clicked() {
            v.prev();
        }
        if ui.button(tr("Successivo ▶")).clicked() {
            v.next();
        }
        if ui.button(tr("Reimposta")).on_hover_text(tr("Riporta Attuale a Inizio")).clicked() {
            v.first();
        }
    });
    ui.add_space(8.0);
    ui.label(egui::RichText::new(tr("File CSV per l'unione")).strong());
    ui.horizontal(|ui| {
        if ui.button(tr("Sfoglia…")).clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("CSV", &["csv", "txt"]).pick_file() {
                match std::fs::read(&p) {
                    Ok(bytes) => {
                        v.csv = parse_csv(&String::from_utf8_lossy(&bytes));
                        v.csv_name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        // the counter runs over the file's rows
                        v.start = 0;
                        v.end = (v.csv.len() as i64 - 1).max(0);
                        v.current = v.current.clamp(v.start, v.end);
                    }
                    Err(e) => app.status = crate::i18n::trf("Impossibile leggere il file: {}", &[&e]),
                }
            }
        }
        if ui.add_enabled(!v.csv.is_empty(), egui::Button::new(tr("Svuota"))).clicked() {
            v.csv.clear();
            v.csv_name.clear();
        }
    });
    if v.csv.is_empty() {
        ui.label(egui::RichText::new(tr("Nessun file caricato")).color(crate::theme::p().text_dim));
    } else {
        ui.label(crate::i18n::trf("{}: {} righe", &[&v.csv_name, &v.csv.len()]));
        if let Some(row) = usize::try_from(v.current).ok().and_then(|r| v.csv.get(r)) {
            let shown: Vec<String> = row.iter().enumerate().map(|(i, f)| format!("%{i} = {f}")).collect();
            ui.label(egui::RichText::new(shown.join("   ")).monospace().color(crate::theme::p().text_dim));
        }
    }
    ui.add_space(8.0);
    ui.label(egui::RichText::new(tr("Serie: d = cifra, h/H = esadecimale, 0 = riempi con zeri (es. SN-0000d). Data/Ora: yyyy MM dd hh mm ss, 'testo' tra apici. CSV: %0, %1… colonne della riga Attuale.")).small().color(crate::theme::p().text_dim));
    if app.doc.vars != before {
        app.dirty = true;
        crate::barcode::refresh_variable(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_patterns() {
        assert_eq!(format_serial("SN-0000d", 42), "SN-00042");
        assert_eq!(format_serial("ddd", 7), "7");
        assert_eq!(format_serial("dd", 1234), "34");
        assert_eq!(format_serial("0HH", 255), "0FF");
        assert_eq!(format_serial("'d'-0hh", 10), "d-00a");
        assert_eq!(format_serial("d d", 1), "bad serial format");
        assert_eq!(format_serial("Lotto 0", 5), "Lotto 0");
    }

    #[test]
    fn merge_and_counter() {
        let csv = parse_csv("nome;codice\nAnna;111\n\"Rossi; Bo\";222\n");
        assert_eq!(csv.len(), 3);
        assert_eq!(format_merge("%0 - %1 (100%%)", 2, &csv), "Rossi; Bo - 222 (100%)");
        assert_eq!(format_merge("%5", 1, &csv), "");
        let mut v = VarText { end: 3, ..Default::default() };
        v.next();
        v.next();
        v.next();
        assert_eq!(v.current, 3);
        v.next();
        assert_eq!(v.current, 0);
    }

    #[test]
    fn dates_and_control_chars() {
        use chrono::TimeZone;
        let t = chrono::Local.with_ymd_and_hms(2026, 3, 7, 9, 5, 2).unwrap();
        assert_eq!(format_datetime("yyyy-MM-dd hh:mm:ss", &t), "2026-03-07 09:05:02");
        assert_eq!(format_datetime("'Lotto' yy.M.d", &t), "Lotto 26.3.7");
        assert_eq!(control_chars("A<RS>B<GS>C<XYZ>"), "A\u{1e}B\u{1d}C<XYZ>");
    }
}

#[cfg(test)]
mod pattern_tests {
    use super::*;

    /// A plain word isn't a pattern (it gets a starter one); real patterns are kept.
    #[test]
    fn starter_only_for_plain_text() {
        assert!(!looks_like_pattern("RayEtch", EVAL_DATE));
        assert!(looks_like_pattern("'Lotto' dd/MM/yyyy", EVAL_DATE));
        assert!(!looks_like_pattern("RayEtch", EVAL_SERIAL));
        assert!(looks_like_pattern("SN-0000d", EVAL_SERIAL));
        assert!(!looks_like_pattern("Nome", EVAL_MERGE));
        assert!(looks_like_pattern("%0 %1", EVAL_MERGE));
        assert_eq!(format_serial(starter_pattern(EVAL_SERIAL), 7), "0007");
    }
}
