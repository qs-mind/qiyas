use std::sync::{Arc, Mutex};

use qiyas_ai::provider::{BoxFuture, Sink};
use qiyas_ai::surface::EXECUTE;
use qiyas_ai::{
    Agent, AgentEvent, AgentSettings, AiError, Autonomy, Provider, StreamEvent, TranscriptEntry,
    TurnRequest, TurnResult,
};
use serde_json::json;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::scripted::Script;

struct StreamingScript(Script);

impl Provider for StreamingScript {
    fn stream_turn<'a>(
        &'a self,
        request: TurnRequest,
        sink: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
        for delta in ["Ready —", "— x", " – y―z"] {
            sink(StreamEvent::TextDelta(delta.into()));
        }
        self.0.stream_turn(request, sink)
    }
}

async fn events_for(agent: &mut Agent) -> Vec<AgentEvent> {
    let (tx, mut rx) = mpsc::channel(1024);
    agent
        .run_turn("Start".into(), tx, CancellationToken::new())
        .await
        .unwrap();
    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }
    events
}

#[tokio::test]
async fn streamed_text_and_questions_use_plain_dashes_in_the_channel_and_transcript() {
    let provider = StreamingScript(Script::new(
        vec![(
            EXECUTE,
            json!({"op":"ask","args":{"questions":[{
                "id":"choice","question":"Choose — now?",
                "options":[
                    {"label":"First — option","description":"Fast – simple―clear"},
                    {"label":"Second – option"}
                ]
            }]}}),
        )],
        Default::default(),
    ));
    let mut agent = Agent::new(
        Box::new(provider),
        Arc::new(Mutex::new(None)),
        AgentSettings::default(),
    );
    let events = events_for(&mut agent).await;
    let text: String = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "Ready -- x - y-z");
    let questions = events
        .iter()
        .find_map(|event| match event {
            AgentEvent::Question { questions } => Some(questions),
            _ => None,
        })
        .expect("question event");
    assert_eq!(questions[0].question, "Choose - now?");
    assert_eq!(questions[0].options[0].label, "First - option");
    assert_eq!(
        questions[0].options[0].description.as_deref(),
        Some("Fast - simple-clear")
    );
    assert_eq!(questions[0].options[1].label, "Second - option");
    assert_eq!(questions[0].options[1].description, None);
    let transcript = agent.snapshot().transcript;
    let saved_text: String = transcript
        .iter()
        .filter_map(|entry| match entry {
            TranscriptEntry::Event {
                event: AgentEvent::Text { text },
            } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(saved_text, text);
    assert!(transcript.iter().any(|entry| matches!(entry,
        TranscriptEntry::Event { event: AgentEvent::Question { questions: saved } } if saved == questions
    )));
}

#[tokio::test]
async fn plans_and_approval_titles_use_plain_dashes() {
    let mut agent = Agent::new(
        Box::new(Script::new(
            vec![(
                EXECUTE,
                json!({"op":"plan","args":{"phases":[
                    {"id":"check","title":"Check — sizes – first―now","status":"active"}
                ]}}),
            )],
            Default::default(),
        )),
        Arc::new(Mutex::new(None)),
        AgentSettings {
            autonomy: Autonomy::PerJob,
            ..Default::default()
        },
    );
    let events = events_for(&mut agent).await;
    assert!(events.iter().any(|event| matches!(event,
        AgentEvent::Plan { phases } if phases[0].title == "Check - sizes - first-now"
    )));
    assert!(events.iter().any(|event| matches!(event,
        AgentEvent::AwaitingApproval { plan } if plan.phases[0].title == "Check - sizes - first-now"
    )));
}
