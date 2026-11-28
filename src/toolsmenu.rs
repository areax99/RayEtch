//! The Strumenti menu (reference picture 28): drawing tools with the Disegna forma submenu,
//! node editing, weld and the boolean operations, cut shapes, the image tools, text on a path,
//! crop image, resize slots, 4- and 16-point warp and the tangent circle.

use crate::app::{RayEtch, Tool};
use crate::canvas::Xf;
use crate::combine::{CombineOp, combine};
use crate::filemenu::FileRow;
use crate::i18n::{tr, trf};
use crate::model::{BBox, Geometry, P2, Polyline, Shape, dist_seg, point_in_rings};
use eframe::egui::{self, Color32, Pos2, Stroke};

/// Node handles: grab distance in screen pixels (estimated).
const NODE_GRAB_PX: f32 = 7.0;
/// Warp: outlines are cut into pieces no longer than this before bending, mm (estimated).
const WARP_STEP_MM: f64 = 0.5;

/// Node editing: the shape and the node picked / being dragged.
#[derive(Clone, Debug, Default)]
pub struct NodeEdit {
    pub shape: Option<u64>,
    pub picked: Option<(usize, usize)>,
    pub dragging: bool,
}

/// A warp in progress: the shapes as they were (world outlines), their box, and the control
/// points (n × n, row by row from the bottom left).
#[derive(Clone, Debug)]
pub struct Warp {
    pub n: usize,
    pub bbox: BBox,
    pub orig: Vec<(u64, Vec<Polyline>)>,
    pub ctrl: Vec<P2>,
    pub drag: Option<usize>,
}

/// The tangent circle tool: the lines picked so far (two points each).
#[derive(Clone, Debug, Default)]
pub struct Tangent {
    pub lines: Vec<(P2, P2)>,
}

/// "Ridimensiona le fessure nella selezione": the slot width looked for, the new width, tolerance.
#[derive(Clone, Debug)]
pub struct SlotDialog {
    pub width: f64,
    pub new_width: f64,
    pub tol: f64,
}

/// The Strumenti menu, in the original's order and words.
pub fn rows(app_tool: Tool, has_sel: bool, image_sel: bool) -> Vec<FileRow> {
    use FileRow::*;
    let mac = cfg!(target_os = "macos");
    let (cmd, alt, alt_shift) = if mac { ("⌘", "⌥", "⌥⇧") } else { ("Ctrl+", "Alt+", "Alt+Maiusc+") };
    let k = |a: &str, b: &str| -> &'static str { Box::leak(format!("{a}{b}").into_boxed_str()) };
    let it = |id: &str, label: &str, keys: &'static str, on: bool| Item(id.into(), label.into(), keys, on);
    let shapes: Vec<FileRow> = crate::app::SHAPE_TOOLS
        .iter()
        .map(|t| {
            let check = if *t == app_tool { "✓ " } else { "    " };
            let keys = match t {
                Tool::Rect => k(cmd, "R"),
                Tool::Ellipse => k(cmd, "E"),
                _ => "",
            };
            it(&crate::panels::tool_menu_id(*t), &format!("{check}{}", crate::panels::tool_it(*t)), keys, true)
        })
        .collect();
    vec![
        it(&crate::panels::tool_menu_id(Tool::Select), tr("Seleziona"), "", true),
        it(&crate::panels::tool_menu_id(Tool::Line), tr("Disegna linee"), k(cmd, "L"), true),
        Sub(tr("Disegna forma").into(), shapes),
        it("tool_nodes", tr("Modifica nodi"), k(cmd, "<"), true),
        it(&crate::panels::tool_menu_id(Tool::Trim), tr("Regola forme"), k(cmd, "K"), true),
        it(&crate::panels::tool_menu_id(Tool::Tabs), tr("Aggiungi linguette"), k(cmd, "⇥"), true),
        it(&crate::panels::tool_menu_id(Tool::Text), tr("Modifica testo"), k(cmd, "T"), true),
        it(&crate::panels::tool_menu_id(Tool::Measure), tr("Misura"), k(alt, "M"), true),
        it(&crate::panels::tool_menu_id(Tool::Barcode), tr("Crea codice a barre"), "", true),
        it("offset", tr("Forme offset"), k(alt, "O"), has_sel),
        it("weld", tr("Salda forme"), k(cmd, "W"), has_sel),
        it("bool_union", tr("Unione booleana"), k(alt, "+"), has_sel),
        it("bool_subtract", tr("Funzione Booleana Sottrazione"), k(alt, "−"), has_sel),
        it("bool_intersect", tr("Intersezione booleana"), k(alt_shift, "8"), has_sel),
        it("combine_assist", tr("Assistente booleano"), k(cmd, "B"), has_sel),
        Sep,
        it("cut_shapes", tr("Forme di taglio"), k(alt_shift, "C"), has_sel),
        it("img_adjust", tr("Regola Immagine"), k(alt, "I"), image_sel),
        it("img_trace", tr("Traccia immagine"), k(alt, "T"), image_sel),
        it("trace_files", tr("Traccia immagine multi-file  (Labs)"), "", true),
        it("text_on_path", tr("Applica tracciato al testo"), "", has_sel),
        it("img_mask", tr("Applica maschera all'immagine"), "", image_sel),
        it("img_crop", tr("Ritaglia immagine"), "", image_sel),
        it("resize_slots", tr("Ridimensiona le fessure nella selezione"), "", has_sel),
        it("warp4", tr("Selezione distorsione (4 punti)"), "", has_sel),
        it("warp16", tr("Selezione deformazione (16 punti)"), "", has_sel),
        it("tangent_circle", tr("Genera cerchio tangente"), "", true),
    ]
}

