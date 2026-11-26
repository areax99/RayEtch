//! The Modifica menu's shape and selection tools, as the original's MainWindow::on_action*
//! handlers (reference picture 25): undo history, invert selection, paste in place, convert to
//! path / bitmap, cut path, close paths, auto-join, optimise, duplicates, reverse direction,
//! the selection helpers, "flash" the current layer, and the Opzioni immagine submenu.

use crate::app::RayEtch;
use crate::i18n::{tr, trf};
use crate::model::{BBox, CutMode, Geometry, P2, Polyline, Shape, point_in_rings};
use eframe::egui;

/// AutoJoinShapes(list, true, 0.05) (on_actionAuto_Join_selected_shapes_triggered 0x100c68ee4).
pub const AUTO_JOIN_TOL: f64 = 0.05;
/// Convert to Bitmap resolution, mm per pixel (estimated: 254 dpi).
pub const BITMAP_MM_PER_PX: f64 = 0.1;
/// How long "Mostra forme nel livello corrente" flashes them, seconds (estimated).
pub const FLASH_SECONDS: f64 = 1.5;

/// A window one of these tools has open.
pub enum EditDialog {
    /// "Chiudi i percorsi selezionati con tolleranza" (CloseShapesWidget / ShapeCloser::Compute)
    CloseTolerance { tol: f64 },
    /// "Ottimizza le forme selezionate" (OptimizeShapesDialog / ShapeOptimizer::Compute(f, f))
    Optimize { simplify: f64, join: f64, remove_dups: bool },
    /// "Elimina duplicati": the duplicates found, waiting for a yes
    ConfirmDuplicates { ids: Vec<u64> },
    /// "Seleziona cerchi" (CircleSelector: "Select circles, optionally by size")
    Circles { by_size: bool, min: f64, max: f64 },
    /// Opzioni immagine > Traccia immagine (TraceImageDialog)
    Trace { image: u64, threshold: u8, ignore: usize },
    /// Opzioni immagine > Regola immagine (AdjustImageDialog: Brightness, Contrast, Gamma)
    Adjust { image: u64, orig: Vec<u8>, brightness: f64, contrast: f64, gamma: f64 },
}

fn is_image(s: &Shape) -> bool {
    matches!(s.geom, Geometry::Image { .. })
}

fn area(b: &BBox) -> f64 {
    b.w() * b.h()
}

/// Join open outlines whose ends are within `tol`; outlines whose two ends meet are closed.
pub fn join_paths(paths: Vec<Polyline>, tol: f64, close: bool) -> Vec<Polyline> {
    let near = |a: P2, b: P2| (a[0] - b[0]).hypot(a[1] - b[1]) <= tol;
    let (mut closed, mut open): (Vec<Polyline>, Vec<Polyline>) = paths.into_iter().partition(|p| p.closed || p.pts.len() < 2);
    let mut out: Vec<Polyline> = Vec::new();
    while let Some(mut cur) = open.pop() {
        loop {
            let (a0, a1) = (cur.pts[0], *cur.pts.last().unwrap());
            let found = open.iter().position(|o| {
                let (b0, b1) = (o.pts[0], *o.pts.last().unwrap());
                near(a1, b0) || near(a1, b1) || near(a0, b1) || near(a0, b0)
            });
            let Some(i) = found else { break };
            let mut o = open.swap_remove(i);
            let (b0, b1) = (o.pts[0], *o.pts.last().unwrap());
            if near(a1, b0) {
                cur.pts.extend(o.pts.drain(1..));
            } else if near(a1, b1) {
                o.pts.reverse();
                cur.pts.extend(o.pts.drain(1..));
            } else if near(a0, b1) {
                o.pts.extend(cur.pts.drain(1..));
                cur = o;
            } else {
                o.pts.reverse();
                o.pts.extend(cur.pts.drain(1..));
                cur = o;
            }
        }
        if close && cur.pts.len() > 2 && near(cur.pts[0], *cur.pts.last().unwrap()) {
            cur.pts.pop();
            cur.closed = true;
        }
        out.push(cur);
    }
    closed.extend(out);
    closed
}

/// Douglas–Peucker: drop points closer than `tol` to the line their neighbours make.
pub fn simplify(pl: &Polyline, tol: f64) -> Polyline {
    if pl.pts.len() < 3 || tol <= 0.0 {
        return pl.clone();
    }
    let mut pts = pl.pts.clone();
    if pl.closed {
        pts.push(pl.pts[0]);
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    *keep.last_mut().unwrap() = true;
    let mut stack = vec![(0, pts.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let mut best = (0, 0.0);
        for i in a + 1..b {
            let d = crate::model::dist_seg(pts[i], pts[a], pts[b]);
            if d > best.1 {
                best = (i, d);
            }
        }
        if best.1 > tol {
            keep[best.0] = true;
            stack.push((a, best.0));
            stack.push((best.0, b));
        }
    }
    let mut out: Vec<P2> = pts.iter().zip(&keep).filter(|(_, k)| **k).map(|(p, _)| *p).collect();
    if pl.closed {
        out.pop();
    }
    if pl.closed && out.len() < 3 {
        return pl.clone();
    }
    Polyline { pts: out, closed: pl.closed }
}

fn same_paths(a: &[Polyline], b: &[Polyline], tol: f64) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(p, q)| {
            p.closed == q.closed && p.pts.len() == q.pts.len() && {
                let fwd = p.pts.iter().zip(&q.pts).all(|(x, y)| (x[0] - y[0]).abs() <= tol && (x[1] - y[1]).abs() <= tol);
                fwd || p.pts.iter().zip(q.pts.iter().rev()).all(|(x, y)| (x[0] - y[0]).abs() <= tol && (x[1] - y[1]).abs() <= tol)
            }
        })
}

