//! Dependency analysis infrastructure for AOT compilation.
//!
//! This module builds a dependency graph from parsed density functions,
//! enabling deduplication and topological ordering for code generation.

use super::parser::density_function::{DensityFunctionArg, DensityFunctionDef, SplineDef, SplineValue};
use std::collections::{HashMap, HashSet};

/// Unique identifier for a density function node.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct NodeId(pub String);

impl NodeId {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Node in the dependency graph.
#[derive(Debug, Clone)]
pub struct DensityNode {
    pub id: NodeId,
    pub def: DensityFunctionDef,
    pub dependencies: Vec<NodeId>,
    pub is_y_independent: bool,
    pub is_flat_cache: bool,
    /// Whether this node is a Cache2D node (per-column caching).
    pub is_cache_2d: bool,
    /// How many times this node is referenced (for deduplication decisions)
    pub usage_count: usize,
    /// Optional human-readable name from the JSON reference
    pub ref_name: Option<String>,
}

/// Dependency graph of all density functions.
pub struct DependencyGraph {
    pub nodes: HashMap<NodeId, DensityNode>,
    pub roots: HashMap<String, NodeId>, // router field -> root node
}

impl DependencyGraph {
    /// Build graph from parsed JSON.
    pub fn build(
        router_fields: &[(&str, &DensityFunctionArg)],
        refs: &HashMap<String, DensityFunctionArg>,
    ) -> Self {
        let mut builder = GraphBuilder::new(refs);

        for (field_name, arg) in router_fields {
            let node_id = builder.visit(arg);
            builder.roots.insert(field_name.to_string(), node_id);
        }

        builder.finalize()
    }

    /// Get all FlatCache nodes (Y-independent, should be pre-computed).
    #[allow(dead_code)]
    pub fn flat_cache_nodes(&self) -> Vec<&DensityNode> {
        self.nodes.values()
            .filter(|n| n.is_flat_cache)
            .collect()
    }

    /// Get all Cache2D nodes (per-column caching).
    #[allow(dead_code)]
    pub fn cache_2d_nodes(&self) -> Vec<&DensityNode> {
        self.nodes.values()
            .filter(|n| n.is_cache_2d)
            .collect()
    }

    /// Get the number of unique nodes in the graph.
    #[allow(dead_code)]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Get the number of FlatCache nodes.
    #[allow(dead_code)]
    pub fn flat_cache_count(&self) -> usize {
        self.nodes.values().filter(|n| n.is_flat_cache).count()
    }

    /// Topological sort for emission order.
    /// Returns nodes in dependency order (dependencies before dependents).
    pub fn topo_sort(&self) -> Vec<&NodeId> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut in_progress = HashSet::new();

        fn visit<'a>(
            id: &'a NodeId,
            graph: &'a DependencyGraph,
            visited: &mut HashSet<NodeId>,
            in_progress: &mut HashSet<NodeId>,
            result: &mut Vec<&'a NodeId>,
        ) {
            if visited.contains(id) {
                return;
            }
            if in_progress.contains(id) {
                // Cycle detected - shouldn't happen in valid density functions
                panic!("Cycle detected at node: {:?}", id);
            }

            in_progress.insert(id.clone());

            if let Some(node) = graph.nodes.get(id) {
                for dep in &node.dependencies {
                    visit(dep, graph, visited, in_progress, result);
                }
            }

            in_progress.remove(id);
            visited.insert(id.clone());
            result.push(id);
        }

        // Start from all root nodes
        for root_id in self.roots.values() {
            visit(root_id, self, &mut visited, &mut in_progress, &mut result);
        }

        result
    }
}

struct GraphBuilder<'a> {
    refs: &'a HashMap<String, DensityFunctionArg>,
    nodes: HashMap<NodeId, DensityNode>,
    roots: HashMap<String, NodeId>,
    /// Canonical form -> NodeId for deduplication
    canonical_cache: HashMap<String, NodeId>,
    /// Reference name -> NodeId for ref tracking
    ref_cache: HashMap<String, NodeId>,
    counter: usize,
}

impl<'a> GraphBuilder<'a> {
    fn new(refs: &'a HashMap<String, DensityFunctionArg>) -> Self {
        Self {
            refs,
            nodes: HashMap::new(),
            roots: HashMap::new(),
            canonical_cache: HashMap::new(),
            ref_cache: HashMap::new(),
            counter: 0,
        }
    }

