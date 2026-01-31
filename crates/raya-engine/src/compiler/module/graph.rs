//! Module dependency graph
//!
//! Tracks dependencies between modules and provides:
//! - Cycle detection
//! - Topological ordering for compilation

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use thiserror::Error;

/// Errors related to module graph operations
#[derive(Debug, Error, Clone)]
pub enum GraphError {
    /// Circular dependency detected
    #[error("Circular dependency detected: {}", format_cycle(.0))]
    CircularDependency(Vec<PathBuf>),

    /// Module not found in graph
    #[error("Module not found in graph: {0}")]
    ModuleNotFound(PathBuf),
}

fn format_cycle(cycle: &[PathBuf]) -> String {
    cycle
        .iter()
        .map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(" -> ")
}

/// A node in the module graph
#[derive(Debug, Clone)]
pub struct ModuleNode {
    /// Absolute path to the module
    pub path: PathBuf,
    /// Modules this module imports (dependencies)
    pub imports: Vec<PathBuf>,
    /// Modules that import this module (dependents)
    pub imported_by: Vec<PathBuf>,
}

impl ModuleNode {
    /// Create a new module node
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            imports: Vec::new(),
            imported_by: Vec::new(),
        }
    }
}

/// Module dependency graph
#[derive(Debug, Default)]
pub struct ModuleGraph {
    /// All modules in the graph
    nodes: HashMap<PathBuf, ModuleNode>,
    /// Entry point modules (modules with no dependents)
    entry_points: HashSet<PathBuf>,
}

impl ModuleGraph {
    /// Create a new empty module graph
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a module to the graph
    pub fn add_module(&mut self, path: PathBuf) {
        if !self.nodes.contains_key(&path) {
            self.nodes.insert(path.clone(), ModuleNode::new(path.clone()));
            self.entry_points.insert(path);
        }
    }

    /// Add a dependency edge (from imports to)
    pub fn add_dependency(&mut self, from: PathBuf, to: PathBuf) {
        // Ensure both modules exist
        self.add_module(from.clone());
        self.add_module(to.clone());

        // Add forward edge (from imports to)
        if let Some(node) = self.nodes.get_mut(&from) {
            if !node.imports.contains(&to) {
                node.imports.push(to.clone());
            }
        }

        // Add backward edge (to is imported by from)
        if let Some(node) = self.nodes.get_mut(&to) {
            if !node.imported_by.contains(&from) {
                node.imported_by.push(from.clone());
            }
        }

        // `to` is not an entry point since it's imported by something
        self.entry_points.remove(&to);
    }

    /// Get a module node by path
    pub fn get(&self, path: &PathBuf) -> Option<&ModuleNode> {
        self.nodes.get(path)
    }

    /// Get all modules in the graph
    pub fn modules(&self) -> impl Iterator<Item = &PathBuf> {
        self.nodes.keys()
    }

    /// Get entry points (modules with no dependents)
    pub fn entry_points(&self) -> &HashSet<PathBuf> {
        &self.entry_points
    }

    /// Get the number of modules in the graph
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Check if the graph is empty
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Detect cycles in the graph
    ///
    /// Returns `Err(GraphError::CircularDependency)` if a cycle is found,
    /// with the cycle path included in the error.
    pub fn detect_cycles(&self) -> Result<(), GraphError> {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        let mut path = Vec::new();

        for start in self.nodes.keys() {
            if !visited.contains(start) {
                if let Some(cycle) = self.dfs_detect_cycle(start, &mut visited, &mut rec_stack, &mut path) {
                    return Err(GraphError::CircularDependency(cycle));
                }
            }
        }

        Ok(())
    }