/// Whether a shape is a circle: an ellipse with equal radii, or a closed outline of 8 points or
/// more all about as far from their centre (within 2 %, estimated). Its diameter if so.
fn circle_diameter(s: &Shape) -> Option<f64> {
    if let Geometry::Ellipse { rx, ry } = s.geom {
        let (a, b) = ((rx * s.sx).abs(), (ry * s.sy).abs());
        return ((a - b).abs() <= 0.01 * a.max(b)).then_some(a + b);
    }
    if is_image(s) {
        return None;
    }
    let paths = s.world_paths();
    let [p] = paths.as_slice() else { return None };
    if !p.closed || p.pts.len() < 8 {
        return None;
    }
    let n = p.pts.len() as f64;
    let c = p.pts.iter().fold([0.0, 0.0], |a, q| [a[0] + q[0] / n, a[1] + q[1] / n]);
    let ds: Vec<f64> = p.pts.iter().map(|q| (q[0] - c[0]).hypot(q[1] - c[1])).collect();
    let (lo, hi) = ds.iter().fold((f64::MAX, 0.0f64), |(l, h), d| (l.min(*d), h.max(*d)));
    (hi > 0.0 && (hi - lo) <= 0.02 * hi).then_some(lo + hi)
}

impl RayEtch {
    fn sel_shapes_mut(&mut self) -> impl Iterator<Item = &mut Shape> {
        let sel = self.selection.clone();
        self.doc.shapes.iter_mut().filter(move |s| sel.contains(&s.id))
    }

    fn visible(&self, s: &Shape) -> bool {
        self.doc.layers.get(s.layer).is_some_and(|l| l.show)
    }

    /// The layer picked in the layer list, else the drawing layer.
    pub fn picked_layer(&self) -> usize {
        self.selected_row.unwrap_or(self.current_layer)
    }

    /// "Inverti selezione" (on_actionInvert_Selection_triggered 0x100c68480): every visible
    /// shape that isn't selected becomes the selection.
    pub fn invert_selection(&mut self) {
        let sel = self.selection.clone();
        self.selection = self.doc.shapes.iter().filter(|s| self.visible(s) && !sel.contains(&s.id)).map(|s| s.id).collect();
    }

    /// "Incolla in posizione" (HandlePaste(false, false) 0x100c54a60): where the copies were.
    pub fn paste_in_place(&mut self) {
        self.paste(0.0);
    }

    /// "Converti in percorso" (0x100c66dec): shapes, text and codes become plain outlines.
    pub fn convert_to_path(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.checkpoint_now();
        for s in self.sel_shapes_mut().filter(|s| !is_image(s)) {
            s.geom = Geometry::Path { subpaths: s.geom.local_paths() };
            s.barcode = None;
        }
        self.dirty = true;
        self.status = tr("Convertito in percorso").into();
    }

    /// "Converti in bitmap" (0x100c67338): the selected vectors drawn into one picture (closed
    /// shapes filled even-odd, open ones as one-pixel lines) at 0.1 mm per pixel, replacing them.
    pub fn convert_to_bitmap(&mut self) {
        let shapes: Vec<Shape> = self.selected_shapes().into_iter().filter(|s| !is_image(s)).collect();
        if shapes.is_empty() {
            self.status = tr("Seleziona forme vettoriali da convertire.").into();
            return;
        }
        let mut b = BBox::empty();
        for s in &shapes {
            b.union(&s.bbox());
        }
        if !b.is_valid() {
            return;
        }
        let k = BITMAP_MM_PER_PX;
        let (pw, ph) = (((b.w() / k).ceil() as usize).clamp(1, 4000), ((b.h() / k).ceil() as usize).clamp(1, 4000));
        let mut gray = vec![255u8; pw * ph];
        let paths: Vec<Polyline> = shapes.iter().flat_map(|s| s.world_paths()).collect();
        let to_px = |p: P2| ((p[0] - b.min[0]) / k, (b.max[1] - p[1]) / k);
        // closed outlines: even-odd scanlines through each pixel row's centre
        for r in 0..ph {
            let y = b.max[1] - (r as f64 + 0.5) * k;
            let mut xs: Vec<f64> = Vec::new();
            for pl in paths.iter().filter(|p| p.closed) {
                let n = pl.pts.len();
                for i in 0..n {
                    let (a, c) = (pl.pts[i], pl.pts[(i + 1) % n]);
                    if (a[1] > y) != (c[1] > y) {
                        xs.push(a[0] + (y - a[1]) / (c[1] - a[1]) * (c[0] - a[0]));
                    }
                }
            }
            xs.sort_by(|a, c| a.total_cmp(c));
            for pair in xs.chunks(2) {
                if let [x0, x1] = pair {
                    let (c0, c1) = (((x0 - b.min[0]) / k).round().max(0.0) as usize, (((x1 - b.min[0]) / k).round() as usize).min(pw));
                    for c in c0..c1 {
                        gray[r * pw + c] = 0;
                    }
                }
            }
        }
        // every outline also drawn as a line, so open shapes and thin parts show
        for pl in &paths {
            let n = pl.pts.len();
            let segs = if pl.closed { n } else { n.saturating_sub(1) };
            for i in 0..segs {
                let (a, c) = (to_px(pl.pts[i]), to_px(pl.pts[(i + 1) % n]));
                let steps = ((c.0 - a.0).abs().max((c.1 - a.1).abs()).ceil() as usize).max(1);
                for t in 0..=steps {
                    let f = t as f64 / steps as f64;
                    let (x, y) = (a.0 + (c.0 - a.0) * f, a.1 + (c.1 - a.1) * f);
                    let (x, y) = (x.floor() as isize, y.floor() as isize);
                    if x >= 0 && y >= 0 && (x as usize) < pw && (y as usize) < ph {
                        gray[y as usize * pw + x as usize] = 0;
                    }
                }
            }
        }
        self.checkpoint_now();
        let layer = shapes[0].layer;
        let sel = self.selection.clone();
        self.doc.shapes.retain(|s| !(sel.contains(&s.id) && !is_image(s)));
        let c = b.center();
        let img = Shape::new(0, Geometry::Image { w: pw as f64 * k, h: ph as f64 * k, px_w: pw as u32, px_h: ph as u32, gray }, layer, c[0], c[1]);
        let id = self.doc.add_shape(img);
        self.selection = vec![id];
        self.dirty = true;
        self.status = tr("Convertito in bitmap").into();
    }

