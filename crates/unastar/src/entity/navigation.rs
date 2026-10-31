//! Bounded, synchronous land navigation over the physics collision geometry.
//!
//! This is a conservative geometric search, not a reproduction of native goal
//! timing. Nodes are block-column centres at actual shape-top heights, plus
//! the exact start and goal. Edges lead to the four side columns and to the
//! four diagonal columns between two sides that are walkable without
//! climbing; they use a lift / horizontal sweep / settle corridor and do not
//! cut corners, cross unsupported gaps, swim, fly, or predict a ballistic
//! jump. Open nodes are expanded by the distance walked plus
//! [`PATH_ESTIMATE_WEIGHT`] times the straight distance left, so paths head
//! straight for the goal, along diagonals where it lies off a block axis, as
//! BDS paths do. A state whose collision the caller waives
//! ([`WalkBlockRole::Collides`], a door the mob opens or breaks) is crossed as
//! if it were empty. Move control must execute the returned step/jump
//! transitions and replan when the world changes.
//!
//! Runtime-state permissions are supplied by the caller, normally as indexed
//! tables compiled from actor navigation policy. This module performs no block
//! name/property lookup of its own. The shared `connected_state` helper uses
//! immutable startup-resolved connection facts and numeric state transitions.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use glam::{DVec3, IVec3};

use super::components::PhysicsBody;
use super::systems::physics::BlockSource;
use crate::entity::mob::constants::PATH_ESTIMATE_WEIGHT;
use crate::registry::collision::{Aabb, block_collision_boxes, connected_state};

const EPSILON: f64 = 1.0e-7;

/// Why a runtime state is being checked. A support state can be dangerous even
/// though its collision box only touches, rather than intersects, the body.
/// `Collides` asks whether the state's collision shape counts at all; `false`
/// makes the search pass through it (no obstruction, no support).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WalkBlockRole {
    Occupied,
    Support,
    Collides,
}

/// Feet positions are exact world positions, not block coordinates. The goal
/// must be supported under its centre; the start only needs a clear body
/// resting on the ground somewhere under its box (see [`find_walk_path`]).
/// A node is in range when the squared distance between its block and the
/// goal's block, in whole blocks, is below `max_range²` truncated; the start
/// is exempt. `max_expansions` bounds the nodes expanded (the search then
/// ends as if the frontier ran out); `node_budget` bounds retained search
/// nodes, including start. Neither is elapsed time. The exact-goal connector
/// does not consume a retained node.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WalkPathRequest {
    pub start: DVec3,
    pub goal: DVec3,
    pub max_range: f64,
    pub max_expansions: usize,
    pub node_budget: usize,
    /// Additional caller restriction; also capped by `PhysicsBody::step_height`.
    pub step_height: f64,
    /// Available vertical jump lift; zero disables jumps. Horizontal gap jumps
    /// are deliberately unsupported. This is a geometric capability, not speed
    /// or a promise that a particular controller's jump impulse can execute it.
    pub jump_height: f64,
    pub max_drop: f64,
    /// On failure, publish the route to the expanded node closest (in a
    /// straight line) to the goal as [`WalkPathOutcome::Partial`], unless
    /// that node is the start.
    pub partial: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WalkPathOutcome {
    Found,
    /// No route in this bounded, conservative column-centre graph. Also used
    /// for invalid/nonfinite inputs or an unsupported/obstructed exact endpoint.
    Unreachable,
    /// The expansion cap ended the search, or a valid unvisited node could
    /// not be retained under the node budget, and the graph searched
    /// contained no route. No partial route is published.
    BudgetExceeded,
    /// No route reached the goal; a requested partial route to the expanded
    /// node closest to the goal was published instead.
    Partial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WalkMovement {
    Walk,
    Step,
    Jump,
    Drop,
}

/// One supported destination. For the edge from the preceding point, a clear
/// corridor was checked by lifting at the preceding feet by `lift_height`,
/// translating horizontally, then settling vertically to `feet`. Thus an
/// ascending edge is NOT a straight-line interpolation through the step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct WalkWaypoint {
    pub feet: DVec3,
    pub movement: WalkMovement,
    pub lift_height: f64,
}

/// Caller-owned output. Includes exact start and goal on success, or the start
/// up to the closest node on a partial result; is empty on every other
/// failure. Clearing or performing another search retains its capacity.
#[derive(Debug, Default)]
pub(crate) struct WalkNavigation {
    pub waypoints: Vec<WalkWaypoint>,
}

