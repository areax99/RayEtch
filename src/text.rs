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

/// The text toolbar's options (reference picture 40; the original's Bold, Italic, Upper Case,
/// Distort, Welded, HSpace, VSpace, Align X, Align Y, the text type and Offset).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct TextStyle {
    pub bold: bool,
    pub italic: bool,
    pub upper: bool,
    /// "Distorsione": resizing stretches the letters; off, a text keeps its proportions
    pub distort: bool,
    /// "Saldato": overlapping letters are joined into one outline
    pub welded: bool,
    /// "Spaziatura orizzontale" / "Spaziatura verticale", mm added between letters / lines
    pub hspace: f64,
    pub vspace: f64,
    /// "Allinea X": 0 Sinistra, 1 Centrale, 2 Destra; "Allinea Y": 0 Alto, 1 Centrale, 2 Basso
    pub align_x: u8,
    pub align_y: u8,
    /// the text type (vartext::EVAL_TYPES: Normale, Data/Ora, Unione/CSV, Numero di serie)
    pub eval: usize,
    /// "Offset": added to the counter / CSV row for this text
    pub offset: i64,
    /// "Applica tracciato al testo": the line the letters follow (in the text's own coordinates);
    /// the text stays a text, editable, and slides along it
    #[serde(default)]
    pub path: Option<Polyline>,
    /// how far along the path the text is moved, mm ("Offset" while on a path; dragging the text)
    #[serde(default)]
    pub path_offset: f64,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle { bold: false, italic: false, upper: false, distort: false, welded: false, hspace: 0.0, vspace: 0.0, align_x: 1, align_y: 1, eval: 0, offset: 0, path: None, path_offset: 0.0 }
    }
}

/// Faux italic: letters lean right by 12° (fonts are drawn from their regular face).
const ITALIC_SLANT: f64 = 0.2126;
/// Faux bold: each letter grows by this share of the text height.
const BOLD_GROW: f64 = 0.03;

/// The text a shape shows: upper case and its variable-text value applied.
pub fn shown_text(text: &str, style: &TextStyle) -> String {
    let t = if style.eval != 0 { crate::vartext::evaluate(text, style.eval, style.offset) } else { text.to_string() };
    if style.upper { t.to_uppercase() } else { t }
}

/// The letters laid out from the first line's baseline, with where each caret position sits on its
/// line: (x, line), each letter's outlines and line, the line height, ascender and descender.
struct Laid {
    glyphs: Vec<(usize, Vec<Polyline>)>,
    carets: Vec<(f64, usize)>,
    line_h: f64,
    asc: f64,
    desc: f64,
}

fn lay_out(text: &str, size: f64, font_name: &str, style: &TextStyle) -> Option<Laid> {
    let list = fonts();
    let data = list.iter().find(|f| f.name == font_name).and_then(|f| f.data()).or_else(|| list[0].data());
    let data = data?;
    let face = ttf_parser::Face::parse(&data, 0).ok()?;
    let upem = face.units_per_em() as f64;
    let scale = size / upem;
    let line_h = (face.ascender() as f64 - face.descender() as f64 + face.line_gap() as f64) * scale + style.vspace;
    let rtl = crate::prefs::with(|p| p.right_to_left_text);
    let mut carets = Vec::new();
    let mut glyphs = Vec::new();
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
                x += size * 0.5 + style.hspace;
                continue;
            };
            let mut b = Builder { paths: Vec::new(), cur: Vec::new(), last: [0.0, 0.0], scale, ox: x, oy: y };
            face.outline_glyph(gid, &mut b);
            b.flush(true);
            merge_glyph(&mut b.paths, 0);
            if style.italic {
                for p in b.paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
                    p[0] += (p[1] - y) * ITALIC_SLANT;
                }
            }
            if !b.paths.is_empty() {
                glyphs.push((li, b.paths));
            }
            x += face.glyph_hor_advance(gid).unwrap_or(0) as f64 * scale + style.hspace;
        }
        // after the line's last letter (the newline's own position)
        carets.push((x, li));
    }
    Some(Laid { glyphs, carets, line_h, asc: face.ascender() as f64 * scale, desc: face.descender() as f64 * scale })
}

