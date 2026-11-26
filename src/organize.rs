//! The Organizza menu (reference pictures 29-31): grouping and auto-grouping, flips, mirror
//! across a line, quarter turns, two-point rotate / scale, align and distribute, nesting, SVGNest,
//! dock, move selection / laser to positions, grid / circular arrays, copy along a path, the
//! rubber-band outline, break apart, drawing order and locking.

use crate::app::{RayEtch, Tool};
use crate::canvas::Xf;
use crate::filemenu::FileRow;
use crate::i18n::{tr, trf};
use crate::model::{BBox, Geometry, P2, Polyline, Shape};
use eframe::egui::{self, Color32, Pos2, Stroke};

/// An arrange window that is open.
pub enum ArrangeDialog {
    /// "Griglia / Matrice" (GridArrayDialog): columns and rows, spacing between edges or
    /// centres, alternate shift and mirroring
    Grid { cols: usize, rows: usize, dx: f64, dy: f64, edges: bool, col_shift: f64, row_shift: f64, mirror_cols: bool, mirror_rows: bool },
    /// "Matrice circolare" (RadialArrayDialog): copies round a centre, from `start` to `end`
    Circular { count: usize, cx: f64, cy: f64, start: f64, end: f64, rotate: bool },
    /// "Copia lungo il percorso"
    AlongPath { count: usize, rotate: bool },
    /// "Incorporamento" (nesting): spacing between parts, margin from the sheet's edge, turns
    Nest { spacing: f64, margin: f64, turns: usize },
}

/// The arrays' last settings, kept like the original keeps them in its preferences.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(default)]
pub struct ArrayPrefs {
    pub grid: (usize, usize, f64, f64, bool, f64, f64, bool, bool),
    pub radial: (usize, f64, f64, bool),
    pub nest: (f64, f64, usize),
}

impl Default for ArrayPrefs {
    fn default() -> Self {
        ArrayPrefs {
            // GridArrayDialog: columns and rows start at their minimum 1 (sbCols/sbRows 1..200),
            // spacing 5 mm each way (Preferences +0xfc/+0x100), measured between edges
            // (rbColSpaceEdges checked, Preferences +0x106 = 0)
            grid: (1, 1, 5.0, 5.0, true, 0.0, 0.0, false, false),
            // RadialArrayDialog::Setup 0x1009ac370: 8 copies (+0x11c), 0° to 360° (+0x114/+0x118),
            // objects turned with their place (+0x120 = 1)
            radial: (8, 0.0, 360.0, true),
            // NestingDialog::LoadFromPrefs 0x1008c57cc: spacing and margin 0 (+0x74/+0x7c);
            // turns: every 90° (estimated)
            nest: (0.0, 0.0, 4),
        }
    }
}

const ARRAY_PREFS: &str = "arrange_prefs.json";

pub fn array_prefs() -> ArrayPrefs {
    crate::i18n::load_json(ARRAY_PREFS).unwrap_or_default()
}

fn save_array_prefs(f: impl FnOnce(&mut ArrayPrefs)) {
    let mut p = array_prefs();
    f(&mut p);
    crate::i18n::save_json(ARRAY_PREFS, &p);
}

impl ArrangeDialog {
    pub fn grid_default() -> Self {
        let (cols, rows, dx, dy, edges, col_shift, row_shift, mirror_cols, mirror_rows) = array_prefs().grid;
        ArrangeDialog::Grid { cols, rows, dx, dy, edges, col_shift, row_shift, mirror_cols, mirror_rows }
    }

    /// The centre starts at the selection's middle (RadialArrayDialog::Setup: Selection::GetBounds).
    pub fn circular_default(app: &RayEtch) -> Self {
        let (count, start, end, rotate) = array_prefs().radial;
        let c = app.doc.bbox_of(&app.selection).center();
        ArrangeDialog::Circular { count, cx: c[0], cy: c[1], start, end, rotate }
    }

    pub fn nest_default() -> Self {
        let (spacing, margin, turns) = array_prefs().nest;
        ArrangeDialog::Nest { spacing, margin, turns }
    }
}

/// "Rotazione / scala a due punti": the pivot, then the reference point; the shapes as they were.
#[derive(Clone, Debug, Default)]
pub struct TwoPoint {
    pub pivot: Option<P2>,
    pub reference: Option<P2>,
    pub orig: Vec<Shape>,
}

