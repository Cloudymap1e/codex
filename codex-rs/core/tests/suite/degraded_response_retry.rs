use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::future::Future;
use tokio::time::Duration;
use tokio::time::Instant;
use wiremock::MockServer;

fn ev_completed_with_reasoning_tokens(id: &str, reasoning_tokens: i64) -> serde_json::Value {
    json!({
        "type": "response.completed",
        "response": {
            "id": id,
            "usage": {
                "input_tokens": 10,
                "input_tokens_details": null,
                "output_tokens": 20,
                "output_tokens_details": {"reasoning_tokens": reasoning_tokens},
                "total_tokens": 30
            }
        }
    })
}

fn agent_message_text(item: &codex_protocol::items::AgentMessageItem) -> String {
    item.content
        .iter()
        .map(|entry| match entry {
            AgentMessageContent::Text { text } => text.as_str(),
        })
        .collect()
}

#[derive(Debug, Default, PartialEq, Eq)]
struct CollectedAgentOutput {
    completed_messages: Vec<String>,
    content_deltas: Vec<String>,
}

async fn submit_turn_collecting_agent_output(
    test: &core_test_support::test_codex::TestCodex,
    prompt: &str,
) -> anyhow::Result<CollectedAgentOutput> {
    test.codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: prompt.to_string(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            responsesapi_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await?;

    let mut agent_output = CollectedAgentOutput::default();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for degraded-response retry turn completion"
        );
        let event = test
            .codex
            .next_event()
            .await
            .expect("event stream should stay open")
            .msg;
        match event {
            EventMsg::AgentMessageContentDelta(event) => {
                agent_output.content_deltas.push(event.delta);
            }
            EventMsg::ItemCompleted(event) => {
                if let TurnItem::AgentMessage(agent_message) = event.item {
                    agent_output
                        .completed_messages
                        .push(agent_message_text(&agent_message));
                }
            }
            EventMsg::TurnComplete(_) => return Ok(agent_output),
            _ => {}
        }
    }
}

fn run_async_test_on_large_stack<F, Fut>(test: F) -> anyhow::Result<()>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<()>> + 'static,
{
    let handle = std::thread::Builder::new()
        .name("degraded-response-retry-test".to_string())
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(test())
        })?;
    match handle.join() {
        Ok(result) => result,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

#[test]
fn retries_degraded_reasoning_response_with_stronger_prompt() -> anyhow::Result<()> {
    run_async_test_on_large_stack(|| async {
        let server = MockServer::start().await;
        let mut response_bodies = Vec::new();
        for (attempt, reasoning_tokens) in [(1, 516), (2, 1034)] {
            response_bodies.push(responses::sse(vec![
                responses::ev_response_created(&format!("resp-bad-{attempt}")),
                responses::ev_assistant_message(&format!("msg-bad-{attempt}"), "bad answer"),
                ev_completed_with_reasoning_tokens(
                    &format!("resp-bad-{attempt}"),
                    reasoning_tokens,
                ),
            ]));
        }
        response_bodies.push(responses::sse(vec![
            responses::ev_response_created("resp-good"),
            responses::ev_assistant_message("msg-good", "good answer"),
            responses::ev_completed("resp-good"),
        ]));

        let response_mock = responses::mount_sse_sequence(&server, response_bodies).await;
        let test = test_codex()
            .with_config(|config| {
                config.model_provider.request_max_retries = Some(0);
                config.model_provider.stream_max_retries = Some(0);
            })
            .build(&server)
            .await?;

        let agent_output = submit_turn_collecting_agent_output(&test, "answer carefully").await?;

        let requests = response_mock.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(
            agent_output.completed_messages,
            vec!["good answer".to_string()]
        );
        for (retry_attempt, request) in requests.iter().enumerate().skip(1) {
            let expected_reasoning_tokens = if retry_attempt == 1 { 516 } else { 1034 };
            assert!(request.body_contains_text("answer carefully"));
            assert!(
                request.body_contains_text("automatic recovery retry attempt"),
                "retry request should include the recovery retry prompt"
            );
            assert!(
                request.body_contains_text("Maximize your reasoning effort before answering"),
                "retry request should require maximized reasoning effort"
            );
            assert!(
                request.body_contains_text(
                    "do not deliver incomplete, low-effort, placeholder, degraded, or shit work"
                ),
                "retry request should explicitly reject lazy or degraded work"
            );
            assert!(
                request.body_contains_text(
                    "Do not deliver an intentionally degraded response, and do not accidentally repeat the degraded response pattern"
                ),
                "retry request should explicitly block intentional degradation and repeated degraded patterns"
            );
            assert!(
                request.body_contains_text(
                    "If the draft looks shallow, generic, truncated, evasive, or under-verified"
                ),
                "retry request should require discarding shallow or under-verified drafts"
            );
            assert!(
                request.body_contains_text(&format!(
                    "known degraded reasoning-token signature {expected_reasoning_tokens}"
                )),
                "retry request should explain the degraded reasoning-token signature"
            );
            assert!(
                request
                    .body_contains_text(&format!("recovery retry attempt {retry_attempt} of 20")),
                "retry request should include the retry attempt"
            );
            assert!(
                request.body_contains_text(&format!(
                    "Avoid repeating the last bad reasoning-token signature {expected_reasoning_tokens}"
                )),
                "retry request should explicitly block repeating the bad signature"
            );
            assert!(
                !request.body_contains_text("bad answer"),
                "retry request should not keep the discarded response in model-visible history"
            );
        }

        Ok(())
    })
}

#[test]
fn discards_streamed_degraded_assistant_output_before_retry() -> anyhow::Result<()> {
    run_async_test_on_large_stack(|| async {
        let server = MockServer::start().await;
        let response_mock = responses::mount_sse_sequence(
            &server,
            vec![
                responses::sse(vec![
                    responses::ev_response_created("resp-bad"),
                    responses::ev_message_item_added("msg-bad", ""),
                    responses::ev_output_text_delta("bad "),
                    responses::ev_output_text_delta("answer"),
                    responses::ev_assistant_message("msg-bad", "bad answer"),
                    ev_completed_with_reasoning_tokens("resp-bad", 516),
                ]),
                responses::sse(vec![
                    responses::ev_response_created("resp-good"),
                    responses::ev_message_item_added("msg-good", ""),
                    responses::ev_output_text_delta("good "),
                    responses::ev_output_text_delta("answer"),
                    responses::ev_assistant_message("msg-good", "good answer"),
                    responses::ev_completed("resp-good"),
                ]),
            ],
        )
        .await;
        let test = test_codex()
            .with_config(|config| {
                config.model_provider.request_max_retries = Some(0);
                config.model_provider.stream_max_retries = Some(0);
            })
            .build(&server)
            .await?;

        let agent_output = submit_turn_collecting_agent_output(&test, "stream carefully").await?;

        assert_eq!(response_mock.requests().len(), 2);
        assert_eq!(
            agent_output,
            CollectedAgentOutput {
                completed_messages: vec!["good answer".to_string()],
                content_deltas: vec!["good ".to_string(), "answer".to_string()],
            }
        );

        Ok(())
    })
}