    /// Visit an argument, returning its NodeId.
    fn visit(&mut self, arg: &DensityFunctionArg) -> NodeId {
        match arg {
            DensityFunctionArg::Constant(v) => {
                // Use canonical caching for constants too
                let canonical = format!("const:{}", v);
                if let Some(id) = self.canonical_cache.get(&canonical).cloned() {
                    self.increment_usage(&id);
                    return id;
                }
                let id = self.make_node(
                    DensityFunctionDef::Constant { argument: *v },
                    vec![],
                    true,
                    None,
                );
                self.canonical_cache.insert(canonical, id.clone());
                id
            }
            DensityFunctionArg::Reference(name) => {
                // Check ref cache first
                if let Some(id) = self.ref_cache.get(name).cloned() {
                    self.increment_usage(&id);
                    return id;
                }

                // Resolve and visit
                if let Some(resolved) = self.refs.get(name).cloned() {
                    let id = self.visit(&resolved);
                    // Update the node's ref_name if not already set
                    if let Some(node) = self.nodes.get_mut(&id) {
                        if node.ref_name.is_none() {
                            node.ref_name = Some(name.clone());
                        }
                    }
                    self.ref_cache.insert(name.clone(), id.clone());
                    id
                } else {
                    // Built-in reference
                    self.visit_builtin(name)
                }
            }
            DensityFunctionArg::Inline(def) => {
                self.visit_def(def)
            }
        }
    }

    fn visit_def(&mut self, def: &DensityFunctionDef) -> NodeId {
        // Build canonical string for deduplication
        let canonical = self.canonical_string(def);
        if let Some(id) = self.canonical_cache.get(&canonical).cloned() {
            self.increment_usage(&id);
            return id;
        }

        // Visit children and determine properties
        let (deps, y_indep) = self.visit_children(def);
        let is_flat_cache = matches!(def, DensityFunctionDef::FlatCache { .. });
        let is_cache_2d = matches!(def, DensityFunctionDef::Cache2D { .. });

        let node_def = def.clone();
        let id = self.make_node(node_def, deps, y_indep, None);

        if is_flat_cache {
            if let Some(node) = self.nodes.get_mut(&id) {
                node.is_flat_cache = true;
            }
        }

        if is_cache_2d {
            if let Some(node) = self.nodes.get_mut(&id) {
                node.is_cache_2d = true;
            }
        }

        self.canonical_cache.insert(canonical, id.clone());
        id
    }