/// The Organizza menu, in the original's order and words.
pub fn rows(has_sel: bool, connected: bool) -> Vec<FileRow> {
    use FileRow::*;
    let mac = cfg!(target_os = "macos");
    let (cmd, shift_cmd, alt) = if mac { ("⌘", "⇧⌘", "⌥") } else { ("Ctrl+", "Ctrl+Maiusc+", "Alt+") };
    let k = |a: &str, b: &str| -> &'static str { Box::leak(format!("{a}{b}").into_boxed_str()) };
    let it = |id: &str, label: &str, keys: &'static str, on: bool| Item(id.into(), label.into(), keys, on);
    vec![
        it("group", tr("Raggruppa"), k(cmd, "G"), has_sel),
        it("ungroup", tr("Separa"), k(cmd, "U"), has_sel),
        it("auto_group", tr("Raggruppamento automatico"), "", true),
        Sep,
        it("mirror_h", tr("Capovolgi orizzontalmente"), k(shift_cmd, "H"), has_sel),
        it("mirror_v", tr("Capovolgi verticalmente"), k(shift_cmd, "V"), has_sel),
        it("mirror_line", tr("Specchia attraverso la linea"), k(shift_cmd, "M"), has_sel),
        it("rot_cw", tr("Ruota di 90 ° in senso orario"), ".", has_sel),
        it("rot_ccw", tr("Ruota di 90 ° in senso antiorario"), ",", has_sel),
        it("two_point", tr("Rotazione / scala a due punti"), k(cmd, "2"), has_sel),
        Sep,
        Sub(
            tr("Allinea").into(),
            vec![
                it("align_0", tr("Allinea a sinistra"), "", has_sel),
                it("align_4", tr("Allinea centri orizzontali"), "", has_sel),
                it("align_1", tr("Allinea a destra"), "", has_sel),
                Sep,
                it("align_2", tr("Allinea in alto"), "", has_sel),
                it("align_5", tr("Allinea centri verticali"), "", has_sel),
                it("align_3", tr("Allinea in basso"), "", has_sel),
                Sep,
                it("align_6", tr("Allinea centri"), "", has_sel),
                Sep,
                it("together_h", tr("Avvicina orizzontalmente"), "", has_sel),
                it("together_v", tr("Avvicina verticalmente"), "", has_sel),
                it("same_w", tr("Stessa larghezza"), "", has_sel),
                it("same_h", tr("Stessa altezza"), "", has_sel),
            ],
        ),
        Sub(
            tr("Distribuisci").into(),
            vec![
                it("dist_h_spaced", tr("Distribuisci spaziatura orizzontale"), "", has_sel),
                it("dist_v_spaced", tr("Distribuisci spaziatura verticale"), "", has_sel),
                it("dist_h_centered", tr("Distribuisci centri orizzontali"), "", has_sel),
                it("dist_v_centered", tr("Distribuisci centri verticali"), "", has_sel),
            ],
        ),
        Sep,
        it("nest", tr("Incorporamento"), k(shift_cmd, "N"), has_sel),
        it("svgnest", tr("SVGNest (strumento esterno)"), "", true),
        Sub(
            tr("Dock").into(),
            vec![
                it("dock_left", tr("Ancoraggio a sinistra"), "", has_sel),
                it("dock_right", tr("Ancoraggio a destra"), "", has_sel),
                it("dock_top", tr("Ancora in alto"), "", has_sel),
                it("dock_bottom", tr("Ancora in basso"), "", has_sel),
            ],
        ),
        Sub(
            tr("Sposta oggetti selezionati").into(),
            vec![
                it("page_center", tr("Sposta a centro pagina"), "", has_sel),
                it("page_tl", tr("Sposta in alto a sinistra"), "", has_sel),
                it("page_tr", tr("Sposta in alto a destra"), "", has_sel),
                it("page_bl", tr("Sposta in basso a sinistra"), "", has_sel),
                it("page_br", tr("Sposta in basso a destra"), "", has_sel),
                it("page_l", tr("Sposta a sinistra"), "", has_sel),
                it("page_r", tr("Sposta a destra"), "", has_sel),
                it("page_t", tr("Sposta sopra"), "", has_sel),
                it("page_b", tr("Sposta in basso"), "", has_sel),
            ],
        ),
        Sub(
            tr("Sposta laser nella selezione").into(),
            vec![
                it("laser_sel_l", tr("Sposta il laser a sinistra"), k(alt, &format!("{cmd}ò")), has_sel && connected),
                it("laser_sel_r", tr("Sposta il laser a destra"), k(alt, &format!("{cmd}à")), has_sel && connected),
                it("laser_sel_b", tr("Sposta il laser verso il basso"), k(shift_cmd, "ò"), has_sel && connected),
                it("laser_sel_t", tr("Sposta il laser verso l'alto"), k(shift_cmd, "à"), has_sel && connected),
            ],
        ),
        Sep,
        it("grid_array", tr("Griglia / Matrice"), "", has_sel),
        it("circular_array", tr("Matrice circolare"), "", has_sel),
        it("copy_along_path", tr("Copia lungo il percorso"), "", has_sel),
        Sep,
        it("rubber_band", tr("Crea un contorno elastico dalla selezione"), "", has_sel),
        it("break_apart", tr("Separa"), k(alt, "B"), has_sel),
        Sep,
        it("order_forward", tr("Porta avanti nell'ordine di disegno"), "", has_sel),
        it("order_backward", tr("Porta indietro nell'ordine di disegno"), "", has_sel),
        it("order_front", tr("Porta in primo piano"), k(cmd, "↑"), has_sel),
        it("order_back", tr("Porta in secondo piano"), k(cmd, "↓"), has_sel),
        it("lock", tr("Blocca le forme selezionate"), "", has_sel),
        it("unlock_shapes", tr("Sblocca le forme selezionate"), "", has_sel),
    ]
}

fn reflect(p: P2, a: P2, b: P2) -> P2 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l2 = d[0] * d[0] + d[1] * d[1];
    if l2 < 1e-18 {
        return p;
    }
    let t = ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2;
    let q = [a[0] + d[0] * t, a[1] + d[1] * t];
    [2.0 * q[0] - p[0], 2.0 * q[1] - p[1]]
}

fn to_world_path(s: &mut Shape, paths: Vec<Polyline>) {
    s.geom = Geometry::Path { subpaths: paths };
    s.tx = 0.0;
    s.ty = 0.0;
    s.sx = 1.0;
    s.sy = 1.0;
    s.rot = 0.0;
    s.tabs.clear();
    s.barcode = None;
}

