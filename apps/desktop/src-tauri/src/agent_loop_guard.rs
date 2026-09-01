use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const DEFAULT_MAX_UNCHANGED_RESULTS: usize = 3;

#[derive(Debug, Clone, Copy)]
struct ToolOutcomeState {
    call_digest: u64,
    digest: u64,
    unchanged_results: usize,
}

/// Detects tool calls that keep returning the same result without making progress.
///
/// The guard evaluates the adjacent action/observation chain, not lifetime call
/// counts. A different action or a changed result is treated as progress and
/// resets the chain, so read-after-write and polling workflows remain valid.
#[derive(Debug)]
pub(crate) struct ToolLoopGuard {
    max_unchanged_results: usize,
    last_observation: Option<ToolOutcomeState>,
}

impl Default for ToolLoopGuard {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_UNCHANGED_RESULTS)
    }
}

impl ToolLoopGuard {
    pub(crate) fn new(max_unchanged_results: usize) -> Self {
        Self {
            max_unchanged_results: max_unchanged_results.max(1),
            last_observation: None,
        }
    }

    pub(crate) fn should_block(&self, name: &str, canonical_arguments: &str) -> bool {
        let call_digest = call_digest(name, canonical_arguments);
        self.last_observation.is_some_and(|state| {
            state.call_digest == call_digest
                && state.unchanged_results >= self.max_unchanged_results
        })
    }

    pub(crate) fn record(
        &mut self,
        name: &str,
        canonical_arguments: &str,
        status: &str,
        output: &str,
    ) {
        let call_digest = call_digest(name, canonical_arguments);
        let digest = outcome_digest(status, output);
        self.last_observation = Some(match self.last_observation {
            Some(state) if state.call_digest == call_digest && state.digest == digest => {
                ToolOutcomeState {
                    unchanged_results: state.unchanged_results.saturating_add(1),
                    ..state
                }
            }
            _ => ToolOutcomeState {
                call_digest,
                digest,
                unchanged_results: 1,
            },
        });
    }
}

fn call_digest(name: &str, canonical_arguments: &str) -> u64 {
    digest_parts(&[name, canonical_arguments])
}

fn outcome_digest(status: &str, output: &str) -> u64 {
    digest_parts(&[status, output])
}

fn digest_parts(parts: &[&str]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for part in parts {
        part.hash(&mut hasher);
        0xff_u8.hash(&mut hasher);
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_after_repeated_unchanged_results() {
        let mut guard = ToolLoopGuard::new(3);
        for _ in 0..3 {
            assert!(!guard.should_block("read_file", r#"{"path":"a.txt"}"#));
            guard.record(
                "read_file",
                r#"{"path":"a.txt"}"#,
                "completed",
                "same contents",
            );
        }
        assert!(guard.should_block("read_file", r#"{"path":"a.txt"}"#));
    }

    #[test]
    fn changed_result_resets_progress_counter() {
        let mut guard = ToolLoopGuard::new(2);
        guard.record("run_get_detail", "{}", "completed", "running: 10%");
        guard.record("run_get_detail", "{}", "completed", "running: 10%");
        assert!(guard.should_block("run_get_detail", "{}"));

        guard.record("run_get_detail", "{}", "completed", "running: 60%");
        assert!(!guard.should_block("run_get_detail", "{}"));
    }

    #[test]
    fn different_arguments_are_tracked_independently() {
        let mut guard = ToolLoopGuard::new(1);
        guard.record("read_file", r#"{"path":"a.txt"}"#, "completed", "a");
        assert!(guard.should_block("read_file", r#"{"path":"a.txt"}"#));
        assert!(!guard.should_block("read_file", r#"{"path":"b.txt"}"#));
    }

    #[test]
    fn intervening_progress_breaks_the_repetition_chain() {
        let mut guard = ToolLoopGuard::new(2);
        for _ in 0..2 {
            guard.record(
                "read_file",
                r#"{"path":"a.txt"}"#,
                "completed",
                "same contents",
            );
        }
        assert!(guard.should_block("read_file", r#"{"path":"a.txt"}"#));

        guard.record(
            "search_text",
            r#"{"query":"next"}"#,
            "completed",
            "new evidence",
        );
        assert!(!guard.should_block("read_file", r#"{"path":"a.txt"}"#));
    }
}