/// Cut each outline into pieces no longer than `step`, so bending keeps it smooth.
pub fn densify(pl: &Polyline, step: f64) -> Polyline {
    let n = pl.pts.len();
    if n < 2 {
        return pl.clone();
    }
    let segs = if pl.closed { n } else { n - 1 };
    let mut pts = Vec::new();
    for i in 0..segs {
        let (a, b) = (pl.pts[i], pl.pts[(i + 1) % n]);
        let k = ((a[0] - b[0]).hypot(a[1] - b[1]) / step).ceil().max(1.0) as usize;
        for j in 0..k {
            let t = j as f64 / k as f64;
            pts.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    if !pl.closed {
        pts.push(pl.pts[n - 1]);
    }
    Polyline { pts, closed: pl.closed }
}

fn bernstein(n: usize, i: usize, t: f64) -> f64 {
    let c = (0..i).fold(1.0, |acc, k| acc * (n - k) as f64 / (k + 1) as f64);
    c * t.powi(i as i32) * (1.0 - t).powi((n - i) as i32)
}

/// Where the warp sends `p`: a Bézier patch over the box (2 × 2 points: bilinear; 4 × 4: bicubic).
fn warp_point(w: &Warp, p: P2) -> P2 {
    let (u, v) = ((p[0] - w.bbox.min[0]) / w.bbox.w().max(1e-9), (p[1] - w.bbox.min[1]) / w.bbox.h().max(1e-9));
    let d = w.n - 1;
    let mut out = [0.0, 0.0];
    for j in 0..w.n {
        let bv = bernstein(d, j, v);
        for i in 0..w.n {
            let b = bernstein(d, i, u) * bv;
            let c = w.ctrl[j * w.n + i];
            out[0] += b * c[0];
            out[1] += b * c[1];
        }
    }
    out
}

pub fn world_path_shape(s: &mut Shape, paths: Vec<Polyline>) {
    s.geom = Geometry::Path { subpaths: paths };
    s.tx = 0.0;
    s.ty = 0.0;
    s.sx = 1.0;
    s.sy = 1.0;
    s.rot = 0.0;
    s.tabs.clear();
    s.barcode = None;
}

fn closest_on_line(a: P2, b: P2, p: P2) -> P2 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l2 = d[0] * d[0] + d[1] * d[1];
    let t = if l2 > 0.0 { ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2 } else { 0.0 };
    [a[0] + d[0] * t, a[1] + d[1] * t]
}

/// Distance from `p` to the infinite line through a and b.
fn line_dist(a: P2, b: P2, p: P2) -> f64 {
    let q = closest_on_line(a, b, p);
    (p[0] - q[0]).hypot(p[1] - q[1])
}

/// The circle tangent to two lines whose centre is nearest the pointer: on the bisector of the
/// angle the pointer is in, at the pointer's projection onto it. (centre, radius)
fn tangent_circle(l1: (P2, P2), l2: (P2, P2), p: P2) -> Option<(P2, f64)> {
    let (a, b) = l1;
    let (c, d) = l2;
    let r = [b[0] - a[0], b[1] - a[1]];
    let s = [d[0] - c[0], d[1] - c[1]];
    let den = r[0] * s[1] - r[1] * s[0];
    if den.abs() < 1e-9 {
        // parallel lines: the circle between them, centred across from the pointer
        let w = line_dist(a, b, c);
        let m = closest_on_line(a, b, p);
        let n = closest_on_line(c, d, p);
        let centre = [(m[0] + n[0]) / 2.0, (m[1] + n[1]) / 2.0];
        return (w > 1e-6).then_some((centre, w / 2.0));
    }
    let t = ((c[0] - a[0]) * s[1] - (c[1] - a[1]) * s[0]) / den;
    let x = [a[0] + r[0] * t, a[1] + r[1] * t];
    let unit = |v: [f64; 2]| {
        let l = v[0].hypot(v[1]);
        [v[0] / l, v[1] / l]
    };
    let (u1, u2) = (unit(r), unit(s));
    let q = [p[0] - x[0], p[1] - x[1]];
    // pick each line's direction pointing to the pointer's side, the bisector between them
    let s1 = if q[0] * u1[0] + q[1] * u1[1] >= 0.0 { 1.0 } else { -1.0 };
    let s2 = if q[0] * u2[0] + q[1] * u2[1] >= 0.0 { 1.0 } else { -1.0 };
    let bis = unit([u1[0] * s1 + u2[0] * s2, u1[1] * s1 + u2[1] * s2]);
    let k = (q[0] * bis[0] + q[1] * bis[1]).max(0.0);
    let centre = [x[0] + bis[0] * k, x[1] + bis[1] * k];
    let rad = line_dist(a, b, centre);
    (rad > 1e-6).then_some((centre, rad))
}

impl RayEtch {
    fn sel_items_paths(&self) -> Vec<(Vec<u64>, Vec<Polyline>)> {
        self.selection_items()
            .into_iter()
            .map(|ids| {
                let paths = self.doc.shapes.iter().filter(|s| ids.contains(&s.id)).flat_map(|s| s.world_paths()).collect();
                (ids, paths)
            })
            .collect()
    }

    /// "Salda forme" (⌘W): every selected shape merged into one outline (union of all).
    pub fn weld(&mut self) {
        let items = self.sel_items_paths();
        if items.is_empty() {
            return;
        }
        let mut acc: Vec<Polyline> = Vec::new();
        for (_, paths) in &items {
            // each shape's own holes count (even-odd), then everything is joined
            let own: Vec<Polyline> = combine(paths, &[], CombineOp::Union).into_iter().flatten().collect();
            acc = combine(&acc, &own, CombineOp::Union).into_iter().flatten().collect();
        }
        if acc.is_empty() {
            self.status = tr("Niente da saldare: servono forme chiuse.").into();
            return;
        }
        let layer = self.doc.shapes.iter().find(|s| self.selection.contains(&s.id)).map(|s| s.layer).unwrap_or(self.current_layer);
        self.checkpoint_now();
        let sel = self.selection.clone();
        self.doc.shapes.retain(|s| !sel.contains(&s.id));
        let id = self.doc.add_shape(Shape::new(0, Geometry::Path { subpaths: acc }, layer, 0.0, 0.0));
        self.selection = vec![id];
        self.dirty = true;
        self.status = tr("Forme saldate").into();
    }

    /// "Forme di taglio" (⌥⇧C): the last selected closed shape cuts the others into the parts
    /// inside and outside it; the cutting shape goes.
    pub fn cut_shapes(&mut self) {
        let items = self.sel_items_paths();
        if items.len() < 2 {
            self.status = tr("Seleziona le forme da tagliare e, per ultima, la forma che taglia.").into();
            return;
        }
        let (cutter_ids, cutter) = items.last().cloned().unwrap();
        self.checkpoint_now();
        let mut ids = Vec::new();
        for (oids, paths) in &items[..items.len() - 1] {
            let layer = self.doc.shapes.iter().find(|s| oids.contains(&s.id)).map(|s| s.layer).unwrap_or(self.current_layer);
            let mut parts = combine(paths, &cutter, CombineOp::Intersect);
            parts.extend(combine(paths, &cutter, CombineOp::Subtract));
            if parts.is_empty() {
                continue;
            }
            self.doc.shapes.retain(|s| !oids.contains(&s.id));
            for p in parts {
                ids.push(self.doc.add_shape(Shape::new(0, Geometry::Path { subpaths: p }, layer, 0.0, 0.0)));
            }
        }
        self.doc.shapes.retain(|s| !cutter_ids.contains(&s.id));
        self.selection = ids;
        self.dirty = true;
        self.status = trf("Forme tagliate: {} parti", &[&self.selection.len()]);
    }

    /// "Applica tracciato al testo": a selected text's letters set along a selected outline,
    /// each turned to follow it, starting at the outline's first point.
    pub fn text_on_path(&mut self) {
        let sel = self.selected_shapes();
        let Some(t) = sel.iter().find(|s| matches!(s.geom, Geometry::Text { .. })) else {
            self.status = tr("Seleziona un testo e il tracciato da seguire.").into();
            return;
        };
        let Some(path) = sel.iter().find(|s| s.id != t.id && !matches!(s.geom, Geometry::Text { .. } | Geometry::Image { .. })).and_then(|s| s.world_paths().into_iter().next()) else {
            self.status = tr("Seleziona anche il tracciato da seguire.").into();
            return;
        };
        // the text stays a text (editable, its options kept) and follows a copy of the path; the
        // path itself stays where it is, as in the original
        let mut t = (*t).clone();
        if let Geometry::Text { size, style, .. } = &mut t.geom {
            *size *= t.sy.abs().max(1e-6);
            style.path = Some(path);
            style.path_offset = 0.0;
        }
        t.tx = 0.0;
        t.ty = 0.0;
        t.sx = 1.0;
        t.sy = 1.0;
        t.rot = 0.0;
        t.geom.rebuild_text();
        if t.geom.local_paths().is_empty() {
            self.status = tr("Il tracciato è troppo corto per il testo.").into();
            return;
        }
        self.checkpoint_now();
        let tid = t.id;
        if let Some(s) = self.doc.find_mut(tid) {
            *s = t;
        }
        self.selection = vec![tid];
        self.dirty = true;
        self.status = tr("Testo applicato al tracciato: trascinalo per farlo scorrere lungo il tracciato").into();
    }

    /// "Ritaglia immagine": a picture cut to the selected closed shape's box, white outside it.
    pub fn crop_image(&mut self) {
        let sel = self.selected_shapes();
        let Some(img) = sel.iter().find(|s| matches!(s.geom, Geometry::Image { .. })).cloned() else { return };
        let rings: Vec<Polyline> = sel.iter().filter(|s| !matches!(s.geom, Geometry::Image { .. })).flat_map(|s| s.world_paths()).filter(|p| p.closed).collect();
        if rings.is_empty() || img.rot.abs() > 1e-9 {
            self.status = tr("Seleziona l'immagine (non ruotata) e una forma chiusa con cui ritagliarla.").into();
            return;
        }
        let Geometry::Image { w, h, px_w, px_h, gray } = &img.geom else { return };
        let mut b = BBox::empty();
        for r in &rings {
            for p in &r.pts {
                b.add(*p);
            }
        }
        // the box in the picture's pixels
        let to_px = |p: P2| {
            let l = img.to_local(p);
            ((l[0] / w + 0.5) * *px_w as f64, (0.5 - l[1] / h) * *px_h as f64)
        };
        let (a, c) = (to_px([b.min[0], b.max[1]]), to_px([b.max[0], b.min[1]]));
        let (x0, x1) = (a.0.min(c.0).floor().max(0.0) as usize, (a.0.max(c.0).ceil() as usize).min(*px_w as usize));
        let (y0, y1) = (a.1.min(c.1).floor().max(0.0) as usize, (a.1.max(c.1).ceil() as usize).min(*px_h as usize));
        if x1 <= x0 || y1 <= y0 {
            self.status = tr("La forma non copre l'immagine.").into();
            return;
        }
        let (nw, nh) = (x1 - x0, y1 - y0);
        let (kx, ky) = (*w / *px_w as f64, *h / *px_h as f64);
        let mut ng = Vec::with_capacity(nw * nh);
        for r in y0..y1 {
            for c in x0..x1 {
                let l = [-*w / 2.0 + (c as f64 + 0.5) * kx, *h / 2.0 - (r as f64 + 0.5) * ky];
                let v = gray[r * *px_w as usize + c];
                ng.push(if point_in_rings(img.to_world(l), &rings) { v } else { 255 });
            }
        }
        // the new picture's centre, in world mm
        let lc = [-*w / 2.0 + (x0 + x1) as f64 / 2.0 * kx, *h / 2.0 - (y0 + y1) as f64 / 2.0 * ky];
        let wc = img.to_world(lc);
        self.checkpoint_now();
        if let Some(s) = self.doc.shapes.iter_mut().find(|s| s.id == img.id) {
            s.geom = Geometry::Image { w: nw as f64 * kx, h: nh as f64 * ky, px_w: nw as u32, px_h: nh as u32, gray: ng };
            s.tx = wc[0];
            s.ty = wc[1];
        }
        self.textures.remove(&img.id);
        self.texture_keys.remove(&img.id);
        self.selection = vec![img.id];
        self.dirty = true;
        self.status = tr("Immagine ritagliata").into();
    }

    /// "Ridimensiona le fessure nella selezione": rectangular slots (closed four-corner outlines
    /// with square corners) whose narrow side is `width` ± `tol` get `new_width`, about their
    /// middle; the long side stays.
    pub fn resize_slots(&mut self, width: f64, new_width: f64, tol: f64) -> usize {
        self.checkpoint_now();
        let sel = self.selection.clone();
        let mut n = 0;
        for s in self.doc.shapes.iter_mut().filter(|s| sel.contains(&s.id) && !matches!(s.geom, Geometry::Image { .. })) {
            let mut paths = s.world_paths();
            let mut changed = false;
            for p in paths.iter_mut().filter(|p| p.closed && p.pts.len() == 4) {
                let e: Vec<P2> = (0..4).map(|i| [p.pts[(i + 1) % 4][0] - p.pts[i][0], p.pts[(i + 1) % 4][1] - p.pts[i][1]]).collect();
                let len = |v: P2| v[0].hypot(v[1]);
                let square = (0..4).all(|i| (e[i][0] * e[(i + 1) % 4][0] + e[i][1] * e[(i + 1) % 4][1]).abs() <= 1e-3 * len(e[i]) * len(e[(i + 1) % 4]));
                if !square {
                    continue;
                }
                // the narrow side: edges 0/2 or 1/3
                let k = if len(e[0]) <= len(e[1]) { 0 } else { 1 };
                let cur = len(e[k]);
                if (cur - width).abs() > tol || cur < 1e-9 {
                    continue;
                }
                let u = [e[k][0] / cur, e[k][1] / cur];
                let grow = (new_width - cur) / 2.0;
                // corners at the start of edge k move back along it, the others forward
                let back = [k, (k + 3) % 4];
                for i in 0..4 {
                    let sgn = if back.contains(&i) { -1.0 } else { 1.0 };
                    p.pts[i] = [p.pts[i][0] + u[0] * grow * sgn, p.pts[i][1] + u[1] * grow * sgn];
                }
                changed = true;
                n += 1;
            }
            if changed {
                world_path_shape(s, paths);
            }
        }
        if n == 0 {
            self.undo.pop();
        }
        self.dirty |= n > 0;
        self.status = trf("Fessure ridimensionate: {}", &[&n]);
        n
    }

    /// "Traccia immagine multi-file (Labs)": several pictures chosen at once, each traced and
    /// added side by side (threshold 128, specks under 4 pixels left out: estimated).
    pub fn trace_files(&mut self) {
        let Some(files) = rfd::FileDialog::new().add_filter(tr("Immagini"), crate::formats::IMAGE_EXTS).pick_files() else { return };
        let mut x = 10.0;
        let mut done = 0;
        for f in files {
            let Ok(mut img) = crate::import::import_image(&f) else { continue };
            let b = img.bbox();
            img.tx = x + b.w() / 2.0;
            img.ty = 10.0 + b.h() / 2.0;
            img.layer = self.current_layer;
            let id = self.doc.add_shape(img);
            let n = self.trace_image(id, 128, 4);
            self.doc.shapes.retain(|s| s.id != id);
            if n > 0 {
                done += 1;
            }
            x += b.w() + 5.0;
        }
        self.status = trf("Immagini tracciate: {}", &[&done]);
    }

    /// Start a 4- or 16-point warp on the selection.
    pub fn start_warp(&mut self, n: usize) {
        let shapes: Vec<Shape> = self.selected_shapes().into_iter().filter(|s| !matches!(s.geom, Geometry::Image { .. })).collect();
        let mut b = BBox::empty();
        for s in &shapes {
            b.union(&s.bbox());
        }
        if shapes.is_empty() || !b.is_valid() || b.w() < 1e-6 || b.h() < 1e-6 {
            self.status = tr("Seleziona le forme vettoriali da deformare.").into();
            return;
        }
        self.checkpoint_now();
        let orig = shapes.iter().map(|s| (s.id, s.world_paths().iter().map(|p| densify(p, WARP_STEP_MM)).collect())).collect();
        let ctrl = (0..n * n).map(|k| {
            let (i, j) = (k % n, k / n);
            [b.min[0] + b.w() * i as f64 / (n - 1) as f64, b.min[1] + b.h() * j as f64 / (n - 1) as f64]
        }).collect();
        self.warp = Some(Warp { n, bbox: b, orig, ctrl, drag: None });
        self.tool = Tool::Warp;
        self.status = tr("Trascina i punti per deformare la selezione · Invio applica · Esc annulla").into();
    }

    fn apply_warp(&mut self) {
        let Some(w) = self.warp.clone() else { return };
        for (id, paths) in &w.orig {
            let bent: Vec<Polyline> = paths.iter().map(|p| Polyline { pts: p.pts.iter().map(|q| warp_point(&w, *q)).collect(), closed: p.closed }).collect();
            if let Some(s) = self.doc.shapes.iter_mut().find(|s| s.id == *id) {
                world_path_shape(s, bent);
            }
        }
        self.dirty = true;
    }

    /// End the warp: keep it (Invio, another tool) or put the shapes back (Esc).
    pub fn finish_warp(&mut self, keep: bool) {
        if self.warp.take().is_some() && !keep {
            if let Some(d) = self.undo.pop() {
                self.doc = d;
            }
        }
        if self.tool == Tool::Warp {
            self.tool = Tool::Select;
        }
    }

    /// Modifica nodi: Canc removes the picked node (an outline keeps at least two).
    pub fn delete_node(&mut self) {
        let Some(id) = self.node_edit.shape else { return };
        let Some((pi, ni)) = self.node_edit.picked else { return };
        let Some(s) = self.doc.shapes.iter().find(|s| s.id == id) else { return };
        let mut paths = s.geom.local_paths();
        let Some(p) = paths.get_mut(pi) else { return };
        if p.pts.len() <= 2 || ni >= p.pts.len() {
            return;
        }
        self.checkpoint_now();
        p.pts.remove(ni);
        if p.closed && p.pts.len() < 3 {
            p.closed = false;
        }
        if let Some(s) = self.doc.shapes.iter_mut().find(|s| s.id == id) {
            s.geom = Geometry::Path { subpaths: paths };
            s.barcode = None;
        }
        self.node_edit.picked = None;
        self.dirty = true;
    }
}

/// Modifica nodi on the canvas: click a shape to show its nodes; drag a node to move it,
/// double-click an outline to add a node there, Canc removes the picked node.
pub fn canvas_nodes(app: &mut RayEtch, resp: &egui::Response, painter: &egui::Painter, xf: &Xf, hover: Option<Pos2>, tol: f64) {
    let raw = hover.map(|h| xf.to_world(h));
    let node_at = |app: &RayEtch, pos: Pos2| -> Option<(usize, usize)> {
        let s = app.doc.shapes.iter().find(|s| Some(s.id) == app.node_edit.shape)?;
        for (pi, p) in s.world_paths().iter().enumerate() {
            for (ni, q) in p.pts.iter().enumerate() {
                if xf.to_screen(*q).distance(pos) <= NODE_GRAB_PX {
                    return Some((pi, ni));
                }
            }
        }
        None
    };
    if resp.drag_started_by(egui::PointerButton::Primary) {
        if let Some(pos) = resp.interact_pointer_pos() {
            if let Some(hit) = node_at(app, pos) {
                app.checkpoint_now();
                app.node_edit.picked = Some(hit);
                app.node_edit.dragging = true;
            }
        }
    }
    if app.node_edit.dragging && resp.dragged_by(egui::PointerButton::Primary) {
        if let (Some(id), Some((pi, ni)), Some(pos)) = (app.node_edit.shape, app.node_edit.picked, resp.interact_pointer_pos()) {
            let w = xf.to_world(pos);
            if let Some(s) = app.doc.shapes.iter_mut().find(|s| s.id == id) {
                let l = s.to_local(w);
                let mut paths = s.geom.local_paths();
                if let Some(q) = paths.get_mut(pi).and_then(|p| p.pts.get_mut(ni)) {
                    *q = l;
                }
                s.geom = Geometry::Path { subpaths: paths };
                s.barcode = None;
                app.dirty = true;
            }
        }
    }
    if resp.drag_stopped_by(egui::PointerButton::Primary) {
        app.node_edit.dragging = false;
    }
    if resp.double_clicked() {
        // a new node where the outline was double-clicked
        if let (Some(id), Some(p)) = (app.node_edit.shape, raw) {
            if let Some(s) = app.doc.shapes.iter().find(|s| s.id == id).cloned() {
                let world = s.world_paths();
                let mut best: Option<(usize, usize, f64)> = None;
                for (pi, pl) in world.iter().enumerate() {
                    let n = pl.pts.len();
                    let segs = if pl.closed { n } else { n.saturating_sub(1) };
                    for i in 0..segs {
                        let d = dist_seg(p, pl.pts[i], pl.pts[(i + 1) % n]);
                        if d <= tol && best.is_none_or(|b| d < b.2) {
                            best = Some((pi, i, d));
                        }
                    }
                }
                if let Some((pi, i, _)) = best {
                    app.checkpoint_now();
                    let mut paths = s.geom.local_paths();
                    paths[pi].pts.insert(i + 1, s.to_local(closest_on_line(world[pi].pts[i], world[pi].pts[(i + 1) % world[pi].pts.len()], p)));
                    if let Some(sm) = app.doc.shapes.iter_mut().find(|q| q.id == id) {
                        sm.geom = Geometry::Path { subpaths: paths };
                        sm.barcode = None;
                    }
                    app.node_edit.picked = Some((pi, i + 1));
                    app.dirty = true;
                }
            }
        }
    } else if resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            if let Some(hit) = node_at(app, pos) {
                app.node_edit.picked = Some(hit);
            } else if let Some(p) = raw {
                let hit = crate::canvas::hit_test(app, p, tol).filter(|id| app.doc.shapes.iter().any(|s| s.id == *id && !matches!(s.geom, Geometry::Image { .. })));
                app.node_edit = NodeEdit { shape: hit, picked: None, dragging: false };
                app.selection = hit.into_iter().collect();
            }
        }
    }
    // the nodes
    if let Some(s) = app.doc.shapes.iter().find(|s| Some(s.id) == app.node_edit.shape) {
        let accent = Color32::from_rgb(255, 140, 40);
        for (pi, p) in s.world_paths().iter().enumerate() {
            for (ni, q) in p.pts.iter().enumerate() {
                let c = xf.to_screen(*q);
                let r = egui::Rect::from_center_size(c, egui::vec2(7.0, 7.0));
                if app.node_edit.picked == Some((pi, ni)) {
                    painter.rect_filled(r, 0.0, accent);
                } else {
                    painter.rect_filled(r, 0.0, Color32::WHITE);
                    painter.rect_stroke(r, 0.0, Stroke::new(1.0, accent), egui::StrokeKind::Middle);
                }
            }
        }
    }
    app.status = tr("Modifica nodi: trascina un nodo · doppio clic sul contorno aggiunge un nodo · Canc lo elimina").into();
}