fn bounds<'a>(paths: impl Iterator<Item = &'a Polyline>) -> Option<([f64; 2], [f64; 2])> {
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for p in paths.flat_map(|p| p.pts.iter()) {
        min[0] = min[0].min(p[0]);
        min[1] = min[1].min(p[1]);
        max[0] = max[0].max(p[0]);
        max[1] = max[1].max(p[1]);
    }
    min[0].is_finite().then_some((min, max))
}

/// How the laid-out letters are placed around the text's origin: each line's own shift along X
/// (Allinea X: its left, middle or right on the origin), then one shift for the whole block
/// (Allinea Y: the top of the first line, the block's middle, or the last line's baseline).
fn placement(l: &Laid, style: &TextStyle) -> (Vec<f64>, f64) {
    let lines = l.carets.last().map(|c| c.1 + 1).unwrap_or(1);
    let mut dx = vec![0.0; lines];
    for (li, d) in dx.iter_mut().enumerate() {
        // a line's extent: its letters, or its caret run when it has none (an empty line)
        let ext = bounds(l.glyphs.iter().filter(|g| g.0 == li).flat_map(|g| g.1.iter())).map(|(a, b)| (a[0], b[0])).unwrap_or_else(|| {
            let xs: Vec<f64> = l.carets.iter().filter(|c| c.1 == li).map(|c| c.0).collect();
            (xs.first().copied().unwrap_or(0.0), xs.last().copied().unwrap_or(0.0))
        });
        *d = match style.align_x {
            0 => -ext.0,
            2 => -ext.1,
            _ => -(ext.0 + ext.1) / 2.0,
        };
    }
    let dy = match style.align_y {
        0 => -l.asc,
        2 => (lines - 1) as f64 * l.line_h,
        _ => bounds(l.glyphs.iter().flat_map(|g| g.1.iter())).map(|(a, b)| -(a[1] + b[1]) / 2.0).unwrap_or(0.0),
    };
    (dx, dy)
}

/// Where the typing cursor stands before the `index`-th character, in the text's own coordinates:
/// the bottom and top of a line from descender to ascender.
pub fn caret_at(text: &str, size: f64, font_name: &str, style: &TextStyle, index: usize) -> Option<(P2, P2)> {
    let l = lay_out(text, size, font_name, style)?;
    let (dx, dy) = placement(&l, style);
    let (x, li) = *l.carets.get(index.min(l.carets.len().saturating_sub(1)))?;
    let y = -(li as f64) * l.line_h + dy;
    let x = x + dx[li];
    Some(([x, y + l.desc], [x, y + l.asc]))
}

/// The caret position (character index) nearest a point in the text's own coordinates: its line
/// first, then the nearest gap between letters on that line.
pub fn caret_index_at(text: &str, size: f64, font_name: &str, style: &TextStyle, p: P2) -> usize {
    let Some(l) = lay_out(text, size, font_name, style) else { return text.chars().count() };
    let (dx, dy) = placement(&l, style);
    let y = p[1] - dy;
    let lines = dx.len();
    // line li runs from its descender to its ascender around baseline -li·line_h
    let li = (((l.asc - y) / l.line_h).floor().max(0.0) as usize).min(lines - 1);
    let x = p[0] - dx[li];
    l.carets
        .iter()
        .enumerate()
        .filter(|(_, c)| c.1 == li)
        .min_by(|a, b| (a.1.0 - x).abs().total_cmp(&(b.1.0 - x).abs()))
        .map(|(i, _)| i)
        .unwrap_or(text.chars().count())
}

/// Outlines (mm, Y up) of a text with its options, placed on the origin by Allinea X / Y.
pub fn styled_outline(text: &str, size: f64, font_name: &str, style: &TextStyle) -> Vec<Polyline> {
    if let Some(path) = &style.path {
        return on_path(text, size, font_name, style, path);
    }
    let Some(l) = lay_out(text, size, font_name, style) else { return Vec::new() };
    let (dx, dy) = placement(&l, style);
    let mut letters: Vec<Vec<Polyline>> = l
        .glyphs
        .into_iter()
        .map(|(li, paths)| paths.into_iter().map(|pl| Polyline { pts: pl.pts.into_iter().map(|p| [p[0] + dx[li], p[1] + dy]).collect(), closed: pl.closed }).collect())
        .collect();
    if style.bold {
        let grow = size * BOLD_GROW;
        for g in letters.iter_mut() {
            let thick = crate::offset::offset_paths(g, grow, crate::offset::Corner::Round, false);
            if !thick.is_empty() {
                *g = thick;
            }
        }
    }
    if style.welded {
        // letter by letter into one outline (each letter's own holes kept)
        let mut acc: Vec<Polyline> = Vec::new();
        for g in letters {
            acc = if acc.is_empty() { g } else { crate::combine::combine(&acc, &g, crate::combine::CombineOp::Union).into_iter().flatten().collect() };
        }
        return acc;
    }
    letters.into_iter().flatten().collect()
}

