//! Swim navigation: the path of a mob whose navigation can swim
//! (`can_swim`, as the hunting drowned's `navigation.generic`) while it is
//! in water.
//!
//! The search runs over block cells. A cell names the lowest block of the
//! body, which stands on the cell's centre: the body covers the cells its
//! box overlaps there. A **water node** has water and no collision shape in
//! every body cell; with `can_breach` only its lowest row must be water and
//! the rows above may be air. An amphibious mob (`can_walk`) also has
//! **land nodes**: a body clear of collision shapes (water or air) over a
//! cell with one. From any node the search tries the six axial cells, then
//! the twelve diagonals made of two axial steps when both steps are nodes;
//! a land node one block up a side is a step out of the water when the
//! body fits above the node it leaves.
//!
//! Search is best first on the distance travelled between cell centres plus
//! [`PATH_ESTIMATE_WEIGHT`] times the straight distance left. The goal is
//! the cell holding the target's feet; when it cannot be reached the route
//! to the node closest to it is published. The route is then pulled
//! straight: from each point the mob heads for the farthest node the line
//! between the body's centres there reaches through cells without collision.
//! BDS 1.26.51's drowned heads straight for the feet of a target above it
//! this way (`sweep.py drowned --items hunt`); how it re-aims once it nears
//! the surface is not modelled.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use glam::{DVec3, IVec3};

use super::{WalkMovement, WalkNavigation, WalkPathOutcome, WalkWaypoint};
use crate::entity::mob::constants::PATH_ESTIMATE_WEIGHT;

/// What one block cell is to a swimming body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SwimCell {
    /// No collision shape and no liquid.
    Air,
    /// Water with no collision shape.
    Water,
    /// A collision shape, an unloaded cell or a state the mob must not touch.
    Solid,
}

/// One swim search.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SwimPathRequest {
    pub start: DVec3,
    /// The target's feet.
    pub goal: DVec3,
    pub width: f64,
    pub height: f64,
    /// Nodes farther than this from the start (Chebyshev, blocks) are not
    /// searched.
    pub max_range: i32,
    pub node_budget: usize,
    pub can_breach: bool,
    /// Land nodes are searched too (an amphibious mob that can walk).
    pub can_walk: bool,
}

const AXES: [IVec3; 6] = [
    IVec3::new(0, -1, 0),
    IVec3::new(0, 1, 0),
    IVec3::new(0, 0, -1),
    IVec3::new(0, 0, 1),
    IVec3::new(-1, 0, 0),
    IVec3::new(1, 0, 0),
];

#[derive(Debug, Clone, Copy)]
struct Node {
    cell: IVec3,
    land: bool,
    parent: Option<usize>,
    cost: f64,
    closed: bool,
}

#[derive(Debug, Clone, Copy)]
struct Open {
    estimate: f64,
    order: usize,
    node: usize,
}

impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .estimate
            .total_cmp(&self.estimate)
            .then(other.order.cmp(&self.order))
    }
}

impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Open {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Open {}

/// Buffers reused across swim searches.
#[derive(Debug, Default)]
pub(crate) struct SwimScratch {
    nodes: Vec<Node>,
    index: HashMap<IVec3, usize>,
    open: BinaryHeap<Open>,
    cells: HashMap<IVec3, SwimCell>,
    route: Vec<usize>,
}

/// The body cells of a node at `cell`: (x/z offsets from, to; rows).
fn footprint(width: f64, height: f64) -> (i32, i32, i32) {
    let half = width / 2.0;
    let low = (0.5 - half).floor() as i32;
    let high = (0.5 + half - 1.0e-7).floor() as i32;
    let rows = (height - 1.0e-7).floor().max(0.0) as i32 + 1;
    (low, high, rows)
}

struct Search<'a, F> {
    request: SwimPathRequest,
    cells: &'a mut HashMap<IVec3, SwimCell>,
    classify: F,
    footprint: (i32, i32, i32),
}

