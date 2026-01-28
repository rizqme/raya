//! Dependency graph for module resolution
//!
//! Tracks module dependencies and detects circular dependencies.

use std::collections::{HashMap, HashSet, VecDeque};
use thiserror::Error;

/// Errors that can occur during dependency graph operations
#[derive(Debug, Error)]
pub enum GraphError {
    /// Circular dependency detected
    #[error("Circular dependency detected: {0}")]
    CircularDependency(String),

    /// Module not found in graph
    #[error("Module not found: {0}")]
    ModuleNotFound(String),
}

/// Dependency graph for module resolution
///
/// Tracks which modules depend on which other modules, and provides
/// topological sorting and circular dependency detection.
#[derive(Debug, Clone)]
pub struct DependencyGraph {
    /// Adjacency list: module -> list of modules it depends on
    edges: HashMap<String, Vec<String>>,
}

impl DependencyGraph {
    /// Create a new empty dependency graph
    pub fn new() -> Self {
        Self {
            edges: HashMap::new(),
        }
    }

    /// Add a module to the graph (without dependencies)
    ///
    /// # Arguments
    /// * `module` - The module name
    pub fn add_module(&mut self, module: String) {
        self.edges.entry(module).or_insert_with(Vec::new);
    }

    /// Add a dependency edge to the graph
    ///
    /// # Arguments
    /// * `module` - The module that has the dependency
    /// * `depends_on` - The module that is depended upon
    ///
    /// # Example
    /// ```
    /// # use raya_core::module::DependencyGraph;
    /// let mut graph = DependencyGraph::new();
    /// graph.add_dependency("main".to_string(), "utils".to_string());
    /// graph.add_dependency("main".to_string(), "config".to_string());
    /// graph.add_dependency("utils".to_string(), "helpers".to_string());
    /// ```
    pub fn add_dependency(&mut self, module: String, depends_on: String) {
        self.edges
            .entry(module)
            .or_insert_with(Vec::new)
            .push(depends_on.clone());

        // Ensure the depended-on module exists in the graph
        self.edges.entry(depends_on).or_insert_with(Vec::new);
    }

    /// Detect if there are any cycles in the dependency graph
    ///
    /// # Returns
    /// * `Some(Vec<String>)` - A cycle path if one exists
    /// * `None` - No cycles detected
    ///
    /// # Example
    /// ```
    /// # use raya_core::module::DependencyGraph;
    /// let mut graph = DependencyGraph::new();
    /// graph.add_dependency("a".to_string(), "b".to_string());
    /// graph.add_dependency("b".to_string(), "c".to_string());
    /// graph.add_dependency("c".to_string(), "a".to_string()); // Creates cycle
    ///
    /// let cycle = graph.detect_cycle();
    /// assert!(cycle.is_some());
    /// ```
    pub fn detect_cycle(&self) -> Option<Vec<String>> {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        let mut path = Vec::new();

        for module in self.edges.keys() {
            if !visited.contains(module) {
                if let Some(cycle) = self.dfs_cycle(module, &mut visited, &mut rec_stack, &mut path)
                {
                    return Some(cycle);
                }
            }
        }

        None
    }

    /// DFS helper for cycle detection
    fn dfs_cycle(
        &self,
        node: &str,
        visited: &mut HashSet<String>,
        rec_stack: &mut HashSet<String>,
        path: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        visited.insert(node.to_string());
        rec_stack.insert(node.to_string());
        path.push(node.to_string());

        if let Some(neighbors) = self.edges.get(node) {
            for neighbor in neighbors {
                if !visited.contains(neighbor) {
                    if let Some(cycle) = self.dfs_cycle(neighbor, visited, rec_stack, path) {
                        return Some(cycle);
                    }
                } else if rec_stack.contains(neighbor) {
                    // Found a cycle - extract the cycle path
                    let cycle_start = path.iter().position(|m| m == neighbor).unwrap();
                    let mut cycle = path[cycle_start..].to_vec();
                    cycle.push(neighbor.to_string()); // Add the node that completes the cycle
                    return Some(cycle);
                }
            }
        }

        rec_stack.remove(node);
        path.pop();
        None
    }