    /// DFS helper for cycle detection
    fn dfs_detect_cycle(
        &self,
        node: &PathBuf,
        visited: &mut HashSet<PathBuf>,
        rec_stack: &mut HashSet<PathBuf>,
        path: &mut Vec<PathBuf>,
    ) -> Option<Vec<PathBuf>> {
        visited.insert(node.clone());
        rec_stack.insert(node.clone());
        path.push(node.clone());

        if let Some(module) = self.nodes.get(node) {
            for dep in &module.imports {
                if !visited.contains(dep) {
                    if let Some(cycle) = self.dfs_detect_cycle(dep, visited, rec_stack, path) {
                        return Some(cycle);
                    }
                } else if rec_stack.contains(dep) {
                    // Found cycle - extract the cycle path
                    let cycle_start = path.iter().position(|p| p == dep).unwrap();
                    let mut cycle: Vec<PathBuf> = path[cycle_start..].to_vec();
                    cycle.push(dep.clone()); // Add the starting node to close the cycle
                    return Some(cycle);
                }
            }
        }

        path.pop();
        rec_stack.remove(node);
        None
    }

    /// Get topological order of modules (dependencies first)
    ///
    /// Returns modules in an order where each module comes after all its dependencies.
    /// This is the order modules should be compiled in.
    pub fn topological_order(&self) -> Result<Vec<PathBuf>, GraphError> {
        // First check for cycles
        self.detect_cycles()?;

        let mut result = Vec::new();
        let mut in_degree: HashMap<PathBuf, usize> = HashMap::new();

        // Calculate in-degrees
        for (path, node) in &self.nodes {
            in_degree.entry(path.clone()).or_insert(0);
            for dep in &node.imports {
                *in_degree.entry(dep.clone()).or_insert(0) += 0; // Ensure dep exists
            }
        }

        // Count imports (not imported_by) for each node
        for node in self.nodes.values() {
            for dep in &node.imports {
                *in_degree.get_mut(dep).unwrap() += 1;
            }
        }

        // Wait, we need dependencies first, so we should use imported_by count
        // Let me recalculate - we want leaves (modules with no imports) first
        let mut in_degree: HashMap<PathBuf, usize> = HashMap::new();
        for (path, node) in &self.nodes {
            in_degree.insert(path.clone(), node.imports.len());
        }

        // Start with modules that have no imports (leaves)
        let mut queue: VecDeque<PathBuf> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(p, _)| p.clone())
            .collect();

