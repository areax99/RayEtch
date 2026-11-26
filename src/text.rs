//! Text to outlines using TrueType fonts (built-in fonts plus system fonts on macOS).

use crate::model::{P2, Polyline};
use std::sync::{Arc, OnceLock};

pub struct FontEntry {
    pub name: String,
    /// Where a system font lives; read only when the font is first used.
    path: Option<std::path::PathBuf>,
    bytes: OnceLock<Option<Arc<Vec<u8>>>>,
}

impl FontEntry {
    fn built_in(name: &str, bytes: &[u8]) -> Self {
        let cell = OnceLock::new();
        let _ = cell.set(Some(Arc::new(bytes.to_vec())));
        FontEntry { name: name.into(), path: None, bytes: cell }
    }

    /// The font file's bytes, read the first time they're needed (None when it can't be read
    /// or isn't a font).
    pub fn data(&self) -> Option<Arc<Vec<u8>>> {
        self.bytes
            .get_or_init(|| {
                let bytes = std::fs::read(self.path.as_ref()?).ok()?;
                ttf_parser::Face::parse(&bytes, 0).ok()?;
                Some(Arc::new(bytes))
            })
            .clone()
    }
}

/// The fonts offered for text: the two built in, then the system's by file name. Only the
/// names are listed here; a system font's file is read when it is first used. Windows keeps
/// hundreds of large fonts, and reading them all while the window opened kept an old PC
/// from drawing or answering for a long time.
pub fn fonts() -> &'static Vec<FontEntry> {
    static FONTS: OnceLock<Vec<FontEntry>> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut v = vec![
            FontEntry::built_in("Sans (built-in)", epaint_default_fonts::UBUNTU_LIGHT),
            FontEntry::built_in("Mono (built-in)", epaint_default_fonts::HACK_REGULAR),
        ];
        let dirs = ["/System/Library/Fonts/Supplemental", "/Library/Fonts", "/usr/share/fonts/truetype", "C:\\Windows\\Fonts"];
        let mut found: Vec<(String, std::path::PathBuf)> = Vec::new();
        for d in dirs {
            if let Ok(rd) = std::fs::read_dir(d) {
                for e in rd.flatten() {
                    let p = e.path();
                    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                    if ext == "ttf" || ext == "otf" {
                        if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                            found.push((stem.to_string(), p.clone()));
                        }
                    }
                }
            }
        }
        found.sort();
        found.dedup_by(|a, b| a.0 == b.0);
        for (name, path) in found.into_iter().take(200) {
            v.push(FontEntry { name, path: Some(path), bytes: OnceLock::new() });
        }
        v
    })
}

struct Builder {
    paths: Vec<Polyline>,
    cur: Vec<P2>,
    last: P2,
    scale: f64,
    ox: f64,
    oy: f64,
}

impl Builder {
    fn pt(&self, x: f32, y: f32) -> P2 {
        [self.ox + x as f64 * self.scale, self.oy + y as f64 * self.scale]
    }
    fn flush(&mut self, closed: bool) {
        if self.cur.len() > 1 {
            let mut pts = std::mem::take(&mut self.cur);
            if closed && pts.len() > 2 {
                let f = pts[0];
                let l = *pts.last().unwrap();
                if (f[0] - l[0]).abs() < 1e-9 && (f[1] - l[1]).abs() < 1e-9 {
                    pts.pop();
                }
            }
            self.paths.push(Polyline { pts, closed });
        }
        self.cur.clear();
    }
}

impl ttf_parser::OutlineBuilder for Builder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.flush(true);
        let p = self.pt(x, y);
        self.cur.push(p);
        self.last = p;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.pt(x, y);
        self.cur.push(p);
        self.last = p;
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let c = self.pt(x1, y1);
        let e = self.pt(x, y);
        let s = self.last;
        // "Qualità della curva" (Impostazioni > Schermo): pieces per curve
        let n = crate::prefs::curve_segments(8);
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            self.cur.push([u * u * s[0] + 2.0 * u * t * c[0] + t * t * e[0], u * u * s[1] + 2.0 * u * t * c[1] + t * t * e[1]]);
        }
        self.last = e;
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let c1 = self.pt(x1, y1);
        let c2 = self.pt(x2, y2);
        let e = self.pt(x, y);
        let s = self.last;
        let n = crate::prefs::curve_segments(10);
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            let a = u * u * u;
            let b = 3.0 * u * u * t;
            let cc = 3.0 * u * t * t;
            let d = t * t * t;
            self.cur.push([a * s[0] + b * c1[0] + cc * c2[0] + d * e[0], a * s[1] + b * c1[1] + cc * c2[1] + d * e[1]]);
        }
        self.last = e;
    }
    fn close(&mut self) {
        self.flush(true);
    }
}