impl WalkNavigation {
    pub fn clear(&mut self) {
        self.waypoints.clear();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct NodeKey([u64; 3]);

impl NodeKey {
    fn new(feet: DVec3) -> Self {
        // Canonicalise signed zero so an exact start cannot create a duplicate.
        Self(
            feet.to_array()
                .map(|v| if v == 0.0 { 0 } else { v.to_bits() }),
        )
    }
}

#[derive(Debug, Clone, Copy)]
struct Node {
    point: WalkWaypoint,
    parent: Option<usize>,
    /// Distance walked from the start along the parent chain.
    cost: f64,
    /// Expanded; its cost is final.
    closed: bool,
}

/// An open node in the search frontier: the lowest `estimate` pops first,
/// equal estimates in push order.
#[derive(Debug, Clone, Copy)]
struct Open {
    estimate: f64,
    order: usize,
    node: usize,
}

impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        // `BinaryHeap` pops the greatest: reverse both keys.
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

/// Columns searched around a node's column: its own (so an off-centre exact
/// start joins the lattice without snapping through geometry), the four
/// sides, then the four diagonals.
const NEIGHBOURS: [(i32, i32); 9] = [
    (0, 0),
    (1, 0),
    (0, 1),
    (-1, 0),
    (0, -1),
    (1, 1),
    (-1, 1),
    (-1, -1),
    (1, -1),
];
/// The two sides (indices into the four sides of [`NEIGHBOURS`]) between
/// which each diagonal lies.
const DIAGONAL_SIDES: [(usize, usize); 4] = [(0, 1), (2, 1), (2, 3), (0, 3)];

#[derive(Debug, Clone, Copy)]
struct CachedBlock {
    first: usize,
    count: usize,
    occupied_allowed: bool,
    support_allowed: bool,
}

/// Reused across searches, but not across simultaneous searches. The geometry
/// cache is cleared at search entry so block edits and permission changes take
/// effect immediately. Nodes, the node index, the frontier, native boxes and
/// interval work buffers use retained storage; there is no owned allocation
/// per node/edge.
#[derive(Debug, Default)]
pub(crate) struct NavigationScratch {
    nodes: Vec<Node>,
    index: HashMap<NodeKey, usize>,
    open: BinaryHeap<Open>,
    blocks: HashMap<IVec3, CachedBlock>,
    boxes: Vec<Aabb>,
    local_boxes: Vec<Aabb>,
    heights: Vec<f64>,
    intervals: Vec<(f64, f64)>,
}

/// Search an eight-connected walking graph best first: an open node's
/// estimate is the distance walked to it plus [`PATH_ESTIMATE_WEIGHT`] times
/// its straight distance to the goal; equal estimates expand in discovery
/// order, so results are deterministic. A diagonal is only taken between two
/// sides that are themselves reachable without climbing. The predicate must
/// be a pure decision on a typed runtime ID and role; air must be permitted
/// as `Occupied` for ordinary land movement. Unloaded and unknown states are
/// always forbidden, regardless of the predicate.
///
/// Native shape tops provide partial-height support. Body width/height apply
/// to every swept corridor, including endpoint connectors. Centre-line support
/// must cover the entire edge (at endpoint heights or between them); forbidden
/// support touching the swept footprint also rejects the edge conservatively.
/// The one exception is the start: a mob standing half over a block edge
/// rests on its box, not its centre, and still paths from where it stands
/// (as in BDS 1.26.51), so edges leaving a start whose centre is unsupported
/// skip the centre-line check. A start not resting on the ground at all
/// (falling, jumping, climbing) has no path.
/// A valid geometric route is required even when start and goal share a cell.
/// No unchecked straight-line fallback is returned; a nearest-reachable route
/// is returned only when the request asks for a partial result.
pub(crate) fn find_walk_path(
    blocks: &impl BlockSource,
    body: &PhysicsBody,
    request: WalkPathRequest,
    scratch: &mut NavigationScratch,
    output: &mut WalkNavigation,
    permitted: impl Fn(u32, WalkBlockRole) -> bool,
) -> WalkPathOutcome {
    output.clear();
    scratch.nodes.clear();
    scratch.index.clear();
    scratch.open.clear();
    scratch.blocks.clear();
    scratch.boxes.clear();
    scratch.local_boxes.clear();
    scratch.heights.clear();
    scratch.intervals.clear();
    if !valid_request(body, request) {
        return WalkPathOutcome::Unreachable;
    }
    let mut search = Search {
        blocks,
        body,
        request,
        scratch,
        permitted,
        start_off_centre: false,
    };
    if !search.standable(request.goal) || !search.clear_area(search.body_box(request.start)) {
        return WalkPathOutcome::Unreachable;
    }
    if !search.supported_segment(request.start, request.start) {
        if !search.resting(request.start) {
            return WalkPathOutcome::Unreachable;
        }
        search.start_off_centre = true;
    }
    if request.node_budget == 0 {
        return WalkPathOutcome::BudgetExceeded;
    }
    search.scratch.nodes.reserve(request.node_budget);
    search.scratch.index.reserve(request.node_budget);
    search.scratch.nodes.push(Node {
        point: WalkWaypoint {
            feet: request.start,
            movement: WalkMovement::Walk,
            lift_height: 0.0,
        },
        parent: None,
        cost: 0.0,
        closed: false,
    });
    search.scratch.index.insert(NodeKey::new(request.start), 0);
    search.scratch.open.push(Open {
        estimate: 0.0,
        order: 0,
        node: 0,
    });
    let mut pushed = 1;
    let mut expanded = 0;
    let mut exhausted = false;
    // The expanded node closest to the goal and its distance (partial routes).
    let mut closest = (0, f64::INFINITY);
    while let Some(Open { node: cursor, .. }) = search.scratch.open.pop() {
        if search.scratch.nodes[cursor].closed {
            // A stale entry of a node reached again more cheaply.
            continue;
        }
        if expanded == request.max_expansions {
            exhausted = true;
            break;
        }
        expanded += 1;
        search.scratch.nodes[cursor].closed = true;
        let from = search.scratch.nodes[cursor].point.feet;
        let walked = search.scratch.nodes[cursor].cost;
        let distance = from.distance(request.goal);
        if distance < closest.1 {
            closest = (cursor, distance);
        }
        // An exact-goal connector is an ordinary fully checked edge, not a
        // fallback. Restrict its horizontal reach to a neighbouring column.
        if (from.x - request.goal.x).abs() <= 1.5
            && (from.z - request.goal.z).abs() <= 1.5
            && let Some(goal) = search.edge(from, request.goal)
        {
            publish(&search.scratch.nodes, cursor, output);
            if from != request.goal {
                output.waypoints.push(goal);
            }
            return WalkPathOutcome::Found;
        }
        let column = from.floor().as_ivec3();
        // Whether each side has an edge that does not climb.
        let mut level_sides = [false; 4];
        for (n, (dx, dz)) in NEIGHBOURS.into_iter().enumerate() {
            let side = n.checked_sub(1).filter(|&side| side < 4);
            if let Some((a, b)) = n.checked_sub(5).map(|d| DIAGONAL_SIDES[d])
                && !(level_sides[a] && level_sides[b])
            {
                continue;
            }
            let x = f64::from(column.x + dx) + 0.5;
            let z = f64::from(column.z + dz) + 0.5;
            search.candidate_heights(x, z, from.y);
            for i in 0..search.scratch.heights.len() {
                let to = DVec3::new(x, search.scratch.heights[i], z);
                if !search.in_range(to) {
                    continue;
                }
                let known = search.scratch.index.get(&NodeKey::new(to)).copied();
                let closed = known.is_some_and(|k| search.scratch.nodes[k].closed);
                // A closed side still decides whether its diagonals are open.
                if closed && side.is_none() {
                    continue;
                }
                let Some(point) = search.edge(from, to) else {
                    continue;
                };
                if let Some(side) = side
                    && to.y <= from.y + EPSILON
                {
                    level_sides[side] = true;
                }
                if closed {
                    continue;
                }
                let cost = walked + from.distance(to);
                let node = match known {
                    Some(k) if cost + EPSILON < search.scratch.nodes[k].cost => {
                        let node = &mut search.scratch.nodes[k];
                        node.point = point;
                        node.parent = Some(cursor);
                        node.cost = cost;
                        k
                    }
                    Some(_) => continue,
                    None if search.scratch.nodes.len() == request.node_budget => {
                        exhausted = true;
                        continue;
                    }
                    None => {
                        let k = search.scratch.nodes.len();
                        search.scratch.nodes.push(Node {
                            point,
                            parent: Some(cursor),
                            cost,
                            closed: false,
                        });
                        search.scratch.index.insert(NodeKey::new(to), k);
                        k
                    }
                };
                search.scratch.open.push(Open {
                    estimate: cost + PATH_ESTIMATE_WEIGHT * to.distance(request.goal),
                    order: pushed,
                    node,
                });
                pushed += 1;
            }
        }
    }
    if request.partial && closest.0 != 0 {
        publish(&search.scratch.nodes, closest.0, output);
        WalkPathOutcome::Partial
    } else if exhausted {
        WalkPathOutcome::BudgetExceeded
    } else {
        WalkPathOutcome::Unreachable
    }
}

/// Write the route from the start to `last` into `output`.
fn publish(nodes: &[Node], last: usize, output: &mut WalkNavigation) {
    let mut length = 0;
    let mut parent = Some(last);
    while let Some(index) = parent {
        length += 1;
        parent = nodes[index].parent;
    }
    output.waypoints.reserve(length + 1);
    let mut parent = Some(last);
    while let Some(index) = parent {
        let node = nodes[index];
        output.waypoints.push(node.point);
        parent = node.parent;
    }
    output.waypoints.reverse();
}

fn valid_request(body: &PhysicsBody, r: WalkPathRequest) -> bool {
    if !r.start.is_finite()
        || !r.goal.is_finite()
        || !body.width.is_finite()
        || !body.height.is_finite()
        || body.width <= 0.0
        || body.height <= 0.0
        || ![
            r.max_range,
            r.step_height,
            r.jump_height,
            r.max_drop,
            body.step_height,
        ]
        .into_iter()
        .all(|n| n.is_finite() && n >= 0.0)
    {
        return false;
    }
    // Leave room for the body and the extra native fence/wall layer before
    // converting query bounds to i32 block coordinates.
    let margin = r.max_range + body.width + body.height + 4.0;
    let limit = f64::from(i32::MAX) - margin;
    limit > 0.0 && r.start.abs().max_element() < limit && r.goal.abs().max_element() < limit
}

struct Search<'a, B, P> {
    blocks: &'a B,
    body: &'a PhysicsBody,
    request: WalkPathRequest,
    scratch: &'a mut NavigationScratch,
    permitted: P,
    /// The start rests on its box but not under its centre.
    start_off_centre: bool,
}

impl<B: BlockSource, P: Fn(u32, WalkBlockRole) -> bool> Search<'_, B, P> {
    /// Whether a node at `feet` lies within range of the goal (see
    /// [`WalkPathRequest`]).
    fn in_range(&self, feet: DVec3) -> bool {
        let blocks = feet.floor() - self.request.goal.floor();
        blocks.length_squared() < (self.request.max_range * self.request.max_range).floor()
    }