    /// "Genera percorso da taglio" (on_actionConvert_to_cut_triggered 0x100c67c20): the path the
    /// laser will really follow for the selection (fill lines included), added as a group "Cut
    /// Path" on each piece's layer.
    pub fn generate_cut_path(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        let opts = crate::gcode::JobOptions { selected_only: true, ..self.job_opts.clone() };
        let job = crate::gcode::plan(&self.doc, &self.selection, &opts);
        let mut pieces: Vec<(usize, Polyline)> = Vec::new();
        let mut cur: Option<(usize, Polyline)> = None;
        let mut last = job.start;
        for m in &job.moves {
            if matches!(m.kind, crate::gcode::MoveKind::Cut) {
                match &mut cur {
                    Some((l, pl)) if *l == m.layer => pl.pts.push(m.to),
                    _ => {
                        if let Some(c) = cur.take() {
                            pieces.push(c);
                        }
                        cur = Some((m.layer, Polyline { pts: vec![last, m.to], closed: false }));
                    }
                }
            } else if let Some(c) = cur.take() {
                pieces.push(c);
            }
            last = m.to;
        }
        if let Some(c) = cur.take() {
            pieces.push(c);
        }
        if pieces.is_empty() {
            self.status = tr("Nessun percorso di taglio: controlla che il livello sia attivo.").into();
            return;
        }
        self.checkpoint_now();
        let gid = self.doc.alloc_id();
        let mut by_layer: std::collections::BTreeMap<usize, Vec<Polyline>> = Default::default();
        for (l, pl) in pieces {
            by_layer.entry(l).or_default().push(pl);
        }
        let mut ids = Vec::new();
        for (l, paths) in by_layer {
            let mut s = Shape::new(0, Geometry::Path { subpaths: paths }, l, 0.0, 0.0);
            s.groups = vec![gid];
            ids.push(self.doc.add_shape(s));
        }
        self.selection = ids;
        self.dirty = true;
        self.status = tr("Percorso di taglio generato").into();
    }

    /// "Chiudi tracciato" (on_actionClose_Path_triggered 0x100c68064): each open outline of the
    /// selection is closed with a line back to its start.
    pub fn close_paths(&mut self) {
        self.checkpoint_now();
        let mut n = 0;
        for s in self.sel_shapes_mut().filter(|s| !is_image(s)) {
            let mut paths = s.geom.local_paths();
            for p in paths.iter_mut().filter(|p| !p.closed && p.pts.len() > 2) {
                p.closed = true;
                n += 1;
            }
            if n > 0 {
                s.geom = Geometry::Path { subpaths: paths };
                s.barcode = None;
            }
        }
        self.dirty |= n > 0;
        self.status = trf("Tracciati chiusi: {}", &[&n]);
    }

    /// Join the selected outlines within `tol` (pieces of different shapes too); each joined
    /// outline becomes a shape on its first piece's layer. `close`: ends that meet are closed.
    fn join_selected(&mut self, tol: f64, close: bool, what: &str) {
        let shapes: Vec<Shape> = self.selected_shapes().into_iter().filter(|s| !is_image(s)).collect();
        if shapes.is_empty() {
            return;
        }
        let layer = shapes[0].layer;
        let paths: Vec<Polyline> = shapes.iter().flat_map(|s| s.world_paths()).collect();
        let before = paths.iter().filter(|p| !p.closed).count();
        let joined = join_paths(paths, tol, close);
        let after = joined.iter().filter(|p| !p.closed).count();
        self.checkpoint_now();
        let sel = self.selection.clone();
        self.doc.shapes.retain(|s| !(sel.contains(&s.id) && !is_image(s)));
        let mut ids = Vec::new();
        for p in joined {
            ids.push(self.doc.add_shape(Shape::new(0, Geometry::Path { subpaths: vec![p] }, layer, 0.0, 0.0)));
        }
        self.selection = ids;
        self.dirty = true;
        self.status = trf("{}: tracciati aperti da {} a {}", &[&what, &before, &after]);
    }