/// Convex hull (Andrew's monotone chain), counter-clockwise.
pub fn convex_hull(mut pts: Vec<P2>) -> Vec<P2> {
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    pts.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: P2, a: P2, b: P2| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut lower: Vec<P2> = Vec::new();
    for p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], *p) <= 0.0 {
            lower.pop();
        }
        lower.push(*p);
    }
    let mut upper: Vec<P2> = Vec::new();
    for p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], *p) <= 0.0 {
            upper.pop();
        }
        upper.push(*p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// One part to nest: its outlines (world mm) and the raster of each allowed turn.
struct NestPart {
    paths: Vec<Polyline>,
    centre: P2,
}

/// A part drawn into cells `cell` mm wide, turned by `deg` about its centre and grown by `grow`
/// cells all round: (width, height, filled cells row by row from the top, the turned outline's
/// left and top in mm). Holes are filled in, as the original's nesting ignores them.
fn raster_part(part: &NestPart, deg: f64, cell: f64, grow: usize) -> (usize, usize, Vec<bool>, f64, f64) {
    let (sn, cs) = deg.to_radians().sin_cos();
    let c = part.centre;
    let turn = |p: &P2| [c[0] + (p[0] - c[0]) * cs - (p[1] - c[1]) * sn, c[1] + (p[0] - c[0]) * sn + (p[1] - c[1]) * cs];
    let paths: Vec<Vec<P2>> = part.paths.iter().map(|pl| pl.pts.iter().map(turn).collect()).collect();
    let (mut minx, mut maxx, mut miny, mut maxy) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in paths.iter().flatten() {
        minx = minx.min(p[0]);
        maxx = maxx.max(p[0]);
        miny = miny.min(p[1]);
        maxy = maxy.max(p[1]);
    }
    let w0 = (((maxx - minx) / cell).ceil() as usize).max(1);
    let h0 = (((maxy - miny) / cell).ceil() as usize).max(1);
    let mut base = vec![false; w0 * h0];
    // inside any closed outline (holes filled), and every outline's own cells
    for r in 0..h0 {
        let y = maxy - (r as f64 + 0.5) * cell;
        for (pi, pl) in part.paths.iter().enumerate() {
            if !pl.closed {
                continue;
            }
            let pts = &paths[pi];
            let n = pts.len();
            let mut xs: Vec<f64> = Vec::new();
            for i in 0..n {
                let (a, b) = (pts[i], pts[(i + 1) % n]);
                if (a[1] > y) != (b[1] > y) {
                    xs.push(a[0] + (y - a[1]) / (b[1] - a[1]) * (b[0] - a[0]));
                }
            }
            xs.sort_by(|a, b| a.total_cmp(b));
            for pair in xs.chunks(2) {
                if let [x0, x1] = pair {
                    let c0 = ((x0 - minx) / cell).floor().max(0.0) as usize;
                    let c1 = (((x1 - minx) / cell).ceil() as usize).min(w0);
                    for c in c0..c1 {
                        base[r * w0 + c] = true;
                    }
                }
            }
        }
    }
    for (pi, pts) in paths.iter().enumerate() {
        let n = pts.len();
        let segs = if part.paths[pi].closed { n } else { n.saturating_sub(1) };
        for i in 0..segs {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            let steps = (((b[0] - a[0]).abs().max((b[1] - a[1]).abs()) / cell * 2.0).ceil() as usize).max(1);
            for t in 0..=steps {
                let f = t as f64 / steps as f64;
                let (x, y) = (a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f);
                let c = (((x - minx) / cell) as usize).min(w0 - 1);
                let r = (((maxy - y) / cell) as usize).min(h0 - 1);
                base[r * w0 + c] = true;
            }
        }
    }
    if grow == 0 {
        return (w0, h0, base, minx, maxy);
    }
    // grown by `grow` cells: the spacing kept round the part
    let (w, h) = (w0 + 2 * grow, h0 + 2 * grow);
    let mut out = vec![false; w * h];
    for r in 0..h0 {
        for c in 0..w0 {
            if base[r * w0 + c] {
                for rr in r..=r + 2 * grow {
                    for cc in c..=c + 2 * grow {
                        out[rr * w + cc] = true;
                    }
                }
            }
        }
    }
    (w, h, out, minx - grow as f64 * cell, maxy + grow as f64 * cell)
}

/// The sheet's filled cells, with for each row the next filled cell at or after each column,
/// so a part can jump past what's in its way.
struct Sheet {
    w: usize,
    h: usize,
    next: Vec<Vec<u32>>,
}

impl Sheet {
    fn new(w: usize, h: usize) -> Self {
        Sheet { w, h, next: vec![(0..=w as u32).map(|_| w as u32).collect(); h] }
    }

    fn fill(&mut self, r: usize, c0: usize, c1: usize) {
        let row = &mut self.next[r];
        for c in c0..c1.min(self.w) {
            row[c] = c as u32;
        }
        // the cells before the run now meet it
        let mut c = c0;
        while c > 0 {
            c -= 1;
            if row[c] == c as u32 {
                break;
            }
            row[c] = row[c].min(c0 as u32);
        }
    }
}

/// Runs of filled cells per row of a raster: (row, first, past the end).
fn runs(w: usize, h: usize, cells: &[bool]) -> Vec<(usize, usize, usize)> {
    let mut v = Vec::new();
    for r in 0..h {
        let mut c = 0;
        while c < w {
            if cells[r * w + c] {
                let s = c;
                while c < w && cells[r * w + c] {
                    c += 1;
                }
                v.push((r, s, c));
            } else {
                c += 1;
            }
        }
    }
    v
}

/// The top-most, then left-most place where `part_runs` fits (bottom-left fill seen from the
/// top-left corner of the sheet): (column, row).
fn first_fit(sheet: &Sheet, w: usize, h: usize, part_runs: &[(usize, usize, usize)], margin: usize) -> Option<(usize, usize)> {
    if w + 2 * margin > sheet.w || h + 2 * margin > sheet.h {
        return None;
    }
    for y in margin..=sheet.h - h - margin {
        let mut x = margin;
        'col: while x + w + margin <= sheet.w {
            for &(r, a, b) in part_runs {
                let o = sheet.next[y + r][x + a] as usize;
                if o < x + b {
                    x = o + 1 - a.min(o + 1);
                    x = x.max(o + 1 - a);
                    continue 'col;
                }
            }
            return Some((x, y));
        }
    }
    None
}

impl RayEtch {
    /// Shapes that may be moved: the selection without the locked ones.
    pub fn movable_shapes(&self) -> Vec<Shape> {
        self.doc.shapes.iter().filter(|s| self.selection.contains(&s.id) && !s.locked).cloned().collect()
    }

    fn sel_bbox(&self) -> BBox {
        self.doc.bbox_of(&self.selection)
    }

    /// Move the selection (not its locked shapes) by `d`.
    fn shift_selection(&mut self, d: P2) {
        let sel = self.selection.clone();
        for s in self.doc.shapes.iter_mut().filter(|s| sel.contains(&s.id) && !s.locked) {
            s.tx += d[0];
            s.ty += d[1];
        }
        self.dirty = true;
    }