        while let Some(path) = queue.pop_front() {
            result.push(path.clone());

            // For each module that imports this one, decrement its in-degree
            if let Some(node) = self.nodes.get(&path) {
                for dependent in &node.imported_by {
                    if let Some(deg) = in_degree.get_mut(dependent) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(dependent.clone());
                        }
                    }
                }
            }
        }

        Ok(result)
    }

    /// Get modules that the given module depends on (transitively)
    pub fn transitive_dependencies(&self, path: &PathBuf) -> Result<Vec<PathBuf>, GraphError> {
        let mut deps = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        if let Some(node) = self.nodes.get(path) {
            for dep in &node.imports {
                queue.push_back(dep.clone());
            }
        } else {
            return Err(GraphError::ModuleNotFound(path.clone()));
        }

        while let Some(dep) = queue.pop_front() {
            if visited.insert(dep.clone()) {
                deps.push(dep.clone());
                if let Some(node) = self.nodes.get(&dep) {
                    for next_dep in &node.imports {
                        if !visited.contains(next_dep) {
                            queue.push_back(next_dep.clone());
                        }
                    }
                }
            }
        }

        Ok(deps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_module() {
        let mut graph = ModuleGraph::new();
        let path = PathBuf::from("/src/main.raya");

        graph.add_module(path.clone());

        assert_eq!(graph.len(), 1);
        assert!(graph.get(&path).is_some());
        assert!(graph.entry_points().contains(&path));
    }

    #[test]
    fn test_add_dependency() {
        let mut graph = ModuleGraph::new();
        let main = PathBuf::from("/src/main.raya");
        let utils = PathBuf::from("/src/utils.raya");

        graph.add_dependency(main.clone(), utils.clone());

        assert_eq!(graph.len(), 2);

        let main_node = graph.get(&main).unwrap();
        assert!(main_node.imports.contains(&utils));

        let utils_node = graph.get(&utils).unwrap();
        assert!(utils_node.imported_by.contains(&main));

        // main is entry point, utils is not
        assert!(graph.entry_points().contains(&main));
        assert!(!graph.entry_points().contains(&utils));
    }

    #[test]
    fn test_no_cycle() {
        let mut graph = ModuleGraph::new();
        let a = PathBuf::from("/a.raya");
        let b = PathBuf::from("/b.raya");
        let c = PathBuf::from("/c.raya");

        // a -> b -> c (no cycle)
        graph.add_dependency(a.clone(), b.clone());
        graph.add_dependency(b.clone(), c.clone());

        assert!(graph.detect_cycles().is_ok());
    }

    #[test]
    fn test_simple_cycle() {
        let mut graph = ModuleGraph::new();
        let a = PathBuf::from("/a.raya");
        let b = PathBuf::from("/b.raya");

        // a -> b -> a (cycle)
        graph.add_dependency(a.clone(), b.clone());
        graph.add_dependency(b.clone(), a.clone());

        let result = graph.detect_cycles();
        assert!(matches!(result, Err(GraphError::CircularDependency(_))));
    }

    #[test]
    fn test_longer_cycle() {
        let mut graph = ModuleGraph::new();
        let a = PathBuf::from("/a.raya");
        let b = PathBuf::from("/b.raya");
        let c = PathBuf::from("/c.raya");

        // a -> b -> c -> a (cycle)
        graph.add_dependency(a.clone(), b.clone());
        graph.add_dependency(b.clone(), c.clone());
        graph.add_dependency(c.clone(), a.clone());

        let result = graph.detect_cycles();
        assert!(matches!(result, Err(GraphError::CircularDependency(_))));
    }

    #[test]
    fn test_topological_order() {
        let mut graph = ModuleGraph::new();
        let main = PathBuf::from("/main.raya");
        let utils = PathBuf::from("/utils.raya");
        let logger = PathBuf::from("/logger.raya");

        // main -> utils -> logger
        graph.add_dependency(main.clone(), utils.clone());
        graph.add_dependency(utils.clone(), logger.clone());

        let order = graph.topological_order().unwrap();

        // logger should come before utils, utils before main
        let logger_pos = order.iter().position(|p| p == &logger).unwrap();
        let utils_pos = order.iter().position(|p| p == &utils).unwrap();
        let main_pos = order.iter().position(|p| p == &main).unwrap();

        assert!(logger_pos < utils_pos);
        assert!(utils_pos < main_pos);
    }

    #[test]
    fn test_diamond_dependency() {
        let mut graph = ModuleGraph::new();
        let main = PathBuf::from("/main.raya");
        let a = PathBuf::from("/a.raya");
        let b = PathBuf::from("/b.raya");
        let shared = PathBuf::from("/shared.raya");

        // main -> a -> shared
        //      -> b -> shared
        graph.add_dependency(main.clone(), a.clone());
        graph.add_dependency(main.clone(), b.clone());
        graph.add_dependency(a.clone(), shared.clone());
        graph.add_dependency(b.clone(), shared.clone());

        // No cycle
        assert!(graph.detect_cycles().is_ok());

        let order = graph.topological_order().unwrap();

        // shared should come before a and b, which should come before main
        let shared_pos = order.iter().position(|p| p == &shared).unwrap();
        let a_pos = order.iter().position(|p| p == &a).unwrap();
        let b_pos = order.iter().position(|p| p == &b).unwrap();
        let main_pos = order.iter().position(|p| p == &main).unwrap();

        assert!(shared_pos < a_pos);
        assert!(shared_pos < b_pos);
        assert!(a_pos < main_pos);
        assert!(b_pos < main_pos);
    }

    #[test]
    fn test_transitive_dependencies() {
        let mut graph = ModuleGraph::new();
        let main = PathBuf::from("/main.raya");
        let utils = PathBuf::from("/utils.raya");
        let logger = PathBuf::from("/logger.raya");

        graph.add_dependency(main.clone(), utils.clone());
        graph.add_dependency(utils.clone(), logger.clone());

        let deps = graph.transitive_dependencies(&main).unwrap();

        assert!(deps.contains(&utils));
        assert!(deps.contains(&logger));
        assert_eq!(deps.len(), 2);
    }
}
