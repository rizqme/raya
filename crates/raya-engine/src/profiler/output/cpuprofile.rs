//! Chrome DevTools `.cpuprofile` JSON output.
//!
//! Compatible with Chrome DevTools Performance panel, VS Code, and speedscope.app.

use crate::profiler::{ResolvedFrame, ResolvedProfileData, ResolvedSample};
use rustc_hash::FxHashMap;
use serde::Serialize;

/// Chrome DevTools `.cpuprofile` format.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuProfile {
    pub nodes: Vec<CpuProfileNode>,
    pub start_time: u64,
    pub end_time: u64,
    pub samples: Vec<u32>,
    pub time_deltas: Vec<i64>,
}

/// A node in the call tree.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuProfileNode {
    pub id: u32,
    pub call_frame: CallFrame,
    pub hit_count: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<u32>,
}

/// Source location for a node.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallFrame {
    pub function_name: String,
    pub script_id: String,
    pub url: String,
    /// 0-indexed (cpuprofile convention).
    pub line_number: i32,
    /// 0-indexed.
    pub column_number: i32,
}

// ---------------------------------------------------------------------------
// Call-tree builder
// ---------------------------------------------------------------------------

/// Trie node for building the call tree.
struct TrieNode {
    id: u32,
    frame: ResolvedFrame,
    hit_count: u32,
    children: FxHashMap<ResolvedFrame, usize>, // frame → index in `nodes`
}

impl ResolvedProfileData {
    /// Convert to Chrome DevTools `.cpuprofile` JSON string.
    pub fn to_cpuprofile_json(&self) -> String {
        let profile = self.build_cpuprofile();
        serde_json::to_string_pretty(&profile).unwrap_or_else(|_| "{}".to_string())
    }

    fn build_cpuprofile(&self) -> CpuProfile {
        let mut nodes: Vec<TrieNode> = Vec::new();
        let mut samples_list: Vec<u32> = Vec::new();
        let mut time_deltas: Vec<i64> = Vec::new();

        // Root node (required by cpuprofile format)
        nodes.push(TrieNode {
            id: 1,
            frame: ResolvedFrame {
                function_name: "(root)".to_string(),
                source_file: String::new(),
                line_number: 0,
                column_number: 0,
            },
            hit_count: 0,
            children: FxHashMap::default(),
        });

        let mut prev_timestamp: Option<u64> = None;

        for sample in &self.samples {
            let leaf_id = self.insert_sample_into_trie(&mut nodes, sample);
            samples_list.push(leaf_id);

            let delta = match prev_timestamp {
                Some(prev) => sample.timestamp_us as i64 - prev as i64,
                None => 0,
            };
            time_deltas.push(delta);
            prev_timestamp = Some(sample.timestamp_us);
        }

        // Convert trie to flat node list
        let cpu_nodes = nodes
            .iter()
            .map(|n| CpuProfileNode {
                id: n.id,
                call_frame: CallFrame {
                    function_name: n.frame.function_name.clone(),
                    script_id: "0".to_string(),
                    url: n.frame.source_file.clone(),
                    // cpuprofile uses 0-indexed lines; our data is 1-indexed
                    line_number: if n.frame.line_number > 0 {
                        n.frame.line_number as i32 - 1
                    } else {
                        -1
                    },
                    column_number: if n.frame.column_number > 0 {
                        n.frame.column_number as i32 - 1
                    } else {
                        -1
                    },
                },
                hit_count: n.hit_count,
                children: n
                    .children
                    .values()
                    .map(|&idx| nodes[idx].id)
                    .collect(),
            })
            .collect();

        CpuProfile {
            nodes: cpu_nodes,
            start_time: self.start_time_us,
            end_time: self.end_time_us,
            samples: samples_list,
            time_deltas,
        }
    }