    /// "Raggruppamento automatico": shapes whose boxes touch or overlap (the selection, or all
    /// when nothing is selected) are put in groups (estimated rule).
    pub fn auto_group(&mut self) {
        let pool: Vec<(u64, BBox)> = self
            .doc
            .shapes
            .iter()
            .filter(|s| (self.selection.is_empty() || self.selection.contains(&s.id)) && s.groups.is_empty())
            .map(|s| (s.id, s.bbox()))
            .collect();
        let n = pool.len();
        let mut comp: Vec<usize> = (0..n).collect();
        fn root(c: &mut [usize], i: usize) -> usize {
            let mut r = i;
            while c[r] != r {
                r = c[r];
            }
            c[i] = r;
            r
        }
        let touch = |a: &BBox, b: &BBox| a.min[0] <= b.max[0] && b.min[0] <= a.max[0] && a.min[1] <= b.max[1] && b.min[1] <= a.max[1];
        for i in 0..n {
            for j in i + 1..n {
                if touch(&pool[i].1, &pool[j].1) {
                    let (a, b) = (root(&mut comp, i), root(&mut comp, j));
                    comp[a] = b;
                }
            }
        }
        let mut groups: std::collections::HashMap<usize, Vec<u64>> = Default::default();
        for i in 0..n {
            let r = root(&mut comp, i);
            groups.entry(r).or_default().push(pool[i].0);
        }
        let made: Vec<Vec<u64>> = groups.into_values().filter(|g| g.len() > 1).collect();
        if made.is_empty() {
            self.status = tr("Nessuna forma da raggruppare automaticamente.").into();
            return;
        }
        self.checkpoint_now();
        for g in &made {
            let gid = self.doc.alloc_id();
            for s in self.doc.shapes.iter_mut().filter(|s| g.contains(&s.id)) {
                s.groups.insert(0, gid);
            }
        }
        self.dirty = true;
        self.status = trf("Gruppi creati: {}", &[&made.len()]);
    }

    /// "Specchia attraverso la linea" (⇧⌘M): the last selected object, a straight line, is the
    /// mirror; the other selected shapes are flipped across it.
    pub fn mirror_across_line(&mut self) {
        let items = self.selection_items();
        let Some(last) = items.last() else { return };
        let line = self.doc.shapes.iter().filter(|s| last.contains(&s.id)).flat_map(|s| s.world_paths()).next();
        let Some(line) = line.filter(|l| !l.closed && l.pts.len() == 2) else {
            self.status = tr("Seleziona le forme e, per ultima, una linea retta da usare come specchio.").into();
            return;
        };
        let (a, b) = (line.pts[0], line.pts[1]);
        let others: Vec<u64> = items[..items.len() - 1].iter().flatten().copied().collect();
        if others.is_empty() {
            return;
        }
        self.checkpoint_now();
        for s in self.doc.shapes.iter_mut().filter(|s| others.contains(&s.id) && !s.locked) {
            if matches!(s.geom, Geometry::Image { .. }) {
                continue;
            }
            let paths = s.world_paths().into_iter().map(|p| Polyline { pts: p.pts.iter().map(|q| reflect(*q, a, b)).collect(), closed: p.closed }).collect();
            to_world_path(s, paths);
        }
        self.dirty = true;
        self.status = tr("Specchiato attraverso la linea").into();
    }

    /// "Rotazione / scala a due punti" (⌘2): click the pivot, then a point of the selection,
    /// then where that point should go.
    pub fn start_two_point(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.two_point = TwoPoint::default();
        self.tool = Tool::TwoPoint;
    }

    /// "Dock": slide the selection one way until it touches the nearest shape in its path, or
    /// the edge of the work area.
    pub fn dock(&mut self, dir: usize) {
        let b = self.sel_bbox();
        if !b.is_valid() {
            return;
        }
        let (w, h) = (self.doc.device.width, self.doc.device.height);
        // 0 left, 1 right, 2 up, 3 down: the free distance to the first obstacle
        let mut room = match dir {
            0 => b.min[0],
            1 => w - b.max[0],
            2 => h - b.max[1],
            _ => b.min[1],
        };
        for s in self.doc.shapes.iter().filter(|s| !self.selection.contains(&s.id)) {
            let o = s.bbox();
            let overlap_y = o.min[1] < b.max[1] && b.min[1] < o.max[1];
            let overlap_x = o.min[0] < b.max[0] && b.min[0] < o.max[0];
            let d = match dir {
                0 if overlap_y && o.max[0] <= b.min[0] + 1e-9 => b.min[0] - o.max[0],
                1 if overlap_y && o.min[0] >= b.max[0] - 1e-9 => o.min[0] - b.max[0],
                2 if overlap_x && o.min[1] >= b.max[1] - 1e-9 => o.min[1] - b.max[1],
                3 if overlap_x && o.max[1] <= b.min[1] + 1e-9 => b.min[1] - o.max[1],
                _ => continue,
            };
            room = room.min(d);
        }
        let room = room.max(0.0);
        let d = match dir {
            0 => [-room, 0.0],
            1 => [room, 0.0],
            2 => [0.0, room],
            _ => [0.0, -room],
        };
        self.checkpoint_now();
        self.shift_selection(d);
    }

    /// "Sposta oggetti selezionati": to the middle, a corner or an edge of the work area.
    pub fn move_to_page(&mut self, spot: &str) {
        let b = self.sel_bbox();
        if !b.is_valid() {
            return;
        }
        let (w, h) = (self.doc.device.width, self.doc.device.height);
        let c = b.center();
        let (left, right, top, bottom, cx, cy) = (-b.min[0], w - b.max[0], h - b.max[1], -b.min[1], w / 2.0 - c[0], h / 2.0 - c[1]);
        let d = match spot {
            "page_center" => [cx, cy],
            "page_tl" => [left, top],
            "page_tr" => [right, top],
            "page_bl" => [left, bottom],
            "page_br" => [right, bottom],
            "page_l" => [left, 0.0],
            "page_r" => [right, 0.0],
            "page_t" => [0.0, top],
            _ => [0.0, bottom],
        };
        self.checkpoint_now();
        self.shift_selection(d);
    }