/// A path's points, closed ones back to their start, and the distance along it at each point.
pub fn path_measure(path: &Polyline) -> (Vec<P2>, Vec<f64>) {
    let mut pts = path.pts.clone();
    if path.closed && !pts.is_empty() {
        pts.push(path.pts[0]);
    }
    let mut acc = vec![0.0];
    for w in pts.windows(2) {
        acc.push(acc.last().unwrap() + (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]));
    }
    (pts, acc)
}

/// The distance along a path of the point on it nearest `p`.
pub fn path_distance_at(path: &Polyline, p: P2) -> f64 {
    let (pts, acc) = path_measure(path);
    let mut best = (f64::INFINITY, 0.0);
    for (i, w) in pts.windows(2).enumerate() {
        let (a, b) = (w[0], w[1]);
        let (vx, vy) = (b[0] - a[0], b[1] - a[1]);
        let l2 = (vx * vx + vy * vy).max(1e-12);
        let t = (((p[0] - a[0]) * vx + (p[1] - a[1]) * vy) / l2).clamp(0.0, 1.0);
        let q = [a[0] + vx * t, a[1] + vy * t];
        let d = (q[0] - p[0]).hypot(q[1] - p[1]);
        if d < best.0 {
            best = (d, acc[i] + t * l2.sqrt());
        }
    }
    best.1
}

/// Letters set along a path: each letter's middle on the path, turned to it. Allinea X places
/// the text at the path's start, middle or end, "Offset" slides it further along; Allinea Y sets
/// the letters on the path by their top, middle or baseline. A closed path wraps around.
fn on_path(text: &str, size: f64, font_name: &str, style: &TextStyle, path: &Polyline) -> Vec<Polyline> {
    let (pts, acc) = path_measure(path);
    let total = *acc.last().unwrap_or(&0.0);
    if pts.len() < 2 || total <= 0.0 {
        return Vec::new();
    }
    let gl = glyphs(text, size, font_name);
    let n = gl.len();
    let width = gl.last().map(|g| g.0 + g.1).unwrap_or(0.0) + style.hspace * n.saturating_sub(1) as f64;
    let start = match style.align_x {
        0 => 0.0,
        2 => total - width,
        _ => (total - width) / 2.0,
    } + style.path_offset;
    let at = |d: f64| -> Option<(P2, f64)> {
        let d = if path.closed { d.rem_euclid(total) } else { d };
        if !(0.0..=total).contains(&d) {
            return None;
        }
        let i = acc.windows(2).position(|w| d <= w[1]).unwrap_or(acc.len().saturating_sub(2));
        let (a, b) = (pts[i], pts[i + 1]);
        let f = (d - acc[i]) / (acc[i + 1] - acc[i]).max(1e-12);
        Some(([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f], (b[1] - a[1]).atan2(b[0] - a[0])))
    };
    // vertical placement of the letters against the path
    let lift = match style.align_y {
        0 => -size * 0.72,
        2 => 0.0,
        _ => -size * 0.36,
    };
    let mut letters: Vec<Vec<Polyline>> = Vec::new();
    for (k, (x, adv, polys)) in gl.into_iter().enumerate() {
        let mid = start + x + style.hspace * k as f64 + adv / 2.0;
        let Some((c, ang)) = at(mid) else { continue };
        let (sn, cs) = ang.sin_cos();
        let mut g = Vec::new();
        for p in polys {
            let q: Vec<P2> = p
                .pts
                .iter()
                .map(|q| {
                    let ly = q[1] + lift;
                    let lx = q[0] - adv / 2.0 + if style.italic { ly * ITALIC_SLANT } else { 0.0 };
                    [c[0] + lx * cs - ly * sn, c[1] + lx * sn + ly * cs]
                })
                .collect();
            g.push(Polyline { pts: q, closed: p.closed });
        }
        letters.push(g);
    }
    if style.bold {
        for g in letters.iter_mut() {
            let thick = crate::offset::offset_paths(g, size * BOLD_GROW, crate::offset::Corner::Round, false);
            if !thick.is_empty() {
                *g = thick;
            }
        }
    }
    if style.welded {
        let mut acc2: Vec<Polyline> = Vec::new();
        for g in letters {
            acc2 = if acc2.is_empty() { g } else { crate::combine::combine(&acc2, &g, crate::combine::CombineOp::Union).into_iter().flatten().collect() };
        }
        return acc2;
    }
    letters.into_iter().flatten().collect()
}