    /// "Unisci automaticamente le forme selezionate" (⌥J, tolerance 0.05 mm).
    pub fn auto_join(&mut self) {
        self.join_selected(AUTO_JOIN_TOL, true, tr("Unione"));
    }

    /// "Chiudi i percorsi selezionati con tolleranza": join and close within `tol`.
    pub fn close_with_tolerance(&mut self, tol: f64) {
        self.join_selected(tol.max(0.0), true, tr("Chiusura"));
    }

    /// "Ottimizza le forme selezionate": join within `join`, drop points within `simplify`
    /// of the line, and (optionally) shapes that repeat another.
    pub fn optimize_selected(&mut self, simplify_tol: f64, join: f64, remove_dups: bool) {
        if remove_dups {
            let dups = self.find_duplicates(true);
            self.doc.shapes.retain(|s| !dups.contains(&s.id));
            self.selection.retain(|id| !dups.contains(id));
        }
        self.join_selected(join, true, tr("Ottimizzazione"));
        let sel = self.selection.clone();
        let mut pts = (0usize, 0usize);
        for s in self.doc.shapes.iter_mut().filter(|s| sel.contains(&s.id)) {
            if let Geometry::Path { subpaths } = &mut s.geom {
                for p in subpaths.iter_mut() {
                    pts.0 += p.pts.len();
                    *p = simplify(p, simplify_tol);
                    pts.1 += p.pts.len();
                }
            }
        }
        self.status = trf("Forme ottimizzate: nodi da {} a {}", &[&pts.0, &pts.1]);
    }

    /// FindDuplicateShapes: shapes on the same layer with the same outlines (within 0.01 mm,
    /// either direction) as an earlier one. In the selection, or everything when none.
    pub fn find_duplicates(&self, selection_only: bool) -> Vec<u64> {
        let pool: Vec<&Shape> =
            self.doc.shapes.iter().filter(|s| !is_image(s) && (!selection_only || self.selection.is_empty() || self.selection.contains(&s.id))).collect();
        let worlds: Vec<Vec<Polyline>> = pool.iter().map(|s| s.world_paths()).collect();
        let mut dups = Vec::new();
        for i in 0..pool.len() {
            if (0..i).any(|j| !dups.contains(&pool[j].id) && pool[j].layer == pool[i].layer && same_paths(&worlds[i], &worlds[j], 0.01)) {
                dups.push(pool[i].id);
            }
        }
        dups
    }

    /// "Elimina duplicati" (on_actionDelete_Duplicate_Shapes_triggered 0x100c69044): finds
    /// them, then asks ("%1 duplicate shapes were found - would you like to delete them?").
    pub fn delete_duplicates(&mut self) {
        let ids = self.find_duplicates(true);
        if ids.is_empty() {
            self.status = tr("Nessuna forma duplicata trovata").into();
        } else {
            self.edit_dialog = Some(EditDialog::ConfirmDuplicates { ids });
        }
    }

    /// "Inverti direzione forma" (UndoReversePath per path, 0x10067dba8).
    pub fn reverse_direction(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.checkpoint_now();
        for s in self.sel_shapes_mut().filter(|s| !is_image(s)) {
            let mut paths = s.geom.local_paths();
            for p in paths.iter_mut() {
                p.pts.reverse();
            }
            s.geom = Geometry::Path { subpaths: paths };
            s.barcode = None;
        }
        self.dirty = true;
        self.status = tr("Direzione invertita").into();
    }

    fn has_open(s: &Shape) -> bool {
        !is_image(s) && s.geom.local_paths().iter().any(|p| !p.closed && p.pts.len() > 1)
    }

    /// "Seleziona forme aperte" (SelectOpenShapes(false)).
    pub fn select_open_shapes(&mut self) {
        self.selection = self.doc.shapes.iter().filter(|s| self.visible(s) && Self::has_open(s)).map(|s| s.id).collect();
        self.status = trf("Forme aperte: {}", &[&self.selection.len()]);
    }

    /// "Seleziona le forme aperte impostate per il riempimento" (SelectOpenShapes(true)): open
    /// shapes on Fill / Offset Fill layers, which can't be filled.
    pub fn select_open_fill_shapes(&mut self) {
        self.selection = self
            .doc
            .shapes
            .iter()
            .filter(|s| self.visible(s) && Self::has_open(s) && matches!(self.doc.layers.get(s.layer).map(|l| l.mode), Some(CutMode::Fill | CutMode::OffsetFill)))
            .map(|s| s.id)
            .collect();
        self.status = trf("Forme aperte su livelli di riempimento: {}", &[&self.selection.len()]);
    }

    /// "Seleziona tutte le forme nel livello corrente" (on_actionSelect_layer_vectors 0x100c52554).
    pub fn select_layer_shapes(&mut self) {
        let l = self.picked_layer();
        self.selection = self.doc.shapes.iter().filter(|s| s.layer == l).map(|s| s.id).collect();
    }

    /// "Mostra forme nel livello corrente" ("Flash Shapes in Current Layer").
    pub fn flash_layer(&mut self, now: f64) {
        self.flash = Some((self.picked_layer(), now + FLASH_SECONDS));
    }