/// The warp on the canvas: drag the control points; the shapes follow.
pub fn canvas_warp(app: &mut RayEtch, resp: &egui::Response, painter: &egui::Painter, xf: &Xf) {
    let Some(w) = app.warp.as_ref() else {
        app.tool = Tool::Select;
        return;
    };
    let n = w.n;
    if resp.drag_started_by(egui::PointerButton::Primary) {
        if let Some(pos) = resp.interact_pointer_pos() {
            let pick = w.ctrl.iter().position(|c| xf.to_screen(*c).distance(pos) <= NODE_GRAB_PX + 2.0);
            if let Some(wm) = app.warp.as_mut() {
                wm.drag = pick;
            }
        }
    }
    if resp.dragged_by(egui::PointerButton::Primary) {
        if let (Some(pos), Some(wm)) = (resp.interact_pointer_pos(), app.warp.as_mut()) {
            if let Some(i) = wm.drag {
                wm.ctrl[i] = xf.to_world(pos);
                app.apply_warp();
            }
        }
    }
    if resp.drag_stopped_by(egui::PointerButton::Primary) {
        if let Some(wm) = app.warp.as_mut() {
            wm.drag = None;
        }
    }
    let Some(w) = app.warp.as_ref() else { return };
    let col = Color32::from_rgb(60, 160, 255);
    for j in 0..n {
        for i in 0..n {
            let c = xf.to_screen(w.ctrl[j * n + i]);
            if i + 1 < n {
                painter.line_segment([c, xf.to_screen(w.ctrl[j * n + i + 1])], Stroke::new(1.0, col.gamma_multiply(0.6)));
            }
            if j + 1 < n {
                painter.line_segment([c, xf.to_screen(w.ctrl[(j + 1) * n + i])], Stroke::new(1.0, col.gamma_multiply(0.6)));
            }
        }
    }
    for c in &w.ctrl {
        let p = xf.to_screen(*c);
        painter.circle_filled(p, 5.0, Color32::WHITE);
        painter.circle_stroke(p, 5.0, Stroke::new(1.5, col));
    }
}