/// Outlines with the default options (centred on the origin).
pub fn text_outline(text: &str, size: f64, font_name: &str) -> Vec<Polyline> {
    styled_outline(text, size, font_name, &TextStyle::default())
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
            let st = TextStyle::default();
            let (a, b) = caret_at(text, 25.0, &font, &st, i).unwrap();
            let mid = [a[0], (a[1] + b[1]) / 2.0];
            assert_eq!(caret_index_at(text, 25.0, &font, &st, mid), i, "caret {i}");
        }
    }
}

#[cfg(test)]
mod style_tests {
    use super::*;

    fn bx(p: &[Polyline]) -> ([f64; 2], [f64; 2]) {
        bounds(p.iter()).unwrap()
    }

    /// Allinea X / Y put the text's left / top edge on the origin; Saldato joins overlapping letters;
    /// spacing widens the text; italic leans it.
    #[test]
    fn options_shape_the_outline() {
        let font = fonts()[0].name.clone();
        let base = TextStyle::default();
        let plain = styled_outline("HH", 20.0, &font, &base);
        let (a, b) = bx(&plain);
        assert!((a[0] + b[0]).abs() < 1e-6 && (a[1] + b[1]).abs() < 1e-6, "centred");
        let left = styled_outline("HH", 20.0, &font, &TextStyle { align_x: 0, ..base.clone() });
        assert!(bx(&left).0[0].abs() < 1e-6);
        let wide = styled_outline("HH", 20.0, &font, &TextStyle { hspace: 5.0, ..base.clone() });
        assert!((bx(&wide).1[0] - bx(&wide).0[0]) - (b[0] - a[0]) > 4.9);
        // letters pushed onto each other: welded is one outline, not welded keeps two
        let tight = TextStyle { hspace: -8.0, ..base.clone() };
        let apart = styled_outline("HH", 20.0, &font, &tight).len();
        let joined = styled_outline("HH", 20.0, &font, &TextStyle { welded: true, ..tight }).len();
        assert!(joined < apart, "{joined} < {apart}");
        let it = styled_outline("H", 20.0, &font, &TextStyle { italic: true, ..base.clone() });
        let up = styled_outline("H", 20.0, &font, &base);
        assert!(bx(&it).1[0] - bx(&it).0[0] > bx(&up).1[0] - bx(&up).0[0]);
        assert_eq!(shown_text("ciao", &TextStyle { upper: true, ..base }), "CIAO");
    }
}

#[cfg(test)]
mod path_tests {
    use super::*;

    /// Letters follow the path (a straight line along X here) and the offset slides them along it;
    /// a point is found back at its distance along the path.
    #[test]
    fn text_follows_and_slides_along_path() {
        let font = fonts()[0].name.clone();
        let path = Polyline { pts: vec![[0.0, 0.0], [200.0, 0.0]], closed: false };
        let mut st = TextStyle { path: Some(path.clone()), align_x: 0, align_y: 2, ..TextStyle::default() };
        let a = styled_outline("AB", 10.0, &font, &st);
        let (amin, _) = bounds(a.iter()).unwrap();
        assert!(amin[0] >= -0.5 && amin[0] < 2.0, "starts at the path's start: {amin:?}");
        st.path_offset = 50.0;
        let b = styled_outline("AB", 10.0, &font, &st);
        let (bmin, _) = bounds(b.iter()).unwrap();
        assert!((bmin[0] - amin[0] - 50.0).abs() < 1e-6, "slid by the offset");
        assert!((path_distance_at(&path, [73.0, 4.0]) - 73.0).abs() < 1e-9);
    }
}