    /// "Seleziona le forme contenute" (0x100c827d8): one closed, single outline selected; the
    /// shapes entirely inside it are selected.
    pub fn select_contained(&mut self) {
        let [id] = self.selection.as_slice() else {
            self.status = tr("Seleziona una sola forma chiusa.").into();
            return;
        };
        let Some(c) = self.doc.shapes.iter().find(|s| s.id == *id) else { return };
        let rings = c.world_paths();
        if is_image(c) || !c.groups.is_empty() || rings.len() != 1 || !rings[0].closed {
            self.status = tr("Forma non compatibile: seleziona un percorso chiuso semplice (non un gruppo, un'immagine o una forma composta).").into();
            return;
        }
        let cid = c.id;
        self.selection = self
            .doc
            .shapes
            .iter()
            .filter(|s| s.id != cid && self.visible(s))
            .filter(|s| {
                let ps = s.world_paths();
                !ps.is_empty() && ps.iter().all(|p| p.pts.iter().all(|q| point_in_rings(*q, &rings)))
            })
            .map(|s| s.id)
            .collect();
        self.status = trf("Forme contenute: {}", &[&self.selection.len()]);
    }

    /// "Seleziona forme più piccole di quelle selezionate" (0x100c8970c): with one shape
    /// selected, every visible shape whose box area (width × height) is no bigger.
    pub fn select_smaller(&mut self) {
        let [id] = self.selection.as_slice() else {
            self.status = tr("Seleziona una sola forma di riferimento.").into();
            return;
        };
        let Some(r) = self.doc.shapes.iter().find(|s| s.id == *id) else { return };
        let limit = area(&r.bbox());
        self.selection = self.doc.shapes.iter().filter(|s| self.visible(s) && !is_image(s) && area(&s.bbox()) <= limit).map(|s| s.id).collect();
        self.status = trf("Forme più piccole: {}", &[&self.selection.len()]);
    }

    /// "Seleziona cerchi": circles, optionally only those with a diameter in [min, max].
    pub fn select_circles(&mut self, by_size: bool, min: f64, max: f64) {
        self.selection = self
            .doc
            .shapes
            .iter()
            .filter(|s| self.visible(s))
            .filter(|s| circle_diameter(s).is_some_and(|d| !by_size || (d >= min - 1e-6 && d <= max + 1e-6)))
            .map(|s| s.id)
            .collect();
        self.status = trf("Cerchi selezionati: {}", &[&self.selection.len()]);
    }

    fn selected_image(&self) -> Option<u64> {
        self.doc.shapes.iter().find(|s| self.selection.contains(&s.id) && is_image(s)).map(|s| s.id)
    }

    fn image_changed(&mut self, id: u64) {
        self.textures.remove(&id);
        self.texture_keys.remove(&id);
        self.dirty = true;
    }

    /// Opzioni immagine > "Sostituisci immagine": another picture in the same box.
    pub fn replace_image(&mut self) {
        let Some(id) = self.selected_image() else {
            self.status = tr("Seleziona un'immagine.").into();
            return;
        };
        let Some(p) = rfd::FileDialog::new().add_filter(tr("Immagini"), &["png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff", "webp", "tga"]).pick_file() else { return };
        match crate::import::import_image(&p) {
            Ok(new) => {
                self.checkpoint_now();
                if let (Some(s), Geometry::Image { px_w, px_h, gray, .. }) = (self.doc.shapes.iter_mut().find(|s| s.id == id), new.geom) {
                    if let Geometry::Image { px_w: w0, px_h: h0, gray: g0, .. } = &mut s.geom {
                        *w0 = px_w;
                        *h0 = px_h;
                        *g0 = gray;
                    }
                }
                self.image_changed(id);
                self.status = tr("Immagine sostituita").into();
            }
            Err(e) => self.status = e,
        }
    }

    /// Opzioni immagine > "Applica maschera all'immagine": with an image and closed shapes
    /// selected, the picture is kept only inside the shapes (white outside).
    pub fn apply_mask(&mut self) {
        let Some(id) = self.selected_image() else {
            self.status = tr("Seleziona un'immagine e le forme chiuse da usare come maschera.").into();
            return;
        };
        let rings: Vec<Polyline> = self.selected_shapes().iter().filter(|s| !is_image(s)).flat_map(|s| s.world_paths()).filter(|p| p.closed).collect();
        if rings.is_empty() {
            self.status = tr("Seleziona anche una forma chiusa da usare come maschera.").into();
            return;
        }
        self.checkpoint_now();
        let Some(s) = self.doc.shapes.iter_mut().find(|s| s.id == id) else { return };
        let sh = s.clone();
        if let Geometry::Image { w, h, px_w, px_h, gray } = &mut s.geom {
            for r in 0..*px_h as usize {
                for c in 0..*px_w as usize {
                    let l = [-*w / 2.0 + (c as f64 + 0.5) * *w / *px_w as f64, *h / 2.0 - (r as f64 + 0.5) * *h / *px_h as f64];
                    if !point_in_rings(sh.to_world(l), &rings) {
                        gray[r * *px_w as usize + c] = 255;
                    }
                }
            }
        }
        self.image_changed(id);
        self.status = tr("Maschera applicata").into();
    }

