//! "Ritaglia" (the scissors, ⌘K), as the original's ScissorTool / TrimShape: pointing at an
//! outline marks the piece of it between the nearest crossings on either side (crossings with
//! any visible shape, itself included); a click cuts that piece away ("Trim Path",
//! TrimShape::TrimPath 0x1002c1674). The pieces left are joined again where their ends meet,
//! unless Ctrl/Cmd is held; with Shift the cut piece moves to the selected layer instead of
//! going (TrimShape::DoTrim 0x1002c0958, TrimPathAndSaveClipping). A shape in a group isn't cut
//! ("Indicated shape is grouped - Ungroup first to trim", ScissorTool::Update 0x10077cf98).

use crate::model::{Geometry, P2, Polyline, Shape, dist_seg};

/// Pieces shorter than this (mm) are dropped, and ends closer than this are joined.
pub const JOIN_TOL: f64 = 0.01;

/// The piece under the pointer.
#[derive(Clone, Debug)]
pub struct Hit {
    pub shape: u64,
    /// which of the shape's outlines
    pub path: usize,
    /// where the piece starts and ends along that outline (mm from its first point)
    pub from: f64,
    pub to: f64,
    /// the whole outline goes (it crosses nothing)
    pub whole: bool,
    /// the piece, in world mm, for drawing
    pub piece: Vec<P2>,
    pub grouped: bool,
}

fn seg_count(pl: &Polyline) -> usize {
    let n = pl.pts.len();
    if n < 2 {
        0
    } else if pl.closed {
        n
    } else {
        n - 1
    }
}

fn seg(pl: &Polyline, i: usize) -> (P2, P2) {
    (pl.pts[i], pl.pts[(i + 1) % pl.pts.len()])
}

fn len(a: P2, b: P2) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

/// Where two segments cross: the fraction along the first, if they do (parallel ones don't).
fn cross(a: P2, b: P2, c: P2, d: P2) -> Option<f64> {
    let r = [b[0] - a[0], b[1] - a[1]];
    let s = [d[0] - c[0], d[1] - c[1]];
    let den = r[0] * s[1] - r[1] * s[0];
    if den.abs() < 1e-12 {
        return None;
    }
    let q = [c[0] - a[0], c[1] - a[1]];
    let t = (q[0] * s[1] - q[1] * s[0]) / den;
    let u = (q[0] * r[1] - q[1] * r[0]) / den;
    let e = 1e-9;
    ((-e..=1.0 + e).contains(&t) && (-e..=1.0 + e).contains(&u)).then_some(t.clamp(0.0, 1.0))
}

/// Total length and the length up to each point.
fn stations(pl: &Polyline) -> (Vec<f64>, f64) {
    let mut acc = vec![0.0];
    let mut total = 0.0;
    for i in 0..seg_count(pl) {
        let (a, b) = seg(pl, i);
        total += len(a, b);
        acc.push(total);
    }
    (acc, total)
}

