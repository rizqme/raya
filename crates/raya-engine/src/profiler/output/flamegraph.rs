//! Brendan Gregg folded stacks output format.
//!
//! Each line: `func1;func2;func3 <count>`
//!
//! Feed to `flamegraph.pl` or import into speedscope.app.

use crate::profiler::ResolvedProfileData;
use std::collections::BTreeMap;

impl ResolvedProfileData {
    /// Convert to folded stacks format (one stack per line, count appended).
    pub fn to_flamegraph(&self) -> String {
        // Aggregate identical stacks
        let mut stacks: BTreeMap<String, u32> = BTreeMap::new();

        for sample in &self.samples {
            let key = sample
                .frames
                .iter()
                .map(|f| {
                    if f.source_file.is_empty() {
                        f.function_name.clone()
                    } else {
                        format!(
                            "{} ({}:{})",
                            f.function_name, f.source_file, f.line_number
                        )
                    }
                })
                .collect::<Vec<_>>()
                .join(";");

            *stacks.entry(key).or_insert(0) += 1;
        }

        let mut output = String::new();
        for (stack, count) in &stacks {
            if !stack.is_empty() {
                output.push_str(stack);
                output.push(' ');
                output.push_str(&count.to_string());
                output.push('\n');
            }
        }
        output
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::profiler::{ResolvedFrame, ResolvedProfileData, ResolvedSample};

    fn frame(name: &str, file: &str, line: u32) -> ResolvedFrame {
        ResolvedFrame {
            function_name: name.to_string(),
            source_file: file.to_string(),
            line_number: line,
            column_number: 1,
        }
    }

    #[test]
    fn test_empty_flamegraph() {
        let data = ResolvedProfileData {
            samples: vec![],
            start_time_us: 0,
            end_time_us: 1_000_000,
        };
        let output = data.to_flamegraph();
        assert!(output.is_empty());
    }

    #[test]
    fn test_single_stack() {
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
        let output = data.to_flamegraph();
        assert_eq!(
            output.trim(),
            "main (app.raya:1);fibonacci (app.raya:10) 1"
        );
    }

    #[test]
    fn test_aggregation() {
        let data = ResolvedProfileData {
            samples: vec![
                ResolvedSample {
                    timestamp_us: 10_000,
                    task_id: 1,
                    frames: vec![frame("main", "a.raya", 1), frame("hot", "a.raya", 5)],
                },
                ResolvedSample {
                    timestamp_us: 20_000,
                    task_id: 1,
                    frames: vec![frame("main", "a.raya", 1), frame("hot", "a.raya", 5)],
                },
                ResolvedSample {
                    timestamp_us: 30_000,
                    task_id: 1,
                    frames: vec![frame("main", "a.raya", 1), frame("cold", "a.raya", 15)],
                },
            ],
            start_time_us: 0,
            end_time_us: 40_000,
        };
        let output = data.to_flamegraph();
        let lines: Vec<&str> = output.trim().lines().collect();
        assert_eq!(lines.len(), 2);
        // BTreeMap sorts lexicographically
        assert!(lines[0].contains("cold") && lines[0].ends_with(" 1"));
        assert!(lines[1].contains("hot") && lines[1].ends_with(" 2"));
    }

    #[test]
    fn test_no_source_file() {
        let data = ResolvedProfileData {
            samples: vec![ResolvedSample {
                timestamp_us: 10_000,
                task_id: 1,
                frames: vec![ResolvedFrame {
                    function_name: "native_func".to_string(),
                    source_file: String::new(),
                    line_number: 0,
                    column_number: 0,
                }],
            }],
            start_time_us: 0,
            end_time_us: 20_000,
        };
        let output = data.to_flamegraph();
        assert_eq!(output.trim(), "native_func 1");
    }
}