    /// Opzioni immagine > "Salva bitmap elaborata": the picture as its layer will engrave it
    /// (image mode, interval as pixel size), saved as a PNG.
    pub fn save_processed_bitmap(&mut self) {
        let Some(id) = self.selected_image() else {
            self.status = tr("Seleziona un'immagine.").into();
            return;
        };
        let Some(s) = self.doc.shapes.iter().find(|s| s.id == id).cloned() else { return };
        let Some(cs) = self.doc.layers.get(s.layer).cloned() else { return };
        let Geometry::Image { w, h, px_w, px_h, gray } = &s.geom else { return };
        let interval = cs.interval.max(0.02);
        let (cols, rows) = (((w * s.sx.abs()) / interval).ceil().max(1.0) as usize, ((h * s.sy.abs()) / interval).ceil().max(1.0) as usize);
        if cols * rows > 40_000_000 {
            self.status = tr("Immagine troppo grande da elaborare.").into();
            return;
        }
        let mut grid = vec![0.0f64; cols * rows];
        for r in 0..rows {
            for c in 0..cols {
                let (u, v) = (((c as f64 + 0.5) / cols as f64 * *px_w as f64) as usize, ((r as f64 + 0.5) / rows as f64 * *px_h as f64) as usize);
                let g = gray[v.min(*px_h as usize - 1) * *px_w as usize + u.min(*px_w as usize - 1)] as f64 / 255.0;
                grid[r * cols + c] = if cs.negative { g } else { 1.0 - g };
            }
        }
        crate::imageproc::apply(&cs, &mut grid, cols, rows, 0.0, rows as f64 * interval, interval);
        let px: Vec<u8> = grid.iter().map(|v| (255.0 - v.clamp(0.0, 1.0) * 255.0).round() as u8).collect();
        let Some(path) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name("bitmap-elaborata.png").save_file() else { return };
        match image::GrayImage::from_raw(cols as u32, rows as u32, px).map(|im| im.save(&path)) {
            Some(Ok(())) => self.status = trf("Bitmap elaborata salvata in {}", &[&path.display()]),
            Some(Err(e)) => self.status = trf("Salvataggio non riuscito: {}", &[&e]),
            None => {}
        }
    }

    /// Opzioni immagine > "Traccia immagine": open its window on the selected picture.
    pub fn open_trace(&mut self) {
        match self.selected_image() {
            Some(image) => self.edit_dialog = Some(EditDialog::Trace { image, threshold: 128, ignore: 4 }),
            None => self.status = tr("Seleziona un'immagine da tracciare.").into(),
        }
    }

    /// Opzioni immagine > "Regola immagine": open its window on the selected picture.
    pub fn open_adjust(&mut self) {
        let Some(image) = self.selected_image() else {
            self.status = tr("Seleziona un'immagine.").into();
            return;
        };
        let orig = match self.doc.shapes.iter().find(|s| s.id == image).map(|s| &s.geom) {
            Some(Geometry::Image { gray, .. }) => gray.clone(),
            _ => return,
        };
        self.checkpoint_now();
        self.edit_dialog = Some(EditDialog::Adjust { image, orig, brightness: 0.0, contrast: 0.0, gamma: 1.0 });
    }

    /// Trace the picture: the outlines between darker-than-`threshold` and lighter pixels
    /// (marching squares), blobs under `ignore` pixels left out, smoothed a little; added as one
    /// shape on the current layer and selected.
    pub fn trace_image(&mut self, id: u64, threshold: u8, ignore: usize) -> usize {
        let Some(s) = self.doc.shapes.iter().find(|s| s.id == id).cloned() else { return 0 };
        let Geometry::Image { w, h, px_w, px_h, gray } = &s.geom else { return 0 };
        let (pw, ph) = (*px_w as usize, *px_h as usize);
        let dark = |c: isize, r: isize| c >= 0 && r >= 0 && (c as usize) < pw && (r as usize) < ph && gray[r as usize * pw + c as usize] < threshold;
        let loops = trace_loops(pw, ph, &dark);
        let (kx, ky) = (*w / pw as f64, *h / ph as f64);
        let mut paths = Vec::new();
        for lp in loops {
            // the loop's area in pixels (shoelace), to leave out specks
            let n = lp.len();
            let a: f64 = (0..n).map(|i| lp[i].0 * lp[(i + 1) % n].1 - lp[(i + 1) % n].0 * lp[i].1).sum::<f64>().abs() / 2.0;
            if a < ignore as f64 {
                continue;
            }
            let pts: Vec<P2> = lp.iter().map(|(x, y)| s.to_world([-*w / 2.0 + x * kx, *h / 2.0 - y * ky])).collect();
            paths.push(simplify(&Polyline { pts, closed: true }, kx.min(ky) * 0.5));
        }
        if paths.is_empty() {
            self.status = tr("Niente da tracciare con questa soglia.").into();
            return 0;
        }
        let n = paths.len();
        self.checkpoint_now();
        let nid = self.doc.add_shape(Shape::new(0, Geometry::Path { subpaths: paths }, self.current_layer, 0.0, 0.0));
        self.selection = vec![nid];
        self.dirty = true;
        self.status = trf("Immagine tracciata: {} contorni", &[&n]);
        n
    }
}