    /// "Sposta laser nella selezione": the head to the middle of one side of the selection.
    pub fn laser_to_selection(&mut self, side: &str) {
        let b = self.sel_bbox();
        if !b.is_valid() {
            return;
        }
        let c = b.center();
        let p = match side {
            "laser_sel_l" => [b.min[0], c[1]],
            "laser_sel_r" => [b.max[0], c[1]],
            "laser_sel_b" => [c[0], b.min[1]],
            _ => [c[0], b.max[1]],
        };
        self.send_lines(vec![crate::gcode::jog_line(true, p[0], p[1], self.jog_speed)]);
        self.status = trf("Sposto il laser a {}, {}", &[&format!("{:.1}", p[0]), &format!("{:.1}", p[1])]);
    }

    /// Copies of the selection, each moved by `d` and turned by `deg` about `pivot`.
    fn add_copy(&mut self, src: &[Shape], d: P2, deg: f64, pivot: P2) -> Vec<u64> {
        let mut remap: std::collections::HashMap<u64, u64> = Default::default();
        let mut ids = Vec::new();
        for s in src {
            let mut c = s.clone();
            c.id = 0;
            c.locked = false;
            for g in c.groups.iter_mut() {
                let ng = *remap.entry(*g).or_insert_with(|| self.doc.alloc_id());
                *g = ng;
            }
            if deg != 0.0 {
                c.rotate_about(deg, pivot);
            }
            c.tx += d[0];
            c.ty += d[1];
            ids.push(self.doc.add_shape(c));
        }
        ids
    }

    /// "Griglia / Matrice": cols × rows copies `dx`/`dy` apart (between edges, or between centres
    /// when `edges` is off); odd rows shift sideways by `row_shift` and odd columns up/down by
    /// `col_shift`; alternate columns / rows can be mirrored (GridArrayDialog options).
    #[allow(clippy::too_many_arguments)]
    pub fn grid_array(&mut self, cols: usize, rows: usize, dx: f64, dy: f64, edges: bool, col_shift: f64, row_shift: f64, mirror_cols: bool, mirror_rows: bool) {
        let src = self.selected_shapes();
        let b = self.sel_bbox();
        if src.is_empty() || !b.is_valid() {
            return;
        }
        save_array_prefs(|p| p.grid = (cols, rows, dx, dy, edges, col_shift, row_shift, mirror_cols, mirror_rows));
        let (sx, sy) = if edges { (b.w() + dx, b.h() + dy) } else { (dx, dy) };
        self.checkpoint_now();
        let mut all = self.selection.clone();
        let c0 = b.center();
        for r in 0..rows.max(1) {
            for c in 0..cols.max(1) {
                if r == 0 && c == 0 {
                    continue;
                }
                let mut d = [c as f64 * sx, -(r as f64) * sy];
                if r % 2 == 1 {
                    d[0] += row_shift;
                }
                if c % 2 == 1 {
                    d[1] -= col_shift;
                }
                let ids = self.add_copy(&src, d, 0.0, [0.0, 0.0]);
                let centre = [c0[0] + d[0], c0[1] + d[1]];
                for s in self.doc.shapes.iter_mut().filter(|s| ids.contains(&s.id)) {
                    if mirror_cols && c % 2 == 1 {
                        s.mirror(true, centre);
                    }
                    if mirror_rows && r % 2 == 1 {
                        s.mirror(false, centre);
                    }
                }
                all.extend(ids);
            }
        }
        self.selection = all;
        self.dirty = true;
    }

    /// "Matrice circolare" (RadialArray::MakeArray 0x1002825a8): `count` objects in all, the
    /// step (end − start) / count; copy i turns by start + i·step about (cx, cy), and the object
    /// turns with it when `rotate` (otherwise it only moves round).
    pub fn circular_array(&mut self, count: usize, cx: f64, cy: f64, start: f64, end: f64, rotate: bool) {
        let src = self.selected_shapes();
        let b = self.sel_bbox();
        if src.is_empty() || !b.is_valid() || count < 2 {
            return;
        }
        save_array_prefs(|p| p.radial = (count, start, end, rotate));
        let step = (end - start) / count as f64;
        let c0 = b.center();
        let centre = [cx, cy];
        self.checkpoint_now();
        let mut all = self.selection.clone();
        for i in 1..count {
            let deg = start + step * i as f64;
            let (sn, cs) = deg.to_radians().sin_cos();
            let rel = [c0[0] - centre[0], c0[1] - centre[1]];
            let p = [centre[0] + rel[0] * cs - rel[1] * sn, centre[1] + rel[0] * sn + rel[1] * cs];
            let d = [p[0] - c0[0], p[1] - c0[1]];
            all.extend(self.add_copy(&src, d, if rotate { deg } else { 0.0 }, c0));
        }
        self.selection = all;
        self.dirty = true;
    }