/// Fonts draw some letters (a "1", accented letters) from pieces that overlap; filled even-odd,
/// the overlap would come out empty. Merge one glyph's pieces by the font's own non-zero rule
/// into clean outlines and holes.
fn merge_glyph(paths: &mut Vec<Polyline>, first: usize) {
    use i_overlay::core::fill_rule::FillRule;
    use i_overlay::float::simplify::SimplifyShape;
    if paths.len() - first < 2 {
        return;
    }
    let contours: Vec<Vec<[f64; 2]>> = paths[first..].iter().filter(|p| p.closed && p.pts.len() >= 3).map(|p| p.pts.clone()).collect();
    if contours.len() != paths.len() - first {
        return;
    }
    let merged = contours.simplify_shape(FillRule::NonZero);
    if merged.is_empty() {
        return;
    }
    paths.truncate(first);
    for shape in merged {
        for c in shape {
            paths.push(Polyline { pts: c, closed: true });
        }
    }
}

/// Letters of the scripts written right to left.
fn is_rtl(c: char) -> bool {
    matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF)
}

/// Build outlines (mm, Y up) centred on the origin. `size` is the cap height-ish em size in mm.
/// Each letter of one line on its own: (where it starts along the line, its advance, its outlines
/// with the start at x = 0 and the baseline at y = 0), mm. For "Applica tracciato al testo".
pub fn glyphs(text: &str, size: f64, font_name: &str) -> Vec<(f64, f64, Vec<Polyline>)> {
    let list = fonts();
    let data = list.iter().find(|f| f.name == font_name).and_then(|f| f.data()).or_else(|| list[0].data());
    let Some(data) = data else { return Vec::new() };
    let Ok(face) = ttf_parser::Face::parse(&data, 0) else { return Vec::new() };
    let scale = size / face.units_per_em() as f64;
    let mut out = Vec::new();
    let mut x = 0.0;
    for ch in text.chars().filter(|c| *c != '\n') {
        let Some(gid) = face.glyph_index(ch) else {
            x += size * 0.5;
            continue;
        };
        let mut b = Builder { paths: Vec::new(), cur: Vec::new(), last: [0.0, 0.0], scale, ox: 0.0, oy: 0.0 };
        face.outline_glyph(gid, &mut b);
        b.flush(true);
        merge_glyph(&mut b.paths, 0);
        let adv = face.glyph_hor_advance(gid).unwrap_or(0) as f64 * scale;
        out.push((x, adv, b.paths));
        x += adv;
    }
    out
}

/// The letters laid out from the first line's baseline (not yet centred), with where each
/// caret position sits on its line: (x, line), plus the line height, ascender and descender.
struct Laid {
    paths: Vec<Polyline>,
    carets: Vec<(f64, usize)>,
    line_h: f64,
    asc: f64,
    desc: f64,
}

fn lay_out(text: &str, size: f64, font_name: &str) -> Option<Laid> {
    let list = fonts();
    let data = list.iter().find(|f| f.name == font_name).and_then(|f| f.data()).or_else(|| list[0].data());
    let data = data?;
    let face = ttf_parser::Face::parse(&data, 0).ok()?;
    let upem = face.units_per_em() as f64;
    let scale = size / upem;
    let line_h = (face.ascender() as f64 - face.descender() as f64 + face.line_gap() as f64) * scale;
    let mut b = Builder { paths: Vec::new(), cur: Vec::new(), last: [0.0, 0.0], scale, ox: 0.0, oy: 0.0 };
    let rtl = crate::prefs::with(|p| p.right_to_left_text);
    let mut carets = Vec::new();
    for (li, line) in text.split('\n').enumerate() {
        let mut x = 0.0;
        let y = -(li as f64) * line_h;
        // "Abilita il supporto del testo da destra a sinistra": a line written in a right-to-left
        // script (Hebrew, Arabic, Syriac, Thaana...) is laid out from its last letter, so it reads
        // right to left
        let chars: Vec<char> = if rtl && line.chars().any(is_rtl) { line.chars().rev().collect() } else { line.chars().collect() };
        for ch in chars {
            carets.push((x, li));
            let Some(gid) = face.glyph_index(ch) else {
                x += size * 0.5;
                continue;
            };
            b.ox = x;
            b.oy = y;
            let first = b.paths.len();
            face.outline_glyph(gid, &mut b);
            b.flush(true);
            merge_glyph(&mut b.paths, first);
            x += face.glyph_hor_advance(gid).unwrap_or(0) as f64 * scale;
        }
        // after the line's last letter (the newline's own position)
        carets.push((x, li));
    }
    Some(Laid { paths: b.paths, carets, line_h, asc: face.ascender() as f64 * scale, desc: face.descender() as f64 * scale })
}