    /// Walk the sample's frame stack through the trie, creating nodes as needed.
    /// Returns the leaf node ID.
    fn insert_sample_into_trie(
        &self,
        nodes: &mut Vec<TrieNode>,
        sample: &ResolvedSample,
    ) -> u32 {
        let mut current_idx: usize = 0; // Start at root

        for frame in &sample.frames {
            let child_idx = if let Some(&idx) = nodes[current_idx].children.get(frame) {
                idx
            } else {
                let new_id = nodes.len() as u32 + 1; // 1-indexed
                let new_idx = nodes.len();
                nodes.push(TrieNode {
                    id: new_id,
                    frame: frame.clone(),
                    hit_count: 0,
                    children: FxHashMap::default(),
                });
                nodes[current_idx]
                    .children
                    .insert(frame.clone(), new_idx);
                new_idx
            };
            current_idx = child_idx;
        }

        // Increment hit count on the leaf
        nodes[current_idx].hit_count += 1;
        nodes[current_idx].id
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiler::ResolvedFrame;

    fn frame(name: &str, file: &str, line: u32) -> ResolvedFrame {
        ResolvedFrame {
            function_name: name.to_string(),
            source_file: file.to_string(),
            line_number: line,
            column_number: 1,
        }
    }

    #[test]
    fn test_empty_profile() {
        let data = ResolvedProfileData {
            samples: vec![],
            start_time_us: 0,
            end_time_us: 1_000_000,
        };
        let json = data.to_cpuprofile_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["nodes"].is_array());
        assert_eq!(parsed["nodes"].as_array().unwrap().len(), 1); // root only
        assert!(parsed["samples"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_single_sample() {
        let data = ResolvedProfileData {
            samples: vec![ResolvedSample {
                timestamp_us: 10_000,
                task_id: 1,
                frames: vec![
                    frame("main", "app.raya", 1),
                    frame("fibonacci", "app.raya", 10),
                ],
            }],
            start_time_us: 0,
            end_time_us: 20_000,
        };
        let json = data.to_cpuprofile_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // root + main + fibonacci = 3 nodes
        assert_eq!(parsed["nodes"].as_array().unwrap().len(), 3);
        assert_eq!(parsed["samples"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_shared_prefix_deduplication() {
        let data = ResolvedProfileData {
            samples: vec![
                ResolvedSample {
                    timestamp_us: 10_000,
                    task_id: 1,
                    frames: vec![
                        frame("main", "app.raya", 1),
                        frame("compute", "app.raya", 5),
                    ],
                },
                ResolvedSample {
                    timestamp_us: 20_000,
                    task_id: 1,
                    frames: vec![
                        frame("main", "app.raya", 1),
                        frame("compute", "app.raya", 5),
                    ],
                },
            ],
            start_time_us: 0,
            end_time_us: 30_000,
        };
        let json = data.to_cpuprofile_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // root + main + compute = 3 nodes (shared prefix)
        assert_eq!(parsed["nodes"].as_array().unwrap().len(), 3);
        // compute has hit_count = 2
        let compute_node = parsed["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["callFrame"]["functionName"] == "compute")
            .unwrap();
        assert_eq!(compute_node["hitCount"], 2);
    }

    #[test]
    fn test_time_deltas() {
        let data = ResolvedProfileData {
            samples: vec![
                ResolvedSample {
                    timestamp_us: 10_000,
                    task_id: 1,
                    frames: vec![frame("main", "a.raya", 1)],
                },
                ResolvedSample {
                    timestamp_us: 20_000,
                    task_id: 1,
                    frames: vec![frame("main", "a.raya", 1)],
                },
                ResolvedSample {
                    timestamp_us: 30_000,
                    task_id: 1,
                    frames: vec![frame("main", "a.raya", 1)],
                },
            ],
            start_time_us: 0,
            end_time_us: 40_000,
        };
        let profile = data.build_cpuprofile();
        assert_eq!(profile.time_deltas, vec![0, 10_000, 10_000]);
    }

    #[test]
    fn test_line_number_0_indexed() {
        let data = ResolvedProfileData {
            samples: vec![ResolvedSample {
                timestamp_us: 10_000,
                task_id: 1,
                frames: vec![frame("foo", "x.raya", 5)],
            }],
            start_time_us: 0,
            end_time_us: 20_000,
        };
        let json = data.to_cpuprofile_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        let foo_node = parsed["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["callFrame"]["functionName"] == "foo")
            .unwrap();
        // 5 in our data → 4 in cpuprofile (0-indexed)
        assert_eq!(foo_node["callFrame"]["lineNumber"], 4);
    }
}