    fn body_box(&self, feet: DVec3) -> Aabb {
        Aabb::from_feet(feet, self.body.width, self.body.height)
    }

    fn cached_block(&mut self, pos: IVec3) -> CachedBlock {
        if let Some(cached) = self.scratch.blocks.get(&pos) {
            return *cached;
        }
        // Each block is classified as it collides: with its neighbour-derived
        // connections, and a door half as the whole door (the lower half
        // holds the open bit, the upper half the hinge).
        let state = self
            .blocks
            .block_at(pos)
            .filter(|id| self.blocks.registry().get_by_runtime_id(*id).is_some())
            .map(|state| {
                connected_state(self.blocks.registry(), pos, state, |p| {
                    self.blocks.block_at(p)
                })
            });
        self.scratch.local_boxes.clear();
        match state {
            Some(state) if (self.permitted)(state, WalkBlockRole::Collides) => {
                block_collision_boxes(state, &mut self.scratch.local_boxes);
            }
            Some(_) => {}
            None => self
                .scratch
                .local_boxes
                .push(Aabb::new(DVec3::ZERO, DVec3::ONE)),
        }
        let cached = CachedBlock {
            first: self.scratch.boxes.len(),
            count: self.scratch.local_boxes.len(),
            occupied_allowed: state.is_some_and(|id| (self.permitted)(id, WalkBlockRole::Occupied)),
            support_allowed: state.is_some_and(|id| (self.permitted)(id, WalkBlockRole::Support)),
        };
        self.scratch.boxes.extend(
            self.scratch
                .local_boxes
                .iter()
                .map(|shape| shape.offset(pos.as_dvec3())),
        );
        self.scratch.blocks.insert(pos, cached);
        cached
    }