    /// Perform a topological sort of the dependency graph
    ///
    /// Returns modules in dependency order (dependencies before dependents).
    ///
    /// # Returns
    /// * `Ok(Vec<String>)` - Modules in topological order
    /// * `Err(GraphError)` - Circular dependency detected
    ///
    /// # Example
    /// ```
    /// # use raya_core::module::DependencyGraph;
    /// let mut graph = DependencyGraph::new();
    /// graph.add_dependency("main".to_string(), "utils".to_string());
    /// graph.add_dependency("utils".to_string(), "helpers".to_string());
    ///
    /// let sorted = graph.topological_sort().unwrap();
    /// // helpers should come before utils, and utils before main
    /// let helpers_idx = sorted.iter().position(|m| m == "helpers").unwrap();
    /// let utils_idx = sorted.iter().position(|m| m == "utils").unwrap();
    /// let main_idx = sorted.iter().position(|m| m == "main").unwrap();
    /// assert!(helpers_idx < utils_idx);
    /// assert!(utils_idx < main_idx);
    /// ```
    pub fn topological_sort(&self) -> Result<Vec<String>, GraphError> {
        // Check for cycles first
        if let Some(cycle) = self.detect_cycle() {
            return Err(GraphError::CircularDependency(cycle.join(" -> ")));
        }

        // Kahn's algorithm for topological sort
        // We need to invert the graph: edges are "module -> depends_on"
        // but we want to process "depends_on -> module" for topological sort

        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut reverse_edges: HashMap<String, Vec<String>> = HashMap::new();

        // Initialize all nodes with 0 in-degree
        for module in self.edges.keys() {
            in_degree.entry(module.clone()).or_insert(0);
            reverse_edges.entry(module.clone()).or_insert_with(Vec::new);
        }

        // Build reverse edges and calculate in-degrees
        // If A depends on B (A -> B), then in reverse: B -> A
        for (module, deps) in &self.edges {
            for dep in deps {
                // module depends on dep, so dep has an outgoing edge to module in reverse
                reverse_edges
                    .entry(dep.clone())
                    .or_insert_with(Vec::new)
                    .push(module.clone());
                // module has an incoming edge in the original graph
                *in_degree.entry(module.clone()).or_insert(0) += 1;
            }
        }

        // Queue of nodes with no incoming edges (no dependencies)
        let mut queue: VecDeque<String> = in_degree
            .iter()
            .filter(|(_, &degree)| degree == 0)
            .map(|(module, _)| module.clone())
            .collect();

        let mut result = Vec::new();

        while let Some(module) = queue.pop_front() {
            result.push(module.clone());

            // For each module that depends on this one (in reverse graph)
            if let Some(dependents) = reverse_edges.get(&module) {
                for dependent in dependents {
                    // Decrease in-degree
                    if let Some(degree) = in_degree.get_mut(dependent) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push_back(dependent.clone());
                        }
                    }
                }
            }
        }

        Ok(result)
    }

    /// Get all modules in the graph
    pub fn modules(&self) -> Vec<String> {
        self.edges.keys().cloned().collect()
    }

    /// Get dependencies of a specific module
    ///
    /// # Arguments
    /// * `module` - The module name
    ///
    /// # Returns
    /// * `Some(&[String])` - List of dependencies
    /// * `None` - Module not found
    pub fn dependencies(&self, module: &str) -> Option<&[String]> {
        self.edges.get(module).map(|v| v.as_slice())
    }

    /// Check if the graph is empty
    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }

    /// Get the number of modules in the graph
    pub fn len(&self) -> usize {
        self.edges.len()
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_creation() {
        let graph = DependencyGraph::new();
        assert!(graph.is_empty());
        assert_eq!(graph.len(), 0);
    }

    #[test]
    fn test_add_module() {
        let mut graph = DependencyGraph::new();
        graph.add_module("test".to_string());
        assert_eq!(graph.len(), 1);
        assert!(!graph.is_empty());
    }

    #[test]
    fn test_add_dependency() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("main".to_string(), "utils".to_string());

        assert_eq!(graph.len(), 2);
        let deps = graph.dependencies("main").unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0], "utils");
    }

    #[test]
    fn test_no_cycle() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("main".to_string(), "utils".to_string());
        graph.add_dependency("utils".to_string(), "helpers".to_string());

        let cycle = graph.detect_cycle();
        assert!(cycle.is_none());
    }

    #[test]
    fn test_simple_cycle() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("a".to_string(), "b".to_string());
        graph.add_dependency("b".to_string(), "a".to_string());

        let cycle = graph.detect_cycle();
        assert!(cycle.is_some());
        let cycle_path = cycle.unwrap();
        assert!(cycle_path.len() >= 2);
    }

    #[test]
    fn test_complex_cycle() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("a".to_string(), "b".to_string());
        graph.add_dependency("b".to_string(), "c".to_string());
        graph.add_dependency("c".to_string(), "d".to_string());
        graph.add_dependency("d".to_string(), "b".to_string()); // Creates cycle b->c->d->b

        let cycle = graph.detect_cycle();
        assert!(cycle.is_some());
    }

    #[test]
    fn test_self_cycle() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("a".to_string(), "a".to_string());

        let cycle = graph.detect_cycle();
        assert!(cycle.is_some());
    }

    #[test]
    fn test_topological_sort() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("main".to_string(), "utils".to_string());
        graph.add_dependency("utils".to_string(), "helpers".to_string());
        graph.add_dependency("main".to_string(), "config".to_string());

        let sorted = graph.topological_sort().unwrap();

        // helpers should come before utils
        let helpers_idx = sorted.iter().position(|m| m == "helpers").unwrap();
        let utils_idx = sorted.iter().position(|m| m == "utils").unwrap();
        assert!(helpers_idx < utils_idx);

        // utils should come before main
        let main_idx = sorted.iter().position(|m| m == "main").unwrap();
        assert!(utils_idx < main_idx);

        // config should come before main
        let config_idx = sorted.iter().position(|m| m == "config").unwrap();
        assert!(config_idx < main_idx);
    }

    #[test]
    fn test_topological_sort_with_cycle() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("a".to_string(), "b".to_string());
        graph.add_dependency("b".to_string(), "c".to_string());
        graph.add_dependency("c".to_string(), "a".to_string());

        let result = graph.topological_sort();
        assert!(result.is_err());
        match result {
            Err(GraphError::CircularDependency(_)) => {}
            _ => panic!("Expected CircularDependency error"),
        }
    }

    #[test]
    fn test_independent_modules() {
        let mut graph = DependencyGraph::new();
        graph.add_module("a".to_string());
        graph.add_module("b".to_string());
        graph.add_module("c".to_string());

        let sorted = graph.topological_sort().unwrap();
        assert_eq!(sorted.len(), 3);
    }

    #[test]
    fn test_diamond_dependency() {
        let mut graph = DependencyGraph::new();
        //     a
        //    / \
        //   b   c
        //    \ /
        //     d
        graph.add_dependency("a".to_string(), "b".to_string());
        graph.add_dependency("a".to_string(), "c".to_string());
        graph.add_dependency("b".to_string(), "d".to_string());
        graph.add_dependency("c".to_string(), "d".to_string());

        let cycle = graph.detect_cycle();
        assert!(cycle.is_none());

        let sorted = graph.topological_sort().unwrap();
        assert_eq!(sorted.len(), 4);

        // d should come before both b and c
        let d_idx = sorted.iter().position(|m| m == "d").unwrap();
        let b_idx = sorted.iter().position(|m| m == "b").unwrap();
        let c_idx = sorted.iter().position(|m| m == "c").unwrap();
        let a_idx = sorted.iter().position(|m| m == "a").unwrap();

        assert!(d_idx < b_idx);
        assert!(d_idx < c_idx);
        assert!(b_idx < a_idx);
        assert!(c_idx < a_idx);
    }
}