impl<F: Fn(IVec3) -> SwimCell> Search<'_, F> {
    fn cell(&mut self, pos: IVec3) -> SwimCell {
        *self
            .cells
            .entry(pos)
            .or_insert_with(|| (self.classify)(pos))
    }

    /// Every body cell at `cell` passes `test(row, kind)`.
    fn body(&mut self, cell: IVec3, test: impl Fn(i32, SwimCell) -> bool) -> bool {
        let (low, high, rows) = self.footprint;
        for row in 0..rows {
            for dx in low..=high {
                for dz in low..=high {
                    let kind = self.cell(cell + IVec3::new(dx, row, dz));
                    if !test(row, kind) {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn water_node(&mut self, cell: IVec3) -> bool {
        let breach = self.request.can_breach;
        self.body(cell, |row, kind| {
            kind == SwimCell::Water || (breach && row > 0 && kind == SwimCell::Air)
        })
    }

    fn land_node(&mut self, cell: IVec3) -> bool {
        if !self.request.can_walk || self.cell(cell - IVec3::Y) != SwimCell::Solid {
            return false;
        }
        self.body(cell, |_, kind| kind != SwimCell::Solid)
    }

    /// The node kind at `cell` (`Some(true)` land, `Some(false)` water).
    fn node(&mut self, cell: IVec3) -> Option<bool> {
        if self.water_node(cell) {
            Some(false)
        } else if self.land_node(cell) {
            Some(true)
        } else {
            None
        }
    }

    fn neighbours(&mut self, from: IVec3, out: &mut Vec<(IVec3, bool)>) {
        let mut axial = [None; 6];
        for (k, axis) in AXES.iter().enumerate() {
            axial[k] = self.node(from + *axis);
            if let Some(land) = axial[k] {
                out.push((from + *axis, land));
            }
        }
        for a in 0..AXES.len() {
            for b in a + 1..AXES.len() {
                // Opposite directions do not make a diagonal.
                if AXES[a] + AXES[b] == IVec3::ZERO || axial[a].is_none() || axial[b].is_none() {
                    continue;
                }
                let cell = from + AXES[a] + AXES[b];
                if let Some(land) = self.node(cell) {
                    out.push((cell, land));
                }
            }
        }
        // Out of the water: a land node one block up a side, when the body
        // fits one block above the node it leaves.
        let (_, _, rows) = self.footprint;
        if self.request.can_walk && self.cell(from + IVec3::Y * rows) != SwimCell::Solid {
            for axis in &AXES[2..] {
                let cell = from + *axis + IVec3::Y;
                if self.land_node(cell) {
                    out.push((cell, true));
                }
            }
        }
    }

    /// Whether the line between the body centres at `from` and `to`
    /// crosses only cells without collision.
    fn clear_line(&mut self, from: DVec3, to: DVec3) -> bool {
        let lift = DVec3::Y * (self.request.height / 2.0);
        let (a, b) = (from + lift, to + lift);
        let steps = (a.distance(b) / LINE_STEP).ceil().max(1.0) as usize;
        (0..=steps).all(|k| {
            let point = a.lerp(b, k as f64 / steps as f64);
            self.cell(point.floor().as_ivec3()) != SwimCell::Solid
        })
    }
}

/// Sampling step of the straight-line check, in blocks.
const LINE_STEP: f64 = 0.1;

fn centre(cell: IVec3) -> DVec3 {
    DVec3::new(
        f64::from(cell.x) + 0.5,
        f64::from(cell.y),
        f64::from(cell.z) + 0.5,
    )
}

fn estimate(cell: IVec3, goal: IVec3) -> f64 {
    PATH_ESTIMATE_WEIGHT * cell.as_dvec3().distance(goal.as_dvec3())
}

/// Search a swim path (module docs) into `output`: the start, then the
/// pulled waypoints. `classify` says what a block cell is to the body.
pub(crate) fn find_swim_path(
    request: SwimPathRequest,
    scratch: &mut SwimScratch,
    output: &mut WalkNavigation,
    classify: impl Fn(IVec3) -> SwimCell,
) -> WalkPathOutcome {
    output.clear();
    scratch.nodes.clear();
    scratch.index.clear();
    scratch.open.clear();
    scratch.cells.clear();
    if !(request.start.is_finite() && request.goal.is_finite())
        || request.width <= 0.0
        || request.height <= 0.0
    {
        return WalkPathOutcome::Unreachable;
    }
    let mut search = Search {
        request,
        cells: &mut scratch.cells,
        classify,
        footprint: footprint(request.width, request.height),
    };
    let start = request.start.floor().as_ivec3();
    let goal = request.goal.floor().as_ivec3();
    scratch.nodes.push(Node {
        cell: start,
        land: false,
        parent: None,
        cost: 0.0,
        closed: false,
    });
    scratch.index.insert(start, 0);
    scratch.open.push(Open {
        estimate: estimate(start, goal),
        order: 0,
        node: 0,
    });
    let mut order = 0;
    let mut best = (start.as_dvec3().distance(goal.as_dvec3()), 0);
    let mut reached = None;
    let mut budget_hit = false;
    let mut neighbours = Vec::new();
    while let Some(open) = scratch.open.pop() {
        let node = scratch.nodes[open.node];
        if node.closed {
            continue;
        }
        if node.cell == goal {
            reached = Some(open.node);
            break;
        }
        scratch.nodes[open.node].closed = true;
        let left = node.cell.as_dvec3().distance(goal.as_dvec3());
        if left < best.0 {
            best = (left, open.node);
        }
        neighbours.clear();
        search.neighbours(node.cell, &mut neighbours);
        for &(cell, land) in &neighbours {
            let offset = (cell - start).abs();
            if offset.max_element() > request.max_range {
                continue;
            }
            let cost = node.cost + cell.as_dvec3().distance(node.cell.as_dvec3());
            let index = match scratch.index.get(&cell) {
                Some(&index) => {
                    let known = &mut scratch.nodes[index];
                    if known.closed || cost >= known.cost {
                        continue;
                    }
                    known.cost = cost;
                    known.parent = Some(open.node);
                    known.land = land;
                    index
                }
                None => {
                    if scratch.nodes.len() >= request.node_budget {
                        budget_hit = true;
                        continue;
                    }
                    scratch.nodes.push(Node {
                        cell,
                        land,
                        parent: Some(open.node),
                        cost,
                        closed: false,
                    });
                    scratch.index.insert(cell, scratch.nodes.len() - 1);
                    scratch.nodes.len() - 1
                }
            };
            order += 1;
            scratch.open.push(Open {
                estimate: cost + estimate(cell, goal),
                order,
                node: index,
            });
        }
    }
    let end = match reached {
        Some(end) => end,
        None if best.1 != 0 => best.1,
        None if budget_hit => return WalkPathOutcome::BudgetExceeded,
        None => return WalkPathOutcome::Unreachable,
    };
    scratch.route.clear();
    let mut at = Some(end);
    while let Some(node) = at {
        scratch.route.push(node);
        at = scratch.nodes[node].parent;
    }
    scratch.route.reverse();
    // Pull the route straight from the start.
    output.waypoints.push(WalkWaypoint {
        feet: request.start,
        movement: WalkMovement::Swim,
        lift_height: 0.0,
    });
    let mut from = request.start;
    let mut k = 1;
    while k < scratch.route.len() {
        let mut far = k;
        for j in (k..scratch.route.len()).rev() {
            if search.clear_line(from, centre(scratch.nodes[scratch.route[j]].cell)) {
                far = j;
                break;
            }
        }
        let node = scratch.nodes[scratch.route[far]];
        from = centre(node.cell);
        output.waypoints.push(WalkWaypoint {
            feet: from,
            movement: if node.land {
                WalkMovement::Walk
            } else {
                WalkMovement::Swim
            },
            lift_height: 0.0,
        });
        k = far + 1;
    }
    if reached.is_some() {
        WalkPathOutcome::Found
    } else {
        WalkPathOutcome::Unreachable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pool of water x 0..16, y 0..6, z -4..4 with a floor below y 0 and a
    /// pillar at x 2 up to y 5.
    fn pool(cell: IVec3) -> SwimCell {
        if cell.y < 0 || cell.x < 0 || cell.x > 15 || cell.z.abs() > 4 {
            return SwimCell::Solid;
        }
        if cell.x == 2 && cell.z == 0 && cell.y < 6 {
            return SwimCell::Solid;
        }
        if cell.y < 7 {
            SwimCell::Water
        } else {
            SwimCell::Air
        }
    }

    fn request(start: DVec3, goal: DVec3) -> SwimPathRequest {
        SwimPathRequest {
            start,
            goal,
            width: 0.6,
            height: 1.9,
            max_range: 32,
            node_budget: 4096,
            can_breach: false,
            can_walk: true,
        }
    }

    #[test]
    fn open_water_is_crossed_in_one_straight_leg() {
        let mut scratch = SwimScratch::default();
        let mut out = WalkNavigation::default();
        let outcome = find_swim_path(
            request(DVec3::new(12.5, 0.0, 0.5), DVec3::new(6.5, 4.0, 0.5)),
            &mut scratch,
            &mut out,
            pool,
        );
        assert_eq!(outcome, WalkPathOutcome::Found);
        let feet: Vec<DVec3> = out.waypoints.iter().map(|w| w.feet).collect();
        assert_eq!(
            feet,
            [DVec3::new(12.5, 0.0, 0.5), DVec3::new(6.5, 4.0, 0.5)]
        );
    }

    #[test]
    fn the_top_of_a_pillar_is_reached_by_stepping_out_of_the_water() {
        let mut scratch = SwimScratch::default();
        let mut out = WalkNavigation::default();
        let outcome = find_swim_path(
            request(DVec3::new(12.5, 0.0, 0.5), DVec3::new(2.5, 6.0, 0.5)),
            &mut scratch,
            &mut out,
            pool,
        );
        assert_eq!(outcome, WalkPathOutcome::Found);
        let last = out.waypoints.last().unwrap();
        assert_eq!(last.feet, DVec3::new(2.5, 6.0, 0.5));
        assert_eq!(last.movement, WalkMovement::Walk);
    }

    #[test]
    fn a_body_never_breaches_without_can_breach() {
        let mut scratch = SwimScratch::default();
        let mut out = WalkNavigation::default();
        // The surface row (y 6) cannot hold a 1.9 block body.
        find_swim_path(
            request(DVec3::new(12.5, 0.0, 0.5), DVec3::new(8.5, 7.0, 0.5)),
            &mut scratch,
            &mut out,
            pool,
        );
        assert!(out.waypoints.iter().all(|w| w.feet.y <= 5.0), "{out:?}");
    }
}