/// Closed outlines around the dark pixels, in pixel units (corners at integer positions), from
/// the edges between dark and light pixels chained into loops.
fn trace_loops(pw: usize, ph: usize, dark: &dyn Fn(isize, isize) -> bool) -> Vec<Vec<(f64, f64)>> {
    use std::collections::HashMap;
    // directed edges with the dark pixel on their left, keyed by start corner
    let mut next: HashMap<(i64, i64), Vec<(i64, i64)>> = HashMap::new();
    for r in -1..=ph as isize {
        for c in -1..=pw as isize {
            if !dark(c, r) {
                continue;
            }
            let (x, y) = (c as i64, r as i64);
            if !dark(c, r - 1) {
                next.entry((x + 1, y)).or_default().push((x, y));
            }
            if !dark(c, r + 1) {
                next.entry((x, y + 1)).or_default().push((x + 1, y + 1));
            }
            if !dark(c - 1, r) {
                next.entry((x, y)).or_default().push((x, y + 1));
            }
            if !dark(c + 1, r) {
                next.entry((x + 1, y + 1)).or_default().push((x + 1, y));
            }
        }
    }
    let mut loops = Vec::new();
    let starts: Vec<(i64, i64)> = next.keys().copied().collect();
    for s in starts {
        while let Some(first) = next.get_mut(&s).and_then(|v| v.pop()) {
            let mut lp = vec![(s.0 as f64, s.1 as f64)];
            let mut cur = first;
            let mut guard = 0;
            while cur != s && guard < 10_000_000 {
                lp.push((cur.0 as f64, cur.1 as f64));
                match next.get_mut(&cur).and_then(|v| v.pop()) {
                    Some(n) => cur = n,
                    None => break,
                }
                guard += 1;
            }
            if lp.len() >= 4 {
                loops.push(lp);
            }
        }
    }
    loops
}

/// The open tool windows of this module, and the current layer's flash on the canvas.
pub fn windows(app: &mut RayEtch, ctx: &egui::Context) {
    use egui::vec2;
    let Some(mut d) = app.edit_dialog.take() else { return };
    let mut keep = true;
    let mut open = true;
    let title = match &d {
        EditDialog::CloseTolerance { .. } => tr("Chiudi i percorsi selezionati con tolleranza"),
        EditDialog::Optimize { .. } => tr("Ottimizza le forme selezionate"),
        EditDialog::ConfirmDuplicates { .. } => tr("Duplicati trovati"),
        EditDialog::Circles { .. } => tr("Seleziona cerchi"),
        EditDialog::Trace { .. } => tr("Traccia immagine"),
        EditDialog::Adjust { .. } => tr("Regola immagine"),
    };
    let mut action: Option<bool> = None;
    crate::theme::centered(ctx, "editops", egui::Window::new(title).open(&mut open).resizable(false).collapsible(false)).show(ctx, |ui| {
        ui.set_min_width(360.0);
        match &mut d {
            EditDialog::CloseTolerance { tol } => {
                ui.label(tr("Chiude i tracciati aperti le cui estremità distano meno della tolleranza, unendo i pezzi che si toccano."));
                ui.horizontal(|ui| {
                    ui.label(tr("Tolleranza"));
                    ui.add(crate::i18n::dv(tol).speed(0.01).range(0.0..=10.0).max_decimals(3).suffix(" mm"));
                });
            }
            EditDialog::Optimize { simplify, join, remove_dups } => {
                egui::Grid::new("opt").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                    ui.label(tr("Unisci estremità entro"));
                    ui.add(crate::i18n::dv(join).speed(0.01).range(0.0..=10.0).max_decimals(3).suffix(" mm"));
                    ui.end_row();
                    ui.label(tr("Riduci nodi (tolleranza)"));
                    ui.add(crate::i18n::dv(simplify).speed(0.005).range(0.0..=5.0).max_decimals(3).suffix(" mm"));
                    ui.end_row();
                });
                crate::panels::switch(ui, remove_dups, tr("Elimina forme duplicate"));
            }
            EditDialog::ConfirmDuplicates { ids } => {
                ui.label(trf("Trovate {} forme duplicate: vuoi eliminarle?", &[&ids.len()]));
            }
            EditDialog::Circles { by_size, min, max } => {
                crate::panels::switch(ui, by_size, tr("Solo cerchi di questa dimensione"));
                ui.add_enabled_ui(*by_size, |ui| {
                    egui::Grid::new("circ").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                        ui.label(tr("Diametro minimo"));
                        ui.add(crate::i18n::dv(min).speed(0.1).range(0.0..=10000.0).max_decimals(2).suffix(" mm"));
                        ui.end_row();
                        ui.label(tr("Diametro massimo"));
                        ui.add(crate::i18n::dv(max).speed(0.1).range(0.0..=10000.0).max_decimals(2).suffix(" mm"));
                        ui.end_row();
                    });
                });
            }
            EditDialog::Trace { threshold, ignore, .. } => {
                egui::Grid::new("trace").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                    ui.label(tr("Soglia"));
                    ui.add(egui::Slider::new(threshold, 1..=254));
                    ui.end_row();
                    ui.label(tr("Ignora meno di (pixel)"));
                    ui.add(egui::DragValue::new(ignore).range(0..=100000));
                    ui.end_row();
                });
                ui.label(egui::RichText::new(tr("I pixel più scuri della soglia diventano forme vettoriali sul livello attuale.")).small().color(crate::theme::p().text_dim));
            }
            EditDialog::Adjust { image, orig, brightness, contrast, gamma } => {
                let mut changed = false;
                egui::Grid::new("adj").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                    ui.label(tr("Luminosità"));
                    changed |= ui.add(egui::Slider::new(brightness, -100.0..=100.0)).changed();
                    ui.end_row();
                    ui.label(tr("Contrasto"));
                    changed |= ui.add(egui::Slider::new(contrast, -100.0..=100.0)).changed();
                    ui.end_row();
                    ui.label(tr("Gamma"));
                    changed |= ui.add(egui::Slider::new(gamma, 0.1..=4.0)).changed();
                    ui.end_row();
                });
                if changed {
                    let (b, c, g) = (*brightness, *contrast, *gamma);
                    let id = *image;
                    let k = (259.0 * (c * 2.55 + 255.0)) / (255.0 * (259.0 - c * 2.55));
                    let px: Vec<u8> = orig
                        .iter()
                        .map(|&v| {
                            let x = v as f64 + b * 2.55;
                            let x = k * (x - 128.0) + 128.0;
                            let x = (x.clamp(0.0, 255.0) / 255.0).powf(1.0 / g) * 255.0;
                            x.round().clamp(0.0, 255.0) as u8
                        })
                        .collect();
                    if let Some(Geometry::Image { gray, .. }) = app.doc.shapes.iter_mut().find(|s| s.id == id).map(|s| &mut s.geom) {
                        *gray = px;
                    }
                    app.image_changed(id);
                }
            }
        }
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ok = if matches!(d, EditDialog::ConfirmDuplicates { .. }) { tr("Elimina") } else { tr("OK") };
            if ui.add(egui::Button::new(egui::RichText::new(ok).strong()).min_size(vec2(96.0, 26.0))).clicked() {
                action = Some(true);
            }
            if ui.add(egui::Button::new(crate::i18n::cancel()).min_size(vec2(96.0, 26.0))).clicked() {
                action = Some(false);
            }
        });
    });
    if !open && action.is_none() {
        action = Some(false);
    }
    match action {
        Some(true) => {
            match &d {
                EditDialog::CloseTolerance { tol } => app.close_with_tolerance(*tol),
                EditDialog::Optimize { simplify, join, remove_dups } => {
                    app.checkpoint_now();
                    app.optimize_selected(*simplify, *join, *remove_dups);
                }
                EditDialog::ConfirmDuplicates { ids } => {
                    app.checkpoint_now();
                    let n = ids.len();
                    app.doc.shapes.retain(|s| !ids.contains(&s.id));
                    app.selection.retain(|x| !ids.contains(x));
                    app.dirty = true;
                    app.status = trf("Forme duplicate eliminate: {}", &[&n]);
                }
                EditDialog::Circles { by_size, min, max } => app.select_circles(*by_size, *min, *max),
                EditDialog::Trace { image, threshold, ignore } => {
                    app.trace_image(*image, *threshold, *ignore);
                }
                EditDialog::Adjust { .. } => app.status = tr("Immagine regolata").into(),
            }
            keep = false;
        }
        Some(false) => {
            if let EditDialog::Adjust { image, orig, .. } = &d {
                let id = *image;
                if let Some(Geometry::Image { gray, .. }) = app.doc.shapes.iter_mut().find(|s| s.id == id).map(|s| &mut s.geom) {
                    *gray = orig.clone();
                }
                app.undo.pop();
                app.image_changed(id);
            }
            keep = false;
        }
        None => {}
    }
    if keep {
        app.edit_dialog = Some(d);
    }
}