/// "Genera cerchio tangente" on the canvas: click two straight pieces of outline, then click
/// where the circle should sit; it touches both lines.
pub fn canvas_tangent(app: &mut RayEtch, resp: &egui::Response, painter: &egui::Painter, xf: &Xf, hover: Option<Pos2>, tol: f64) {
    let raw = hover.map(|h| xf.to_world(h));
    let col = Color32::from_rgb(255, 60, 200);
    for (a, b) in &app.tangent.lines {
        painter.line_segment([xf.to_screen(*a), xf.to_screen(*b)], Stroke::new(3.0, col));
    }
    let preview = match (app.tangent.lines.as_slice(), raw) {
        ([l1, l2], Some(p)) => tangent_circle(*l1, *l2, p),
        _ => None,
    };
    if let Some((c, r)) = preview {
        painter.circle_stroke(xf.to_screen(c), (r as f32) * xf.zoom, Stroke::new(1.5, col));
    }
    if resp.clicked() {
        if let Some(p) = raw {
            if app.tangent.lines.len() < 2 {
                // the straight piece under the pointer
                let mut best: Option<((P2, P2), f64)> = None;
                for s in app.doc.shapes.iter().filter(|s| !matches!(s.geom, Geometry::Image { .. })) {
                    for pl in s.world_paths() {
                        let n = pl.pts.len();
                        let segs = if pl.closed { n } else { n.saturating_sub(1) };
                        for i in 0..segs {
                            let (a, b) = (pl.pts[i], pl.pts[(i + 1) % n]);
                            let d = dist_seg(p, a, b);
                            if d <= tol && best.is_none_or(|x| d < x.1) {
                                best = Some(((a, b), d));
                            }
                        }
                    }
                }
                if let Some((l, _)) = best {
                    app.tangent.lines.push(l);
                }
            } else if let Some((c, r)) = preview {
                app.checkpoint_now();
                let id = app.doc.add_shape(Shape::new(0, Geometry::Ellipse { rx: r, ry: r }, app.current_layer, c[0], c[1]));
                app.selection = vec![id];
                app.tangent.lines.clear();
                app.dirty = true;
            }
        }
    }
    app.status = match app.tangent.lines.len() {
        0 => tr("Cerchio tangente: fai clic sulla prima linea"),
        1 => tr("Cerchio tangente: fai clic sulla seconda linea"),
        _ => tr("Cerchio tangente: fai clic dove mettere il cerchio"),
    }
    .into();
}