/// The point `at` mm along the outline.
fn point_at(pl: &Polyline, acc: &[f64], at: f64) -> P2 {
    for i in 0..seg_count(pl) {
        if at <= acc[i + 1] || i + 1 == seg_count(pl) {
            let (a, b) = seg(pl, i);
            let l = acc[i + 1] - acc[i];
            let t = if l > 1e-12 { ((at - acc[i]) / l).clamp(0.0, 1.0) } else { 0.0 };
            return [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        }
    }
    pl.pts[0]
}

/// The outline from `a` to `b` mm (a < b, both within the outline's length).
fn stretch(pl: &Polyline, acc: &[f64], a: f64, b: f64) -> Vec<P2> {
    let mut out = vec![point_at(pl, acc, a)];
    for i in 1..acc.len() - 1 {
        if acc[i] > a && acc[i] < b {
            out.push(pl.pts[i % pl.pts.len()]);
        }
    }
    out.push(point_at(pl, acc, b));
    out
}

/// The outline from `a` round past its start to `b` (a closed outline; a > b).
fn stretch_wrapped(pl: &Polyline, acc: &[f64], total: f64, a: f64, b: f64) -> Vec<P2> {
    let mut out = stretch(pl, acc, a, total);
    let rest = stretch(pl, acc, 0.0, b);
    out.extend(rest.into_iter().skip(1));
    out
}

/// The piece of outline under `p` (within `tol` mm), between its nearest crossings.
pub fn find(shapes: &[Shape], visible: impl Fn(&Shape) -> bool, p: P2, tol: f64) -> Option<Hit> {
    let worlds: Vec<(usize, Vec<Polyline>)> =
        shapes.iter().enumerate().filter(|(_, s)| visible(s) && !matches!(s.geom, Geometry::Image { .. })).map(|(i, s)| (i, s.world_paths())).collect();
    // the nearest outline under the pointer, the top shape first
    let mut best: Option<(usize, usize, usize, f64, f64)> = None; // (worlds index, path, segment, t, distance)
    for (wi, (_, paths)) in worlds.iter().enumerate().rev() {
        for (pi, pl) in paths.iter().enumerate() {
            for i in 0..seg_count(pl) {
                let (a, b) = seg(pl, i);
                let d = dist_seg(p, a, b);
                if d <= tol && best.is_none_or(|x| d < x.4 - 1e-9) {
                    let l2 = (b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2);
                    let t = if l2 > 0.0 { (((p[0] - a[0]) * (b[0] - a[0]) + (p[1] - a[1]) * (b[1] - a[1])) / l2).clamp(0.0, 1.0) } else { 0.0 };
                    best = Some((wi, pi, i, t, d));
                }
            }
        }
    }
    let (wi, pi, si, st, _) = best?;
    let (shape_idx, paths) = &worlds[wi];
    let pl = &paths[pi];
    let (acc, total) = stations(pl);
    if total < 1e-9 {
        return None;
    }
    let at = acc[si] + st * (acc[si + 1] - acc[si]);
    // every crossing along this outline, in mm from its start
    let nseg = seg_count(pl);
    let mut xs: Vec<f64> = Vec::new();
    for i in 0..nseg {
        let (a, b) = seg(pl, i);
        let l = acc[i + 1] - acc[i];
        for (wj, (_, others)) in worlds.iter().enumerate() {
            for (pj, ol) in others.iter().enumerate() {
                let same = wj == wi && pj == pi;
                for j in 0..seg_count(ol) {
                    // a segment against itself or its neighbours only meets at shared corners
                    if same && (i == j || (i + 1) % nseg == j || (j + 1) % nseg == i) {
                        continue;
                    }
                    let (c, d) = seg(ol, j);
                    if let Some(t) = cross(a, b, c, d) {
                        xs.push(acc[i] + t * l);
                    }
                }
            }
        }
    }
    xs.sort_by(|a, b| a.total_cmp(b));
    xs.dedup_by(|a, b| (*a - *b).abs() < JOIN_TOL);
    if !pl.closed {
        // the outline's own ends bound the first and last pieces
        xs.retain(|x| *x > JOIN_TOL && *x < total - JOIN_TOL);
    }
    let grouped = !shapes[*shape_idx].groups.is_empty();
    let id = shapes[*shape_idx].id;
    let closed_whole = pl.closed && xs.len() < 2;
    if closed_whole || (!pl.closed && xs.is_empty()) {
        let mut piece = pl.pts.clone();
        if pl.closed {
            piece.push(pl.pts[0]);
        }
        return Some(Hit { shape: id, path: pi, from: 0.0, to: total, whole: true, piece, grouped });
    }
    let below = xs.iter().copied().filter(|x| *x <= at).next_back();
    let above = xs.iter().copied().find(|x| *x > at);
    let (from, to, piece) = if pl.closed {
        let from = below.unwrap_or(*xs.last().unwrap());
        let to = above.unwrap_or(xs[0]);
        let piece = if from < to { stretch(pl, &acc, from, to) } else { stretch_wrapped(pl, &acc, total, from, to) };
        (from, to, piece)
    } else {
        let from = below.unwrap_or(0.0);
        let to = above.unwrap_or(total);
        (from, to, stretch(pl, &acc, from, to))
    };
    Some(Hit { shape: id, path: pi, from, to, whole: false, piece, grouped })
}

/// What's left of the outline once the hit piece is cut away (world mm, open pieces).
fn remains(pl: &Polyline, hit: &Hit) -> Vec<Polyline> {
    if hit.whole {
        return Vec::new();
    }
    let (acc, total) = stations(pl);
    let mut out = Vec::new();
    let mut keep = |pts: Vec<P2>| {
        let l: f64 = pts.windows(2).map(|w| len(w[0], w[1])).sum();
        if l > JOIN_TOL {
            out.push(Polyline { pts, closed: false });
        }
    };
    if pl.closed {
        // from the piece's end round to its start
        if hit.to < hit.from {
            keep(stretch(pl, &acc, hit.to, hit.from));
        } else {
            keep(stretch_wrapped(pl, &acc, total, hit.to, hit.from));
        }
    } else {
        keep(stretch(pl, &acc, 0.0, hit.from));
        keep(stretch(pl, &acc, hit.to, total));
    }
    out
}

fn near(a: P2, b: P2) -> bool {
    len(a, b) <= JOIN_TOL
}

/// Join open outlines whose ends meet into longer ones, and close those whose ends meet.
pub fn join(mut paths: Vec<Polyline>) -> Vec<Polyline> {
    loop {
        let mut merged = false;
        'outer: for i in 0..paths.len() {
            if paths[i].closed {
                continue;
            }
            for j in 0..paths.len() {
                if i == j || paths[j].closed {
                    continue;
                }
                let (a, b) = (&paths[i].pts, &paths[j].pts);
                let (a0, a1, b0, b1) = (a[0], *a.last().unwrap(), b[0], *b.last().unwrap());
                let mut bj = paths[j].pts.clone();
                let mut ai = paths[i].pts.clone();
                let joined = if near(a1, b0) {
                    ai.extend(bj.drain(1..));
                    ai
                } else if near(a1, b1) {
                    bj.reverse();
                    ai.extend(bj.drain(1..));
                    ai
                } else if near(a0, b1) {
                    bj.extend(ai.drain(1..));
                    bj
                } else if near(a0, b0) {
                    bj.reverse();
                    bj.extend(ai.drain(1..));
                    bj
                } else {
                    continue;
                };
                let (hi, lo) = (i.max(j), i.min(j));
                paths.remove(hi);
                paths.remove(lo);
                paths.push(Polyline { pts: joined, closed: false });
                merged = true;
                break 'outer;
            }
        }
        if !merged {
            break;
        }
    }
    for p in &mut paths {
        if !p.closed && p.pts.len() > 2 && near(p.pts[0], *p.pts.last().unwrap()) {
            p.pts.pop();
            p.closed = true;
        }
    }
    paths
}

