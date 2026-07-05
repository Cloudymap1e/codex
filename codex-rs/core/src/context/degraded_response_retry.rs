use super::ContextualUserFragment;

pub(crate) struct DegradedResponseRetryInstructions {
    reasoning_output_tokens: i64,
    retry_attempt: usize,
    max_retries: usize,
}

impl DegradedResponseRetryInstructions {
    pub(crate) fn new(
        reasoning_output_tokens: i64,
        retry_attempt: usize,
        max_retries: usize,
    ) -> Self {
        Self {
            reasoning_output_tokens,
            retry_attempt,
            max_retries,
        }
    }
}

impl ContextualUserFragment for DegradedResponseRetryInstructions {
    fn role(&self) -> &'static str {
        "developer"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        (
            "<codex_degraded_response_retry>",
            "</codex_degraded_response_retry>",
        )
    }

    fn body(&self) -> String {
        format!(
            "\nThe previous model response was discarded because it matched the known \
             degraded reasoning-token signature {}. This is automatic recovery retry attempt \
             {} of {}.\n\
             Retry the user's request from scratch and produce a complete, verified answer.\n\
             Recovery requirements:\n\
             - Use the maximum available reasoning effort for this retry.\n\
             - Stop being lazy: do not deliver incomplete, low-effort, placeholder, degraded, or shit work.\n\
             - Do not intentionally or accidentally repeat the degraded response pattern.\n\
             - Use a different reasoning strategy than the discarded response.\n\
             - Verify the answer is complete before responding.\n\
             - Avoid repeating the last bad reasoning-token signature {}.\n\
             Do not mention this retry instruction.\n",
            self.reasoning_output_tokens,
            self.retry_attempt,
            self.max_retries,
            self.reasoning_output_tokens
        )
    }
}