/// The "Ridimensiona le fessure" window.
pub fn slot_window(app: &mut RayEtch, ctx: &egui::Context) {
    let Some(mut d) = app.slot_dialog.take() else { return };
    let mut open = true;
    let mut act: Option<bool> = None;
    crate::theme::centered(ctx, "slots", egui::Window::new(tr("Ridimensiona le fessure nella selezione")).open(&mut open).resizable(false).collapsible(false)).show(ctx, |ui| {
        egui::Grid::new("slots").num_columns(2).spacing(egui::vec2(10.0, 6.0)).show(ui, |ui| {
            ui.label(tr("Larghezza fessura attuale"));
            ui.add(crate::i18n::dv(&mut d.width).speed(0.01).range(0.0..=1000.0).max_decimals(3).suffix(" mm"));
            ui.end_row();
            ui.label(tr("Nuova larghezza"));
            ui.add(crate::i18n::dv(&mut d.new_width).speed(0.01).range(0.0..=1000.0).max_decimals(3).suffix(" mm"));
            ui.end_row();
            ui.label(tr("Tolleranza"));
            ui.add(crate::i18n::dv(&mut d.tol).speed(0.005).range(0.0..=10.0).max_decimals(3).suffix(" mm"));
            ui.end_row();
        });
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
        Some(true) => {
            app.resize_slots(d.width, d.new_width, d.tol);
        }
        Some(false) => {}
        None if open => app.slot_dialog = Some(d),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warp_starts_as_identity_and_tangent_circle_touches_both_lines() {
        let mut b = BBox::empty();
        b.add([0.0, 0.0]);
        b.add([10.0, 20.0]);
        for n in [2usize, 4] {
            let ctrl = (0..n * n).map(|k| [10.0 * (k % n) as f64 / (n - 1) as f64, 20.0 * (k / n) as f64 / (n - 1) as f64]).collect();
            let w = Warp { n, bbox: b, orig: Vec::new(), ctrl, drag: None };
            let q = warp_point(&w, [3.0, 7.0]);
            assert!((q[0] - 3.0).abs() < 1e-9 && (q[1] - 7.0).abs() < 1e-9, "{q:?}");
        }
        // the x and y axes; pointer in the first quadrant: centre on the diagonal
        let (c, r) = tangent_circle(([0.0, 0.0], [10.0, 0.0]), ([0.0, 0.0], [0.0, 10.0]), [5.0, 5.0]).unwrap();
        assert!((c[0] - 5.0).abs() < 1e-9 && (c[1] - 5.0).abs() < 1e-9 && (r - 5.0).abs() < 1e-9);
    }

}