    /// "Copia lungo il percorso": the last selected object is the path; `count` copies of the
    /// others are set along it, evenly (turned with it when `rotate`).
    pub fn copy_along_path(&mut self, count: usize, rotate: bool) {
        let items = self.selection_items();
        if items.len() < 2 || count == 0 {
            self.status = tr("Seleziona le forme da copiare e, per ultimo, il percorso.").into();
            return;
        }
        let last = items.last().unwrap().clone();
        let Some(path) = self.doc.shapes.iter().filter(|s| last.contains(&s.id)).flat_map(|s| s.world_paths()).next() else { return };
        let src: Vec<Shape> = self.doc.shapes.iter().filter(|s| items[..items.len() - 1].iter().flatten().any(|i| *i == s.id)).cloned().collect();
        let mut sb = BBox::empty();
        for s in &src {
            sb.union(&s.bbox());
        }
        let mut pts = path.pts.clone();
        if path.closed {
            pts.push(path.pts[0]);
        }
        let mut acc = vec![0.0];
        for w in pts.windows(2) {
            acc.push(acc.last().unwrap() + (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]));
        }
        let total = *acc.last().unwrap();
        if total <= 0.0 {
            return;
        }
        let at = |d: f64| -> (P2, f64) {
            let i = acc.windows(2).position(|w| d <= w[1]).unwrap_or(acc.len() - 2);
            let (a, b) = (pts[i], pts[i + 1]);
            let f = (d - acc[i]) / (acc[i + 1] - acc[i]).max(1e-12);
            ([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f], (b[1] - a[1]).atan2(b[0] - a[0]).to_degrees())
        };
        let n = count.max(1);
        let div = if path.closed { n as f64 } else { (n.max(2) - 1) as f64 };
        self.checkpoint_now();
        let c0 = sb.center();
        let mut ids = Vec::new();
        for i in 0..n {
            let (p, ang) = at(total * i as f64 / div);
            ids.extend(self.add_copy(&src, [p[0] - c0[0], p[1] - c0[1]], if rotate { ang } else { 0.0 }, c0));
        }
        self.selection = ids;
        self.dirty = true;
        self.status = trf("Copie lungo il percorso: {}", &[&n]);
    }

    /// "Crea un contorno elastico dalla selezione": a closed outline stretched around it.
    pub fn rubber_band(&mut self) {
        let pts: Vec<P2> = self.selected_shapes().iter().flat_map(|s| s.world_paths()).flat_map(|p| p.pts).collect();
        let hull = convex_hull(pts);
        if hull.len() < 3 {
            return;
        }
        self.checkpoint_now();
        let id = self.doc.add_shape(Shape::new(0, Geometry::Path { subpaths: vec![Polyline { pts: hull, closed: true }] }, self.current_layer, 0.0, 0.0));
        self.selection = vec![id];
        self.dirty = true;
    }

    /// "Separa" (⌥B, break apart): each outline of the selected shapes becomes a shape of its own.
    pub fn break_apart(&mut self) {
        self.checkpoint_now();
        let sel = self.selection.clone();
        let mut ids = Vec::new();
        let mut out = Vec::new();
        for s in self.doc.shapes.iter().filter(|s| sel.contains(&s.id) && !matches!(s.geom, Geometry::Image { .. })) {
            for p in s.geom.local_paths() {
                let mut c = s.clone();
                c.id = 0;
                c.geom = Geometry::Path { subpaths: vec![p] };
                c.barcode = None;
                c.tabs.clear();
                out.push(c);
            }
        }
        if out.is_empty() {
            self.undo.pop();
            return;
        }
        self.doc.shapes.retain(|s| !(sel.contains(&s.id) && !matches!(s.geom, Geometry::Image { .. })));
        for c in out {
            ids.push(self.doc.add_shape(c));
        }
        self.selection = ids;
        self.dirty = true;
    }

    /// Drawing order: 0 one forward, 1 one back, 2 to the front, 3 to the back.
    pub fn reorder(&mut self, how: u8) {
        let sel = self.selection.clone();
        if sel.is_empty() {
            return;
        }
        self.checkpoint_now();
        let v = &mut self.doc.shapes;
        match how {
            0 => {
                for i in (0..v.len().saturating_sub(1)).rev() {
                    if sel.contains(&v[i].id) && !sel.contains(&v[i + 1].id) {
                        v.swap(i, i + 1);
                    }
                }
            }
            1 => {
                for i in 1..v.len() {
                    if sel.contains(&v[i].id) && !sel.contains(&v[i - 1].id) {
                        v.swap(i, i - 1);
                    }
                }
            }
            2 => {
                let (mut a, b): (Vec<Shape>, Vec<Shape>) = v.drain(..).partition(|s| !sel.contains(&s.id));
                a.extend(b);
                *v = a;
            }
            _ => {
                let (a, mut b): (Vec<Shape>, Vec<Shape>) = v.drain(..).partition(|s| sel.contains(&s.id));
                let mut a = a;
                a.append(&mut b);
                *v = a;
            }
        }
        self.dirty = true;
    }

    /// "Blocca / Sblocca le forme selezionate": locked shapes stay where they are.
    pub fn set_locked(&mut self, on: bool) {
        self.checkpoint_now();
        let sel = self.selection.clone();
        for s in self.doc.shapes.iter_mut().filter(|s| sel.contains(&s.id)) {
            s.locked = on;
        }
        self.dirty = true;
        self.status = if on { tr("Forme bloccate") } else { tr("Forme sbloccate") }.into();
    }

    /// "Incorporamento" (⇧⌘N): the selected objects packed into the work area by their real
    /// outlines (as the original's true-shape nesting, which leaves holes empty): biggest first,
    /// each at the top-most, left-most place it fits, trying `turns` orientations; `spacing`
    /// between parts and `margin` from the edge.
    pub fn nest(&mut self, spacing: f64, margin: f64, turns: usize) {
        let items: Vec<Vec<u64>> = self.selection_items().into_iter().filter(|ids| self.doc.shapes.iter().any(|s| ids.contains(&s.id) && !s.locked)).collect();
        if items.is_empty() {
            return;
        }
        save_array_prefs(|p| p.nest = (spacing, margin, turns));
        let (w_mm, h_mm) = (self.doc.device.width, self.doc.device.height);
        // cells small enough for detail, few enough to stay quick (estimated: about 600 000)
        let cell = ((w_mm * h_mm / 600_000.0).sqrt()).max(0.25);
        let (sw, sh) = ((w_mm / cell).floor() as usize, (h_mm / cell).floor() as usize);
        let mut sheet = Sheet::new(sw, sh);
        let parts: Vec<NestPart> = items
            .iter()
            .map(|ids| {
                let paths: Vec<Polyline> = self.doc.shapes.iter().filter(|s| ids.contains(&s.id)).flat_map(|s| s.world_paths()).collect();
                let b = self.doc.bbox_of(ids);
                NestPart { paths, centre: b.center() }
            })
            .collect();
        let mut order: Vec<usize> = (0..parts.len()).collect();
        let area = |i: usize| self.doc.bbox_of(&items[i]).w() * self.doc.bbox_of(&items[i]).h();
        order.sort_by(|a, b| area(*b).total_cmp(&area(*a)));
        let grow = (spacing / cell).ceil() as usize;
        let margin_cells = (margin / cell).ceil() as usize;
        let angles: Vec<f64> = (0..turns.max(1)).map(|k| 360.0 * k as f64 / turns.max(1) as f64).collect();
        self.checkpoint_now();
        let mut placed = 0;
        for i in order {
            let mut best: Option<(usize, usize, f64, f64, f64, usize, usize, Vec<bool>)> = None;
            for &deg in &angles {
                // the spacing is kept by placing the grown raster, then marking the plain one
                let (gw, gh, grown, left, top) = raster_part(&parts[i], deg, cell, grow);
                let gruns = runs(gw, gh, &grown);
                if let Some((x, y)) = first_fit(&sheet, gw, gh, &gruns, margin_cells.saturating_sub(grow)) {
                    if best.as_ref().is_none_or(|b| (y, x) < (b.1, b.0)) {
                        let (pw, ph, plain, _, _) = raster_part(&parts[i], deg, cell, 0);
                        let _ = (pw, ph);
                        best = Some((x, y, deg, left, top, gw, gh, plain));
                    }
                }
            }
            let Some((x, y, deg, left, top, _gw, _gh, plain)) = best else { continue };
            let (pw, ph, _, _, _) = raster_part(&parts[i], deg, cell, 0);
            for (r, a, b) in runs(pw, ph, &plain) {
                sheet.fill(y + grow + r, x + grow + a, x + grow + b);
            }
            // where the turned part's top-left goes, in mm
            let target = [(x as f64) * cell, h_mm - (y as f64) * cell];
            let d = [target[0] - left, target[1] - top];
            let c = parts[i].centre;
            for s in self.doc.shapes.iter_mut().filter(|s| items[i].contains(&s.id) && !s.locked) {
                if deg != 0.0 {
                    s.rotate_about(deg, c);
                }
                s.tx += d[0];
                s.ty += d[1];
            }
            placed += 1;
        }
        self.dirty = true;
        self.status = if placed < items.len() {
            trf("Incorporamento: {} di {} oggetti nell'area di lavoro (gli altri non entrano).", &[&placed, &items.len()])
        } else {
            trf("Incorporamento fatto: {} oggetti", &[&placed])
        };
    }
}