    /// Visit all children of a def and return (dependencies, is_y_independent).
    fn visit_children(&mut self, def: &DensityFunctionDef) -> (Vec<NodeId>, bool) {
        match def {
            DensityFunctionDef::Constant { .. } => (vec![], true),

            DensityFunctionDef::Add { argument1, argument2 }
            | DensityFunctionDef::Mul { argument1, argument2 }
            | DensityFunctionDef::Min { argument1, argument2 }
            | DensityFunctionDef::Max { argument1, argument2 } => {
                let d1 = self.visit(argument1);
                let d2 = self.visit(argument2);
                let y_indep = self.is_y_independent(&d1) && self.is_y_independent(&d2);
                (vec![d1, d2], y_indep)
            }

            DensityFunctionDef::Clamp { input, .. } => {
                let d = self.visit(input);
                let y_indep = self.is_y_independent(&d);
                (vec![d], y_indep)
            }

            DensityFunctionDef::Abs { argument }
            | DensityFunctionDef::Square { argument }
            | DensityFunctionDef::Cube { argument }
            | DensityFunctionDef::Squeeze { argument }
            | DensityFunctionDef::HalfNegative { argument }
            | DensityFunctionDef::QuarterNegative { argument } => {
                let d = self.visit(argument);
                let y_indep = self.is_y_independent(&d);
                (vec![d], y_indep)
            }

            DensityFunctionDef::YClampedGradient { .. } => {
                // Y-dependent by definition
                (vec![], false)
            }

            DensityFunctionDef::Noise { .. } => {
                // Noise sampling uses Y coordinate
                (vec![], false)
            }

            DensityFunctionDef::ShiftedNoise { shift_x, shift_y, shift_z, .. } => {
                let sx = self.visit(shift_x);
                let sy = self.visit(shift_y);
                let sz = self.visit(shift_z);
                // ShiftedNoise is Y-dependent due to Y coordinate usage
                (vec![sx, sy, sz], false)
            }

            DensityFunctionDef::ShiftA { .. } | DensityFunctionDef::ShiftB { .. } | DensityFunctionDef::Shift { .. } => {
                // Shift functions don't use Y
                (vec![], true)
            }

            DensityFunctionDef::FlatCache { argument } => {
                let inner = self.visit(argument);
                // FlatCache marks Y-independence
                (vec![inner], true)
            }

            DensityFunctionDef::Cache2D { argument } => {
                let inner = self.visit(argument);
                // Cache2D is typically Y-independent (caches by X,Z)
                (vec![inner], true)
            }

            DensityFunctionDef::CacheOnce { argument } => {
                let inner = self.visit(argument);
                let y_indep = self.is_y_independent(&inner);
                (vec![inner], y_indep)
            }

            DensityFunctionDef::Interpolated { argument } => {
                let inner = self.visit(argument);
                // Interpolated functions are Y-dependent
                (vec![inner], false)
            }

            DensityFunctionDef::BlendAlpha {} | DensityFunctionDef::BlendOffset {} => {
                (vec![], true)
            }

            DensityFunctionDef::BlendDensity { argument } => {
                let inner = self.visit(argument);
                let y_indep = self.is_y_independent(&inner);
                (vec![inner], y_indep)
            }

            DensityFunctionDef::RangeChoice { input, when_in_range, when_out_of_range, .. } => {
                let inp = self.visit(input);
                let wir = self.visit(when_in_range);
                let wor = self.visit(when_out_of_range);
                let y_indep = self.is_y_independent(&inp)
                    && self.is_y_independent(&wir)
                    && self.is_y_independent(&wor);
                (vec![inp, wir, wor], y_indep)
            }

            DensityFunctionDef::Spline { spline } => {
                let deps = self.visit_spline(spline);
                // Splines can be Y-dependent if their coordinate is
                let y_indep = deps.iter().all(|d| self.is_y_independent(d));
                (deps, y_indep)
            }

            DensityFunctionDef::WeirdScaledSampler { input, .. } => {
                let inp = self.visit(input);
                // WeirdScaledSampler uses Y for noise sampling
                (vec![inp], false)
            }

            DensityFunctionDef::OldBlendedNoise { .. } => {
                // OldBlendedNoise is Y-dependent
                (vec![], false)
            }

            DensityFunctionDef::EndIslands {} => {
                // EndIslands doesn't use Y (it's based on X,Z distance from 0,0)
                (vec![], true)
            }

            DensityFunctionDef::Invert { argument } => {
                let d = self.visit(argument);
                let y_indep = self.is_y_independent(&d);
                (vec![d], y_indep)
            }

            DensityFunctionDef::FindTopSurface { density, upper_bound, .. } => {
                // FindTopSurface is Y-independent (returns a surface Y level)
                // But we need to track its inner density function as a dependency
                let d = self.visit(density);
                let ub = self.visit(upper_bound);
                // Always Y-independent since it computes a single Y value for (X, Z)
                (vec![d, ub], true)
            }
        }
    }

    /// Visit a spline and collect all its dependencies.
    fn visit_spline(&mut self, spline: &SplineDef) -> Vec<NodeId> {
        let mut deps = vec![self.visit(&spline.coordinate)];

        for point in &spline.points {
            match &point.value {
                SplineValue::Constant(_) => {}
                SplineValue::Nested(nested) => {
                    deps.extend(self.visit_spline(nested));
                }
            }
        }

        deps
    }

    fn make_node(
        &mut self,
        def: DensityFunctionDef,
        deps: Vec<NodeId>,
        y_indep: bool,
        ref_name: Option<String>,
    ) -> NodeId {
        let id = NodeId(format!("n{}", self.counter));
        self.counter += 1;

        self.nodes.insert(id.clone(), DensityNode {
            id: id.clone(),
            def,
            dependencies: deps,
            is_y_independent: y_indep,
            is_flat_cache: false,
            is_cache_2d: false,
            usage_count: 1,
            ref_name,
        });

        id
    }