    fn clear_area(&mut self, area: Aabb) -> bool {
        let min = area.min.floor().as_ivec3();
        let max = (area.max.ceil() - DVec3::ONE).as_ivec3();
        // Match physics' extra lower layer for fences/walls extending above
        // their owning voxel. Permissions concern positive voxel overlap only.
        for y in (min.y - 1)..=max.y {
            for x in min.x..=max.x {
                for z in min.z..=max.z {
                    let pos = IVec3::new(x, y, z);
                    let cached = self.cached_block(pos);
                    let voxel = Aabb::new(pos.as_dvec3(), pos.as_dvec3() + DVec3::ONE);
                    if voxel.intersects(&area) && !cached.occupied_allowed {
                        return false;
                    }
                    for i in cached.first..cached.first + cached.count {
                        if self.scratch.boxes[i].intersects(&area) {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    fn standable(&mut self, feet: DVec3) -> bool {
        if !self.clear_area(self.body_box(feet)) {
            return false;
        }
        self.supported_segment(feet, feet)
    }

    /// Whether the body at `feet` rests on an allowed support shape whose
    /// top meets the feet anywhere under its footprint (positive overlap,
    /// like the physics ground contact); forbidden support touching the
    /// footprint rejects it.
    fn resting(&mut self, feet: DVec3) -> bool {
        let half = self.body.width * 0.5;
        let min = feet - DVec3::new(half, 0.0, half);
        let max = feet + DVec3::new(half, 0.0, half);
        let mut rests = false;
        for y in (feet.y.floor() as i32 - 2)..=feet.y.floor() as i32 {
            for x in min.x.floor() as i32..max.x.ceil() as i32 {
                for z in min.z.floor() as i32..max.z.ceil() as i32 {
                    let cached = self.cached_block(IVec3::new(x, y, z));
                    for i in cached.first..cached.first + cached.count {
                        let shape = self.scratch.boxes[i];
                        if (shape.max.y - feet.y).abs() > EPSILON
                            || shape.min.x >= max.x
                            || shape.max.x <= min.x
                            || shape.min.z >= max.z
                            || shape.max.z <= min.z
                        {
                            continue;
                        }
                        if !cached.support_allowed {
                            return false;
                        }
                        rests = true;
                    }
                }
            }
        }
        rests
    }

    fn candidate_heights(&mut self, x: f64, z: f64, from_y: f64) {
        self.scratch.heights.clear();
        let lower = from_y - self.request.max_drop;
        let upper = from_y
            + self
                .request
                .step_height
                .min(self.body.step_height)
                .max(self.request.jump_height);
        for y in (lower.floor() as i32 - 2)..=upper.floor() as i32 {
            let cached = self.cached_block(IVec3::new(x.floor() as i32, y, z.floor() as i32));
            if !cached.support_allowed {
                continue;
            }
            for i in cached.first..cached.first + cached.count {
                let shape = self.scratch.boxes[i];
                if shape.min.x <= x
                    && x <= shape.max.x
                    && shape.min.z <= z
                    && z <= shape.max.z
                    && lower - EPSILON <= shape.max.y
                    && shape.max.y <= upper + EPSILON
                {
                    self.scratch.heights.push(shape.max.y);
                }
            }
        }
        self.scratch.heights.sort_by(f64::total_cmp);
        self.scratch
            .heights
            .dedup_by(|a, b| (*a - *b).abs() <= EPSILON);
    }

    fn edge(&mut self, from: DVec3, to: DVec3) -> Option<WalkWaypoint> {
        let dy = to.y - from.y;
        let step = self.request.step_height.min(self.body.step_height);
        let (movement, lift_height) = if dy > EPSILON {
            if dy <= step + EPSILON {
                (WalkMovement::Step, dy)
            } else if dy <= self.request.jump_height + EPSILON {
                (WalkMovement::Jump, self.request.jump_height)
            } else {
                return None;
            }
        } else if dy < -EPSILON {
            if -dy > self.request.max_drop + EPSILON {
                return None;
            }
            (WalkMovement::Drop, 0.0)
        } else {
            (WalkMovement::Walk, dy.max(0.0))
        };
        let raised = from + DVec3::Y * lift_height;
        let across = DVec3::new(to.x, raised.y, to.z);
        let off_centre_start = self.start_off_centre && from == self.request.start;
        if !self.in_range(to)
            || !self.standable(to)
            || !self.clear_area(self.body_box(from).expand_towards(DVec3::Y * lift_height))
            || !self.clear_area(self.body_box(raised).expand_towards(across - raised))
            || !self.clear_area(self.body_box(across).expand_towards(to - across))
            || !(off_centre_start || self.supported_segment(from, to))
        {
            return None;
        }
        Some(WalkWaypoint {
            feet: to,
            movement,
            lift_height,
        })
    }

    fn supported_segment(&mut self, from: DVec3, to: DVec3) -> bool {
        self.scratch.intervals.clear();
        let half = self.body.width * 0.5;
        let low_y = from.y.min(to.y);
        let high_y = from.y.max(to.y);
        let min = from.min(to) - DVec3::new(half, 0.0, half);
        let max = from.max(to) + DVec3::new(half, 0.0, half);
        for y in (low_y.floor() as i32 - 2)..=high_y.floor() as i32 {
            for x in min.x.floor() as i32..max.x.ceil() as i32 {
                for z in min.z.floor() as i32..max.z.ceil() as i32 {
                    let cached = self.cached_block(IVec3::new(x, y, z));
                    for i in cached.first..cached.first + cached.count {
                        let shape = self.scratch.boxes[i];
                        if shape.max.y < low_y - EPSILON
                            || shape.max.y > high_y + EPSILON
                            || shape.min.x >= max.x
                            || shape.max.x <= min.x
                            || shape.min.z >= max.z
                            || shape.max.z <= min.z
                        {
                            continue;
                        }
                        if !cached.support_allowed {
                            return false;
                        }
                        if let Some(interval) = support_interval(shape, from, to) {
                            self.scratch.intervals.push(interval);
                        }
                    }
                }
            }
        }
        self.scratch
            .intervals
            .sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let mut covered = 0.0;
        for &(begin, end) in &self.scratch.intervals {
            if begin > covered + EPSILON {
                return false;
            }
            covered = covered.max(end);
            if covered >= 1.0 - EPSILON {
                return true;
            }
        }
        false
    }
}

/// Exact parameter interval for the centre line above a shape's top rectangle.
/// Unioning these intervals avoids sample spacing that could skip narrow gaps.
fn support_interval(shape: Aabb, from: DVec3, to: DVec3) -> Option<(f64, f64)> {
    let mut begin: f64 = 0.0;
    let mut end: f64 = 1.0;
    for (start, delta, min, max) in [
        (from.x, to.x - from.x, shape.min.x, shape.max.x),
        (from.z, to.z - from.z, shape.min.z, shape.max.z),
    ] {
        if delta == 0.0 {
            if start < min || start > max {
                return None;
            }
        } else {
            let a = (min - start) / delta;
            let b = (max - start) / delta;
            begin = begin.max(a.min(b));
            end = end.min(a.max(b));
        }
    }
    (begin <= end).then_some((begin, end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::systems::physics::{collect_block_boxes, tests::TestBlocks};
    use crate::registry::BlockRegistry;
    use crate::registry::block::StateValue;

    fn request(start: DVec3, goal: DVec3) -> WalkPathRequest {
        WalkPathRequest {
            start,
            goal,
            max_range: 8.0,
            max_expansions: 1024,
            node_budget: 1024,
            step_height: 0.6,
            jump_height: 0.0,
            max_drop: 1.0,
            partial: false,
        }
    }

    fn find(
        blocks: &impl BlockSource,
        body: &PhysicsBody,
        request: WalkPathRequest,
    ) -> (WalkPathOutcome, WalkNavigation) {
        let mut output = WalkNavigation::default();
        let outcome = find_walk_path(
            blocks,
            body,
            request,
            &mut NavigationScratch::default(),
            &mut output,
            |_, _| true,
        );
        (outcome, output)
    }

    fn assert_clear_corridors(
        blocks: &impl BlockSource,
        body: &PhysicsBody,
        path: &WalkNavigation,
    ) {
        let mut boxes = Vec::new();
        for edge in path.waypoints.windows(2) {
            let from = edge[0].feet;
            let point = edge[1];
            let raised = from + DVec3::Y * point.lift_height;
            let across = DVec3::new(point.feet.x, raised.y, point.feet.z);
            for (start, end) in [(from, raised), (raised, across), (across, point.feet)] {
                boxes.clear();
                collect_block_boxes(
                    blocks,
                    Aabb::from_feet(start, body.width, body.height).expand_towards(end - start),
                    &mut boxes,
                );
                assert!(boxes.is_empty(), "blocked corridor: {start:?} -> {end:?}");
            }
        }
    }

    #[test]
    fn wall_detour_has_clear_edges_and_exact_endpoints() {
        let mut blocks = TestBlocks::floor("minecraft:stone", 0);
        for z in -1..=1 {
            for y in 1..=3 {
                blocks.set(IVec3::new(1, y, z), "minecraft:stone");
            }
        }
        let body = PhysicsBody::living(0.9, 1.4);
        let r = request(DVec3::new(-0.2, 1.0, 0.3), DVec3::new(3.2, 1.0, 0.7));
        let (outcome, output) = find(&blocks, &body, r);
        assert_eq!(outcome, WalkPathOutcome::Found);
        assert_eq!(output.waypoints.first().unwrap().feet, r.start);
        assert_eq!(output.waypoints.last().unwrap().feet, r.goal);
        assert!(
            output
                .waypoints
                .iter()
                .any(|p| p.feet.z < -1.0 || p.feet.z > 2.0)
        );
        assert_clear_corridors(&blocks, &body, &output);
        let (_, repeated) = find(&blocks, &body, r);
        assert_eq!(output.waypoints, repeated.waypoints);
    }

    #[test]
    fn open_ground_paths_walk_the_diagonal() {
        let blocks = TestBlocks::floor("minecraft:stone", 0);
        let body = PhysicsBody::living(0.9, 1.4);
        let r = request(DVec3::new(0.5, 1.0, 0.5), DVec3::new(3.5, 1.0, 3.5));
        let (outcome, output) = find(&blocks, &body, r);
        assert_eq!(outcome, WalkPathOutcome::Found);
        let steps: Vec<DVec3> = output
            .waypoints
            .windows(2)
            .map(|edge| edge[1].feet - edge[0].feet)
            .collect();
        assert_eq!(
            steps,
            vec![DVec3::new(1.0, 0.0, 1.0); 3],
            "{:?}",
            output.waypoints
        );
        assert_clear_corridors(&blocks, &body, &output);
    }

    #[test]
    fn diagonals_need_both_sides_walkable() {
        let mut blocks = TestBlocks::floor("minecraft:stone", 0);
        // Holes on both sides of the diagonal from (0, 0) to (1, 1); the
        // diagonal's own centre line is still supported at the corner.
        blocks.blocks.remove(&IVec3::new(1, 0, 0));
        blocks.blocks.remove(&IVec3::new(0, 0, 1));
        let body = PhysicsBody::living(0.6, 0.7);
        let r = request(DVec3::new(0.5, 1.0, 0.5), DVec3::new(2.5, 1.0, 2.5));
        let (outcome, output) = find(&blocks, &body, r);
        assert_eq!(outcome, WalkPathOutcome::Found);
        assert_ne!(
            output.waypoints[1].feet,
            DVec3::new(1.5, 1.0, 1.5),
            "{:?}",
            output.waypoints
        );
    }

    #[test]
    fn sealed_clear_destination_is_unreachable_without_partial_success() {
        let mut blocks = TestBlocks::floor("minecraft:stone", 0);
        for (x, z) in [(1, 0), (3, 0), (2, -1), (2, 1)] {
            for y in 1..=3 {
                blocks.set(IVec3::new(x, y, z), "minecraft:stone");
            }
        }
        let (outcome, output) = find(
            &blocks,
            &PhysicsBody::living(0.9, 1.4),
            request(DVec3::new(-1.5, 1.0, 0.5), DVec3::new(2.5, 1.0, 0.5)),
        );
        assert_eq!(outcome, WalkPathOutcome::Unreachable);
        assert!(output.waypoints.is_empty());
    }

    #[test]
    fn partial_request_publishes_the_route_to_the_closest_expanded_node() {
        let mut blocks = TestBlocks::floor("minecraft:stone", 0);
        for (x, z) in [(1, 0), (3, 0), (2, -1), (2, 1)] {
            for y in 1..=3 {
                blocks.set(IVec3::new(x, y, z), "minecraft:stone");
            }
        }
        let body = PhysicsBody::living(0.9, 1.4);
        let r = WalkPathRequest {
            partial: true,
            ..request(DVec3::new(-1.5, 1.0, 0.5), DVec3::new(2.5, 1.0, 0.5))
        };
        let (outcome, output) = find(&blocks, &body, r);
        assert_eq!(outcome, WalkPathOutcome::Partial);
        assert_eq!(output.waypoints.first().unwrap().feet, r.start);
        let last = output.waypoints.last().unwrap().feet;
        // The nearest columns outside the sealed cell are one diagonal away.
        assert!(
            (last.distance(r.goal) - 2f64.sqrt()).abs() < EPSILON,
            "{:?}",
            output.waypoints
        );
        assert_clear_corridors(&blocks, &body, &output);

        // Already at the closest node: nothing to publish.
        let (outcome, output) = find(&blocks, &body, WalkPathRequest { start: last, ..r });
        assert_eq!(outcome, WalkPathOutcome::Unreachable);
        assert!(output.waypoints.is_empty());
    }

    #[test]
    fn native_clearance_rejects_low_ceiling_and_wide_body() {
        let mut blocks = TestBlocks::floor("minecraft:stone", 0);
        blocks.set(IVec3::new(2, 2, 0), "minecraft:stone");
        let r = request(DVec3::new(-1.5, 1.0, 0.5), DVec3::new(2.5, 1.0, 0.5));
        assert_eq!(
            find(&blocks, &PhysicsBody::living(0.9, 1.4), r).0,
            WalkPathOutcome::Unreachable
        );
        assert_eq!(
            find(&blocks, &PhysicsBody::living(0.6, 0.7), r).0,
            WalkPathOutcome::Found
        );
        blocks.blocks.remove(&IVec3::new(2, 2, 0));
        for y in 1..=3 {
            blocks.set(IVec3::new(2, y, -1), "minecraft:stone");
            blocks.set(IVec3::new(2, y, 1), "minecraft:stone");
        }
        assert_eq!(
            find(&blocks, &PhysicsBody::living(1.2, 1.4), r).0,
            WalkPathOutcome::Unreachable
        );
        assert_eq!(
            find(&blocks, &PhysicsBody::living(0.9, 1.4), r).0,
            WalkPathOutcome::Found
        );
    }

    #[test]
    fn intermediate_choke_checks_clearance_not_just_endpoints() {
        let mut blocks = TestBlocks::default();
        for x in -4..=4 {
            blocks.set(IVec3::new(x, 0, 0), "minecraft:stone");
        }
        // Only the centre row has ground; neither body can bypass the choke.
        // Both exact endpoints are unobstructed for both body sizes.
        for z in [-1, 1] {
            for y in 1..=3 {
                blocks.set(IVec3::new(1, y, z), "minecraft:stone");
            }
        }
        let r = request(DVec3::new(-2.5, 1.0, 0.5), DVec3::new(3.5, 1.0, 0.5));
        assert_eq!(
            find(&blocks, &PhysicsBody::living(1.2, 0.7), r).0,
            WalkPathOutcome::Unreachable,
        );
        let small = PhysicsBody::living(0.6, 0.7);
        let (outcome, output) = find(&blocks, &small, r);
        assert_eq!(outcome, WalkPathOutcome::Found);
        assert_clear_corridors(&blocks, &small, &output);
        blocks.set(IVec3::new(1, 2, 0), "minecraft:stone");
        assert_eq!(
            find(&blocks, &PhysicsBody::living(0.6, 1.4), r).0,
            WalkPathOutcome::Unreachable,
        );
        assert_eq!(find(&blocks, &small, r).0, WalkPathOutcome::Found);
    }

    struct MissingStrip(TestBlocks);

    impl BlockSource for MissingStrip {
        fn block_at(&self, pos: IVec3) -> Option<u32> {
            if pos.x == 1 {
                None
            } else {
                self.0.block_at(pos)
            }
        }
        fn registry(&self) -> &BlockRegistry {
            self.0.registry()
        }
    }

    #[test]
    fn unloaded_columns_are_not_crossed_or_used_as_support() {
        let blocks = MissingStrip(TestBlocks::floor("minecraft:stone", 0));
        let (outcome, output) = find(
            &blocks,
            &PhysicsBody::living(0.9, 1.4),
            request(DVec3::new(-1.5, 1.0, 0.5), DVec3::new(3.5, 1.0, 0.5)),
        );
        assert_eq!(outcome, WalkPathOutcome::Unreachable);
        assert!(output.waypoints.is_empty());
        let r = request(DVec3::new(1.5, 4.0, 0.5), DVec3::new(1.5, 4.0, 0.5));
        assert_eq!(
            find(&blocks, &PhysicsBody::living(0.6, 0.7), r).0,
            WalkPathOutcome::Unreachable
        );
    }

    fn bottom_slab() -> u32 {
        let slab = BlockRegistry::vanilla()
            .get_by_name("minecraft:oak_slab")
            .unwrap();
        slab.with_state_value(
            slab.default_state_id,
            "minecraft:vertical_half",
            StateValue::Str("bottom"),
        )
        .unwrap()
    }

    #[test]
    fn partial_height_support_uses_native_shape_tops_and_step_capability() {
        let mut blocks = TestBlocks::floor("minecraft:stone", 0);
        blocks.set_state(IVec3::new(1, 1, 0), bottom_slab());
        let body = PhysicsBody::living(0.9, 1.4);
        let r = request(DVec3::new(-0.5, 1.0, 0.5), DVec3::new(1.5, 1.5, 0.5));
        let (outcome, output) = find(&blocks, &body, r);
        assert_eq!(outcome, WalkPathOutcome::Found);
        let last = output.waypoints.last().unwrap();
        assert_eq!(last.feet.y, 1.5);
        assert_eq!(last.movement, WalkMovement::Step);
        assert!((last.lift_height - 0.5).abs() < EPSILON);
        assert_clear_corridors(&blocks, &body, &output);
        let restricted = WalkPathRequest {
            step_height: 0.4,
            ..r
        };
        assert_eq!(
            find(&blocks, &body, restricted).0,
            WalkPathOutcome::Unreachable
        );
    }

    #[test]
    fn jump_and_drop_require_explicit_capabilities_and_clear_corridors() {
        let mut blocks = TestBlocks::floor("minecraft:stone", 0);
        blocks.set(IVec3::new(1, 1, 0), "minecraft:stone");
        let body = PhysicsBody::living(0.6, 0.7);
        let base = request(DVec3::new(0.5, 1.0, 0.5), DVec3::new(1.5, 2.0, 0.5));
        assert_eq!(find(&blocks, &body, base).0, WalkPathOutcome::Unreachable);
        let r = WalkPathRequest {
            jump_height: 1.25,
            ..base
        };
        let (outcome, output) = find(&blocks, &body, r);
        assert_eq!(outcome, WalkPathOutcome::Found);
        assert_eq!(
            output.waypoints.last().unwrap().movement,
            WalkMovement::Jump
        );
        assert_clear_corridors(&blocks, &body, &output);
        let down = WalkPathRequest {
            start: base.goal,
            goal: base.start,
            max_drop: 0.5,
            ..base
        };
        assert_eq!(find(&blocks, &body, down).0, WalkPathOutcome::Unreachable);
        let down = WalkPathRequest {
            max_drop: 1.0,
            ..down
        };
        let (outcome, output) = find(&blocks, &body, down);
        assert_eq!(outcome, WalkPathOutcome::Found);
        assert_eq!(
            output.waypoints.last().unwrap().movement,
            WalkMovement::Drop
        );
        assert_clear_corridors(&blocks, &body, &output);
    }

    #[test]
    fn runtime_policy_rejects_occupied_and_support_states() {
        let mut blocks = TestBlocks::floor("minecraft:stone", 0);
        blocks.set(IVec3::new(2, 1, 0), "minecraft:water");
        let water = blocks.block_at(IVec3::new(2, 1, 0)).unwrap();
        let mut output = WalkNavigation::default();
        let mut scratch = NavigationScratch::default();
        let body = PhysicsBody::living(0.6, 0.7);
        let r = request(DVec3::new(-0.5, 1.0, 0.5), DVec3::new(2.5, 1.0, 0.5));
        assert_eq!(
            find_walk_path(
                &blocks,
                &body,
                r,
                &mut scratch,
                &mut output,
                |id, role| role != WalkBlockRole::Occupied || id != water,
            ),
            WalkPathOutcome::Unreachable
        );
        blocks.blocks.remove(&IVec3::new(2, 1, 0));
        let stone = blocks.block_at(IVec3::new(2, 0, 0)).unwrap();
        assert_eq!(
            find_walk_path(
                &blocks,
                &body,
                r,
                &mut scratch,
                &mut output,
                |id, role| role != WalkBlockRole::Support || id != stone,
            ),
            WalkPathOutcome::Unreachable
        );
    }

    #[test]
    fn bounded_search_distinguishes_budget_exhaustion_and_range_failure() {
        let blocks = TestBlocks::floor("minecraft:stone", 0);
        let body = PhysicsBody::living(0.9, 1.4);
        let r = request(DVec3::new(-2.5, 1.0, 0.5), DVec3::new(3.5, 1.0, 0.5));
        let mut scratch = NavigationScratch::default();
        let mut output = WalkNavigation::default();
        assert_eq!(
            find_walk_path(&blocks, &body, r, &mut scratch, &mut output, |_, _| true,),
            WalkPathOutcome::Found
        );
        assert_eq!(
            find_walk_path(
                &blocks,
                &body,
                WalkPathRequest {
                    node_budget: 1,
                    ..r
                },
                &mut scratch,
                &mut output,
                |_, _| true,
            ),
            WalkPathOutcome::BudgetExceeded
        );
        assert!(output.waypoints.is_empty());
        assert_eq!(
            find_walk_path(
                &blocks,
                &body,
                WalkPathRequest {
                    max_expansions: 2,
                    ..r
                },
                &mut scratch,
                &mut output,
                |_, _| true,
            ),
            WalkPathOutcome::BudgetExceeded
        );
        assert!(output.waypoints.is_empty());
        assert_eq!(
            find_walk_path(
                &blocks,
                &body,
                WalkPathRequest {
                    max_range: 2.0,
                    ..r
                },
                &mut scratch,
                &mut output,
                |_, _| true,
            ),
            WalkPathOutcome::Unreachable
        );
        assert!(output.waypoints.is_empty());
    }

    #[test]
    fn unsupported_gap_is_not_skipped_by_an_exact_goal_connector() {
        let mut blocks = TestBlocks::default();
        blocks.set(IVec3::new(0, 0, 0), "minecraft:stone");
        blocks.set(IVec3::new(2, 0, 0), "minecraft:stone");
        let r = request(DVec3::new(0.9, 1.0, 0.5), DVec3::new(2.1, 1.0, 0.5));
        assert_eq!(
            find(&blocks, &PhysicsBody::living(0.2, 0.7), r).0,
            WalkPathOutcome::Unreachable
        );
    }

    #[test]
    fn scratch_cache_is_invalidated_between_searches_without_losing_capacity() {
        let mut blocks = TestBlocks::floor("minecraft:stone", 0);
        let body = PhysicsBody::living(0.9, 1.4);
        let r = request(DVec3::new(-1.5, 1.0, 0.5), DVec3::new(2.5, 1.0, 0.5));
        let mut scratch = NavigationScratch::default();
        let mut output = WalkNavigation::default();
        assert_eq!(
            find_walk_path(&blocks, &body, r, &mut scratch, &mut output, |_, _| true,),
            WalkPathOutcome::Found
        );
        let node_capacity = scratch.nodes.capacity();
        let shape_capacity = scratch.boxes.capacity();
        let output_capacity = output.waypoints.capacity();
        blocks.set(IVec3::new(2, 1, 0), "minecraft:stone");
        assert_eq!(
            find_walk_path(&blocks, &body, r, &mut scratch, &mut output, |_, _| true,),
            WalkPathOutcome::Unreachable
        );
        assert!(output.waypoints.is_empty());
        assert_eq!(scratch.nodes.capacity(), node_capacity);
        assert!(scratch.boxes.capacity() >= shape_capacity);
        assert_eq!(output.waypoints.capacity(), output_capacity);
    }
}