/// "Rotazione / scala a due punti" on the canvas.
pub fn canvas_two_point(app: &mut RayEtch, resp: &egui::Response, painter: &egui::Painter, xf: &Xf, hover: Option<Pos2>) {
    let raw = hover.map(|h| xf.to_world(h));
    let col = Color32::from_rgb(60, 160, 255);
    let tp = app.two_point.clone();
    if let (Some(pv), Some(rf), Some(cur)) = (tp.pivot, tp.reference, raw) {
        // live: turn and scale the copies kept at the start so `rf` lands on the pointer
        let (a0, a1) = ((rf[1] - pv[1]).atan2(rf[0] - pv[0]), (cur[1] - pv[1]).atan2(cur[0] - pv[0]));
        let (d0, d1) = ((rf[0] - pv[0]).hypot(rf[1] - pv[1]), (cur[0] - pv[0]).hypot(cur[1] - pv[1]));
        let k = if d0 > 1e-9 { d1 / d0 } else { 1.0 };
        let deg = (a1 - a0).to_degrees();
        for o in &tp.orig {
            if let Some(s) = app.doc.shapes.iter_mut().find(|s| s.id == o.id) {
                *s = o.clone();
                s.rotate_about(deg, pv);
                s.sx *= k;
                s.sy *= k;
                s.tx = pv[0] + (s.tx - pv[0]) * k;
                s.ty = pv[1] + (s.ty - pv[1]) * k;
            }
        }
        painter.line_segment([xf.to_screen(pv), xf.to_screen(cur)], Stroke::new(1.0, col));
    }
    for p in [tp.pivot, tp.reference].into_iter().flatten() {
        painter.circle_stroke(xf.to_screen(p), 5.0, Stroke::new(1.5, col));
    }
    if resp.clicked() {
        if let Some(p) = raw {
            if tp.pivot.is_none() {
                app.two_point.pivot = Some(p);
            } else if tp.reference.is_none() {
                app.checkpoint_now();
                app.two_point.reference = Some(p);
                app.two_point.orig = app.movable_shapes();
            } else {
                app.dirty = true;
                app.two_point = TwoPoint::default();
                app.tool = Tool::Select;
            }
        }
    }
    app.status = match (tp.pivot, tp.reference) {
        (None, _) => tr("Due punti: fai clic sul punto fisso (perno)"),
        (Some(_), None) => tr("Due punti: fai clic su un punto della selezione"),
        _ => tr("Due punti: fai clic dove portarlo (ruota e scala) · Esc annulla"),
    }
    .into();
}