/// Cut the hit piece out of its shape. The shape becomes a plain outline in world mm (a text or
/// bar code stops being editable as such); a shape left with nothing is taken away. `no_join`:
/// Ctrl/Cmd held. Returns the cut piece, for moving it to another layer.
pub fn cut(shapes: &mut Vec<Shape>, hit: &Hit, no_join: bool) -> Option<Polyline> {
    let idx = shapes.iter().position(|s| s.id == hit.shape)?;
    let s = &shapes[idx];
    let paths = s.world_paths();
    let target = paths.get(hit.path)?.clone();
    let mut rest: Vec<Polyline> = paths.iter().enumerate().filter(|(i, _)| *i != hit.path).map(|(_, p)| p.clone()).collect();
    let pieces = remains(&target, hit);
    if no_join {
        rest.extend(pieces);
    } else {
        rest.extend(pieces);
        rest = join(rest);
    }
    let cut_piece = Polyline { pts: hit.piece.clone(), closed: hit.whole && target.closed };
    let cut_piece = if cut_piece.closed { Polyline { pts: target.pts.clone(), closed: true } } else { cut_piece };
    if rest.is_empty() {
        shapes.remove(idx);
    } else {
        let s = &mut shapes[idx];
        s.geom = Geometry::Path { subpaths: rest };
        s.tx = 0.0;
        s.ty = 0.0;
        s.sx = 1.0;
        s.sy = 1.0;
        s.rot = 0.0;
        s.tabs.clear();
        s.barcode = None;
    }
    Some(cut_piece)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(a: P2, b: P2) -> Shape {
        Shape::new(0, Geometry::Path { subpaths: vec![Polyline { pts: vec![a, b], closed: false }] }, 0, 0.0, 0.0)
    }

    #[test]
    fn cuts_between_crossings() {
        // a horizontal line crossed by two verticals at x = 10 and x = 20
        let mut shapes = vec![line([0.0, 0.0], [30.0, 0.0]), line([10.0, -5.0], [10.0, 5.0]), line([20.0, -5.0], [20.0, 5.0])];
        for (i, s) in shapes.iter_mut().enumerate() {
            s.id = i as u64 + 1;
        }
        let hit = find(&shapes, |_| true, [15.0, 0.1], 0.5).unwrap();
        assert_eq!(hit.shape, 1);
        assert!((hit.from - 10.0).abs() < 1e-6 && (hit.to - 20.0).abs() < 1e-6);
        cut(&mut shapes, &hit, false);
        let left = shapes[0].world_paths();
        assert_eq!(left.len(), 2);
        // the end piece runs from the last crossing to the end
        let end = find(&shapes, |_| true, [25.0, 0.0], 0.5).unwrap();
        assert!((end.from - 0.0).abs() < 1e-6 && (end.to - 10.0).abs() < 1e-6, "{end:?}");
    }

    #[test]
    fn square_side_cut_leaves_open_outline() {
        let sq = Polyline { pts: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]], closed: true };
        let mut shapes = vec![Shape::new(1, Geometry::Path { subpaths: vec![sq] }, 0, 0.0, 0.0), line([5.0, -5.0], [5.0, 15.0])];
        shapes[1].id = 2;
        // the square's right part between the two crossings at x = 5 (bottom and top)
        let hit = find(&shapes, |_| true, [10.0, 5.0], 0.5).unwrap();
        assert!(!hit.whole);
        cut(&mut shapes, &hit, false);
        let p = &shapes[0].world_paths()[0];
        assert!(!p.closed);
        assert!(p.pts.iter().all(|q| q[0] <= 5.0 + 1e-6), "{p:?}");
        // nothing crosses an outline on its own: it goes whole
        let mut lone = vec![Shape::new(7, Geometry::Rect { w: 4.0, h: 4.0, radius: 0.0 }, 0, 50.0, 50.0)];
        let hit = find(&lone, |_| true, [52.0, 50.0], 0.5).unwrap();
        assert!(hit.whole);
        cut(&mut lone, &hit, false);
        assert!(lone.is_empty());
    }

    #[test]
    fn join_closes_and_chains() {
        let a = Polyline { pts: vec![[0.0, 0.0], [1.0, 0.0]], closed: false };
        let b = Polyline { pts: vec![[1.0, 0.0], [1.0, 1.0]], closed: false };
        let c = Polyline { pts: vec![[1.0, 1.0], [0.0, 0.0]], closed: false };
        let j = join(vec![a, b, c]);
        assert_eq!(j.len(), 1);
        assert!(j[0].closed);
    }
}