    fn increment_usage(&mut self, id: &NodeId) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.usage_count += 1;
        }
    }

    fn is_y_independent(&self, id: &NodeId) -> bool {
        self.nodes.get(id).map(|n| n.is_y_independent).unwrap_or(false)
    }

    fn visit_builtin(&mut self, name: &str) -> NodeId {
        // Check if we already processed this builtin
        if let Some(id) = self.ref_cache.get(name).cloned() {
            self.increment_usage(&id);
            return id;
        }

        let id = match name {
            "minecraft:y" => self.make_node(
                DensityFunctionDef::YClampedGradient {
                    from_y: -64, to_y: 320,
                    from_value: -64.0, to_value: 320.0
                },
                vec![],
                false, // Y-dependent!
                Some(name.to_string()),
            ),
            "minecraft:zero" => self.make_node(
                DensityFunctionDef::Constant { argument: 0.0 },
                vec![],
                true,
                Some(name.to_string()),
            ),
            "minecraft:shift_x" => {
                // FlatCache(Cache2D(ShiftA(Offset)))
                let shift_a_id = self.make_node(
                    DensityFunctionDef::ShiftA { argument: "minecraft:offset".to_string() },
                    vec![],
                    true,
                    None,
                );
                let cache_2d_id = self.make_node(
                    DensityFunctionDef::Cache2D { argument: DensityFunctionArg::Constant(0.0) }, // placeholder
                    vec![shift_a_id],
                    true,
                    None,
                );
                let flat_cache_id = self.make_node(
                    DensityFunctionDef::FlatCache { argument: DensityFunctionArg::Constant(0.0) }, // placeholder
                    vec![cache_2d_id],
                    true,
                    Some(name.to_string()),
                );
                if let Some(node) = self.nodes.get_mut(&flat_cache_id) {
                    node.is_flat_cache = true;
                }
                flat_cache_id
            }
            "minecraft:shift_z" => {
                // FlatCache(Cache2D(ShiftB(Offset)))
                let shift_b_id = self.make_node(
                    DensityFunctionDef::ShiftB { argument: "minecraft:offset".to_string() },
                    vec![],
                    true,
                    None,
                );
                let cache_2d_id = self.make_node(
                    DensityFunctionDef::Cache2D { argument: DensityFunctionArg::Constant(0.0) }, // placeholder
                    vec![shift_b_id],
                    true,
                    None,
                );
                let flat_cache_id = self.make_node(
                    DensityFunctionDef::FlatCache { argument: DensityFunctionArg::Constant(0.0) }, // placeholder
                    vec![cache_2d_id],
                    true,
                    Some(name.to_string()),
                );
                if let Some(node) = self.nodes.get_mut(&flat_cache_id) {
                    node.is_flat_cache = true;
                }
                flat_cache_id
            }
            _ => {
                eprintln!("Warning: Unknown builtin reference: {}", name);
                self.make_node(
                    DensityFunctionDef::Constant { argument: 0.0 },
                    vec![],
                    true,
                    Some(name.to_string()),
                )
            }
        };

        self.ref_cache.insert(name.to_string(), id.clone());
        id
    }

    /// Create a canonical string for deduplication.
    fn canonical_string(&self, def: &DensityFunctionDef) -> String {
        // Use Debug format for now - it's not perfect but works for deduplication
        format!("{:?}", def)
    }

    fn finalize(self) -> DependencyGraph {
        DependencyGraph {
            nodes: self.nodes,
            roots: self.roots,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_deduplication() {
        let refs = HashMap::new();
        let arg1 = DensityFunctionArg::Constant(1.0);
        let arg2 = DensityFunctionArg::Constant(1.0);

        let graph = DependencyGraph::build(
            &[("a", &arg1), ("b", &arg2)],
            &refs,
        );

        // Both should point to the same node (deduplication)
        assert_eq!(graph.roots.get("a"), graph.roots.get("b"));
        // Only one node should exist
        assert_eq!(graph.node_count(), 1);
    }

    #[test]
    fn test_topo_sort() {
        let refs = HashMap::new();

        // Create a simple Add(Constant(1), Constant(2))
        let add_def = DensityFunctionDef::Add {
            argument1: DensityFunctionArg::Constant(1.0),
            argument2: DensityFunctionArg::Constant(2.0),
        };
        let arg = DensityFunctionArg::Inline(Box::new(add_def));

        let graph = DependencyGraph::build(&[("root", &arg)], &refs);
        let sorted = graph.topo_sort();

        // Should have 3 nodes: two constants and one add
        assert_eq!(sorted.len(), 3);

        // The Add node should come after both constants
        let add_idx = sorted.iter().position(|id| {
            matches!(graph.nodes.get(*id).unwrap().def, DensityFunctionDef::Add { .. })
        }).unwrap();

        let const_indices: Vec<_> = sorted.iter().enumerate()
            .filter(|(_, id)| matches!(graph.nodes.get(*id).unwrap().def, DensityFunctionDef::Constant { .. }))
            .map(|(i, _)| i)
            .collect();

        for const_idx in const_indices {
            assert!(const_idx < add_idx, "Constants should come before Add");
        }
    }
}