/// The open arrange window.
pub fn windows(app: &mut RayEtch, ctx: &egui::Context) {
    use egui::vec2;
    let Some(mut d) = app.arrange_dialog.take() else { return };
    let mut open = true;
    let mut act: Option<bool> = None;
    let title = match d {
        ArrangeDialog::Grid { .. } => tr("Griglia / Matrice"),
        ArrangeDialog::Circular { .. } => tr("Matrice circolare"),
        ArrangeDialog::AlongPath { .. } => tr("Copia lungo il percorso"),
        ArrangeDialog::Nest { .. } => tr("Incorporamento"),
    };
    crate::theme::centered(ctx, "arrange", egui::Window::new(title).open(&mut open).resizable(false).collapsible(false)).show(ctx, |ui| {
        ui.set_min_width(360.0);
        egui::Grid::new("arr").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| match &mut d {
            ArrangeDialog::Grid { cols, rows, dx, dy, edges, col_shift, row_shift, mirror_cols, mirror_rows } => {
                ui.label(tr("Colonne"));
                ui.add(egui::DragValue::new(cols).range(1..=200));
                ui.end_row();
                ui.label(tr("Righe"));
                ui.add(egui::DragValue::new(rows).range(1..=200));
                ui.end_row();
                ui.label(tr("Spaziatura colonne"));
                ui.add(crate::i18n::dv(dx).speed(0.1).range(-5000.0..=5000.0).max_decimals(3).suffix(" mm"));
                ui.end_row();
                ui.label(tr("Spaziatura righe"));
                ui.add(crate::i18n::dv(dy).speed(0.1).range(-5000.0..=5000.0).max_decimals(3).suffix(" mm"));
                ui.end_row();
                ui.label(tr("Misura da"));
                ui.horizontal(|ui| {
                    ui.radio_value(edges, true, tr("Bordi"));
                    ui.radio_value(edges, false, tr("Centri"));
                });
                ui.end_row();
                ui.label(tr("Sposta colonne alterne"));
                ui.add(crate::i18n::dv(col_shift).speed(0.1).max_decimals(3).suffix(" mm"));
                ui.end_row();
                ui.label(tr("Sposta righe alterne"));
                ui.add(crate::i18n::dv(row_shift).speed(0.1).max_decimals(3).suffix(" mm"));
                ui.end_row();
                ui.label("");
                ui.checkbox(mirror_cols, tr("Specchia colonne alterne"));
                ui.end_row();
                ui.label("");
                ui.checkbox(mirror_rows, tr("Specchia righe alterne"));
                ui.end_row();
            }
            ArrangeDialog::Circular { count, cx, cy, start, end, rotate } => {
                ui.label(tr("Numero di copie"));
                ui.add(egui::DragValue::new(count).range(2..=1000));
                ui.end_row();
                ui.label(tr("Centro X"));
                ui.add(crate::i18n::dv(cx).speed(0.5).max_decimals(2).suffix(" mm"));
                ui.end_row();
                ui.label(tr("Centro Y"));
                ui.add(crate::i18n::dv(cy).speed(0.5).max_decimals(2).suffix(" mm"));
                ui.end_row();
                ui.label(tr("Angolo iniziale"));
                ui.add(crate::i18n::dv(start).speed(1.0).max_decimals(2).suffix("°"));
                ui.end_row();
                ui.label(tr("Angolo finale"));
                ui.add(crate::i18n::dv(end).speed(1.0).max_decimals(2).suffix("°"));
                ui.end_row();
                ui.label(tr("Passo"));
                ui.label(format!("{:.2}°", (*end - *start) / (*count).max(1) as f64));
                ui.end_row();
                ui.label("");
                ui.checkbox(rotate, tr("Ruota gli oggetti"));
                ui.end_row();
            }
            ArrangeDialog::AlongPath { count, rotate } => {
                ui.label(tr("Numero di copie"));
                ui.add(egui::DragValue::new(count).range(1..=1000));
                ui.end_row();
                ui.label("");
                ui.checkbox(rotate, tr("Ruota le copie seguendo il percorso"));
                ui.end_row();
            }
            ArrangeDialog::Nest { spacing, margin, turns } => {
                ui.label(tr("Spaziatura tra le parti"));
                ui.add(crate::i18n::dv(spacing).speed(0.1).range(0.0..=100.0).max_decimals(2).suffix(" mm"));
                ui.end_row();
                ui.label(tr("Margine dal bordo"));
                ui.add(crate::i18n::dv(margin).speed(0.1).range(0.0..=100.0).max_decimals(2).suffix(" mm"));
                ui.end_row();
                ui.label(tr("Orientamenti"));
                egui::ComboBox::from_id_salt("nest_turns").selected_text(match *turns {
                    1 => tr("Nessuna rotazione"),
                    2 => tr("0° e 180°"),
                    4 => tr("Ogni 90°"),
                    _ => tr("Ogni 45°"),
                }).show_ui(ui, |ui| {
                    ui.selectable_value(turns, 1, tr("Nessuna rotazione"));
                    ui.selectable_value(turns, 2, tr("0° e 180°"));
                    ui.selectable_value(turns, 4, tr("Ogni 90°"));
                    ui.selectable_value(turns, 8, tr("Ogni 45°"));
                });
                ui.end_row();
            }
        });
        if matches!(d, ArrangeDialog::Nest { .. }) {
            ui.label(egui::RichText::new(tr("Le parti si incastrano secondo la loro forma reale, dalla più grande, in alto a sinistra dell'area di lavoro. I fori interni restano vuoti.")).small().color(crate::theme::p().text_dim));
        }
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
        Some(true) => match d {
            ArrangeDialog::Grid { cols, rows, dx, dy, edges, col_shift, row_shift, mirror_cols, mirror_rows } => {
                app.grid_array(cols, rows, dx, dy, edges, col_shift, row_shift, mirror_cols, mirror_rows)
            }
            ArrangeDialog::Circular { count, cx, cy, start, end, rotate } => app.circular_array(count, cx, cy, start, end, rotate),
            ArrangeDialog::AlongPath { count, rotate } => app.copy_along_path(count, rotate),
            ArrangeDialog::Nest { spacing, margin, turns } => app.nest(spacing, margin, turns),
        },
        Some(false) => {}
        None if open => app.arrange_dialog = Some(d),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hull_reflect_and_nesting() {
        let h = convex_hull(vec![[0.0, 0.0], [2.0, 0.0], [1.0, 1.0], [2.0, 2.0], [0.0, 2.0]]);
        assert_eq!(h.len(), 4);
        let r = reflect([1.0, 2.0], [0.0, 0.0], [10.0, 0.0]);
        assert!((r[0] - 1.0).abs() < 1e-9 && (r[1] + 2.0).abs() < 1e-9);
        // two 4×2 cell blocks in a 6-wide sheet: the second goes under the first
        let mut sheet = Sheet::new(6, 6);
        let block = vec![true; 8];
        let rr = runs(4, 2, &block);
        let (x, y) = first_fit(&sheet, 4, 2, &rr, 0).unwrap();
        assert_eq!((x, y), (0, 0));
        for (r, a, b) in &rr {
            sheet.fill(y + r, x + a, x + b);
        }
        assert_eq!(first_fit(&sheet, 4, 2, &rr, 0), Some((0, 2)));
        // an L leaves room: a 2×2 block fits beside its short arm
        let mut s2 = Sheet::new(4, 4);
        let l = [true, true, false, false, true, true, false, false, true, true, true, true, true, true, true, true];
        for (r, a, b) in runs(4, 4, &l) {
            s2.fill(r, a, b);
        }
        assert_eq!(first_fit(&s2, 2, 2, &runs(2, 2, &[true; 4]), 0), Some((2, 0)));
    }
}
