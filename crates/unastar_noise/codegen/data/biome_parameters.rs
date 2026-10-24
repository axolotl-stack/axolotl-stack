//! Build-time multi-noise biome search: the overworld parameter points
//! (`java_data/biome_parameters_overworld.json`) and the R-tree Java searches them with.
//!
//! The tree is built exactly the way Java builds it (6 children per node, split on the
//! parameter whose buckets have the smallest total extent, stable sorts on parameter
//! centres) because the search order decides which leaf wins a tie. It is emitted as flat
//! static arrays (`emitter/biome_source.rs`).

use serde::Deserialize;
use std::path::Path;

/// Parameters per point: temperature, humidity, continentalness, erosion, depth,
/// weirdness, offset.
pub const DIMS: usize = 7;

/// Children per R-tree node.
const CHILDREN_PER_NODE: usize = 6;

/// A quantized parameter range `[min, max]`.
pub type Span = (i64, i64);

#[derive(Deserialize)]
struct PointsFile {
    biomes: Vec<String>,
    points: Vec<Vec<String>>,
}

/// One parameter point and the biome it selects.
#[derive(Clone, Debug)]
pub struct Point {
    pub space: [Span; DIMS],
    /// Index into [`BiomeParameters::biomes`].
    pub biome: usize,
}

/// The overworld parameter list.
pub struct BiomeParameters {
    /// Namespaced biome names, in first-use order.
    pub biomes: Vec<String>,
    /// Points in report order.
    pub points: Vec<Point>,
}

/// Java's coordinate quantization: `(long)(value * 10000.0F)` in `float` arithmetic.
pub fn quantize(value: f32) -> i64 {
    (value * 10000.0f32) as i64
}

fn parse_f32(text: &str) -> Result<f32, String> {
    text.parse::<f32>()
        .map_err(|e| format!("bad biome parameter {text}: {e}"))
}

impl BiomeParameters {
    pub fn load(java_data: &Path) -> Result<Self, String> {
        let path = java_data.join("biome_parameters_overworld.json");
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let file: PointsFile =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut points = Vec::with_capacity(file.points.len());
        for row in &file.points {
            if row.len() != 1 + 2 * 6 + 1 {
                return Err(format!("biome parameter row has {} columns", row.len()));
            }
            let biome: usize = row[0]
                .parse()
                .map_err(|e| format!("bad biome index {}: {e}", row[0]))?;
            if biome >= file.biomes.len() {
                return Err(format!("biome index {biome} out of range"));
            }
            let mut space = [(0, 0); DIMS];
            for (dim, slot) in space.iter_mut().take(6).enumerate() {
                *slot = (
                    quantize(parse_f32(&row[1 + dim * 2])?),
                    quantize(parse_f32(&row[2 + dim * 2])?),
                );
            }
            let offset = quantize(parse_f32(&row[13])?);
            space[6] = (offset, offset);
            points.push(Point { space, biome });
        }
        Ok(Self {
            biomes: file.biomes,
            points,
        })
    }
}

/// R-tree node during construction.
#[derive(Clone, Debug)]
pub enum Node {
    Leaf(Point),
    Sub {
        space: [Span; DIMS],
        children: Vec<Node>,
    },
}

impl Node {
    pub fn space(&self) -> &[Span; DIMS] {
        match self {
            Node::Leaf(point) => &point.space,
            Node::Sub { space, .. } => space,
        }
    }

    fn sub(children: Vec<Node>) -> Node {
        let mut space = *children[0].space();
        for child in &children[1..] {
            for (dim, span) in child.space().iter().enumerate() {
                space[dim].0 = space[dim].0.min(span.0);
                space[dim].1 = space[dim].1.max(span.1);
            }
        }
        Node::Sub { space, children }
    }
}

fn center(span: Span) -> i64 {
    // Java: (min + max) / 2L, truncating.
    (span.0 + span.1) / 2
}

fn sort_by_dim(nodes: &mut [Node], dim: usize, absolute: bool) {
    let key = |node: &Node, d: usize| {
        let c = center(node.space()[d]);
        if absolute { c.abs() } else { c }
    };
    nodes.sort_by(|a, b| {
        for k in 0..DIMS {
            let d = (dim + k) % DIMS;
            let order = key(a, d).cmp(&key(b, d));
            if order != std::cmp::Ordering::Equal {
                return order;
            }
        }
        std::cmp::Ordering::Equal
    });
}

fn bucketize(nodes: Vec<Node>) -> Vec<Node> {
    let size = (CHILDREN_PER_NODE as f64)
        .powf(((nodes.len() as f64 - 0.01).ln() / (CHILDREN_PER_NODE as f64).ln()).floor())
        as usize;
    let mut buckets = Vec::new();
    let mut current = Vec::new();
    for node in nodes {
        current.push(node);
        if current.len() >= size {
            buckets.push(Node::sub(std::mem::take(&mut current)));
        }
    }
    if !current.is_empty() {
        buckets.push(Node::sub(current));
    }
    buckets
}

fn cost(space: &[Span; DIMS]) -> i64 {
    space.iter().map(|(min, max)| (max - min).abs()).sum()
}

/// Build the R-tree the way Java does.
pub fn build(mut nodes: Vec<Node>) -> Node {
    assert!(!nodes.is_empty(), "biome R-tree needs at least one point");
    if nodes.len() == 1 {
        return nodes.pop().unwrap();
    }
    if nodes.len() <= CHILDREN_PER_NODE {
        nodes.sort_by_key(|node| {
            node.space()
                .iter()
                .map(|&span| center(span).abs())
                .sum::<i64>()
        });
        return Node::sub(nodes);
    }
    let mut best_cost = i64::MAX;
    let mut best: Option<(usize, Vec<Node>)> = None;
    for dim in 0..DIMS {
        sort_by_dim(&mut nodes, dim, false);
        let buckets = bucketize(nodes.clone());
        let total: i64 = buckets.iter().map(|b| cost(b.space())).sum();
        if best_cost > total {
            best_cost = total;
            best = Some((dim, buckets));
        }
    }
    let (dim, mut buckets) = best.expect("at least one split dimension");
    sort_by_dim(&mut buckets, dim, true);
    let children = buckets
        .into_iter()
        .map(|bucket| match bucket {
            Node::Sub { children, .. } => build(children),
            leaf => leaf,
        })
        .collect();
    Node::sub(children)
}