/// "Mostra forme nel livello corrente": the layer's shapes drawn thick and blinking for a moment.
pub fn draw_flash(app: &mut RayEtch, painter: &egui::Painter, xf: &crate::canvas::Xf, now: f64) {
    let Some((layer, until)) = app.flash else { return };
    if now > until {
        app.flash = None;
        return;
    }
    if ((until - now) * 6.0) as i64 % 2 == 0 {
        let col = egui::Color32::from_rgb(255, 60, 200);
        for s in app.doc.shapes.iter().filter(|s| s.layer == layer) {
            for p in s.world_paths() {
                let mut pts: Vec<egui::Pos2> = p.pts.iter().map(|q| xf.to_screen(*q)).collect();
                if p.closed && !pts.is_empty() {
                    pts.push(pts[0]);
                }
                painter.add(egui::Shape::line(pts, egui::Stroke::new(3.0, col)));
            }
        }
    }
    painter.ctx().request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_simplify_and_trace() {
        let a = Polyline { pts: vec![[0.0, 0.0], [10.0, 0.0]], closed: false };
        let b = Polyline { pts: vec![[10.02, 0.0], [10.0, 10.0], [0.0, 0.03]], closed: false };
        let j = join_paths(vec![a, b], AUTO_JOIN_TOL, true);
        assert_eq!(j.len(), 1);
        assert!(j[0].closed);
        let line = Polyline { pts: vec![[0.0, 0.0], [1.0, 0.001], [2.0, 0.0]], closed: false };
        assert_eq!(simplify(&line, 0.01).pts.len(), 2);
        // a 3×2 dark block in a 6×5 picture traces to one loop around it
        let dark = |c: isize, r: isize| (1..4).contains(&c) && (1..3).contains(&r);
        let loops = trace_loops(6, 5, &dark);
        assert_eq!(loops.len(), 1);
        let xs: Vec<f64> = loops[0].iter().map(|p| p.0).collect();
        assert_eq!(xs.iter().cloned().fold(f64::MAX, f64::min), 1.0);
        assert_eq!(xs.iter().cloned().fold(0.0, f64::max), 4.0);
    }

    #[test]
    fn circles_are_recognised() {
        let c = Shape::new(1, Geometry::Ellipse { rx: 5.0, ry: 5.0 }, 0, 0.0, 0.0);
        assert_eq!(circle_diameter(&c), Some(10.0));
        let e = Shape::new(2, Geometry::Ellipse { rx: 5.0, ry: 3.0 }, 0, 0.0, 0.0);
        assert_eq!(circle_diameter(&e), None);
    }
}