/// The centre the outline is shifted by (so a text's origin is its middle).
fn centre_of(paths: &[Polyline]) -> Option<P2> {
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for p in paths.iter().flat_map(|p| p.pts.iter()) {
        min[0] = min[0].min(p[0]);
        min[1] = min[1].min(p[1]);
        max[0] = max[0].max(p[0]);
        max[1] = max[1].max(p[1]);
    }
    min[0].is_finite().then(|| [(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0])
}

/// Where the typing cursor stands before the `index`-th character, in the text's own (centred)
/// coordinates: the bottom and top of a line from descender to ascender.
pub fn caret_at(text: &str, size: f64, font_name: &str, index: usize) -> Option<(P2, P2)> {
    let l = lay_out(text, size, font_name)?;
    let c = centre_of(&l.paths).unwrap_or([0.0, 0.0]);
    let (x, li) = *l.carets.get(index.min(l.carets.len().saturating_sub(1)))?;
    let y = -(li as f64) * l.line_h;
    Some(([x - c[0], y + l.desc - c[1]], [x - c[0], y + l.asc - c[1]]))
}

/// The caret position (character index) nearest a point in the text's own (centred)
/// coordinates: its line first, then the nearest gap between letters on that line.
pub fn caret_index_at(text: &str, size: f64, font_name: &str, p: P2) -> usize {
    let Some(l) = lay_out(text, size, font_name) else { return text.chars().count() };
    let c = centre_of(&l.paths).unwrap_or([0.0, 0.0]);
    let (x, y) = (p[0] + c[0], p[1] + c[1]);
    let lines = l.carets.last().map(|c| c.1 + 1).unwrap_or(1);
    // line li runs from its descender to its ascender around baseline -li·line_h
    let li = (((l.asc - y) / l.line_h).floor().max(0.0) as usize).min(lines - 1);
    l.carets
        .iter()
        .enumerate()
        .filter(|(_, c)| c.1 == li)
        .min_by(|a, b| (a.1.0 - x).abs().total_cmp(&(b.1.0 - x).abs()))
        .map(|(i, _)| i)
        .unwrap_or(text.chars().count())
}

pub fn text_outline(text: &str, size: f64, font_name: &str) -> Vec<Polyline> {
    let Some(l) = lay_out(text, size, font_name) else { return Vec::new() };
    let b = Builder { paths: l.paths, cur: Vec::new(), last: [0.0, 0.0], scale: 1.0, ox: 0.0, oy: 0.0 };
    // centre
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for p in b.paths.iter().flat_map(|p| p.pts.iter()) {
        min[0] = min[0].min(p[0]);
        min[1] = min[1].min(p[1]);
        max[0] = max[0].max(p[0]);
        max[1] = max[1].max(p[1]);
    }
    if !min[0].is_finite() {
        return Vec::new();
    }
    let c = [(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0];
    b.paths
        .into_iter()
        .map(|pl| Polyline { pts: pl.pts.into_iter().map(|p| [p[0] - c[0], p[1] - c[1]]).collect(), closed: pl.closed })
        .collect()
}

#[cfg(test)]
mod caret_tests {
    use super::*;

    /// Clicking where the cursor would stand gives back that character, on every line.
    #[test]
    fn click_finds_the_caret_it_draws() {
        let font = fonts()[0].name.clone();
        let text = "Ciao\nmondo";
        for i in 0..=text.chars().count() {
            let (a, b) = caret_at(text, 25.0, &font, i).unwrap();
            let mid = [a[0], (a[1] + b[1]) / 2.0];
            assert_eq!(caret_index_at(text, 25.0, &font, mid), i, "caret {i}");
        }
    }
}
