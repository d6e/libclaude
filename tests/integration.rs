//! Integration tests for libclaude using mock readers.

mod common;

use futures::StreamExt;
use libclaude::stream::ResponseStream;
use libclaude::StreamEvent;

use common::{MockReader, ScenarioBuilder};

#[tokio::test]
async fn simple_text_response() {
    let reader = ScenarioBuilder::new()
        .system_init()
        .text_response("Hello, world!")
        .success_result(Some("Hello, world!"))
        .build();

    let mut stream = ResponseStream::from_reader(reader);
    let mut text = String::new();
    let mut saw_session_init = false;
    let mut saw_complete = false;

    while let Some(event) = stream.next().await {
        let event = event.expect("should not error");
        match event {
            StreamEvent::SessionInit(info) => {
                saw_session_init = true;
                assert_eq!(info.session_id.as_str(), "test-session-123");
            }
            StreamEvent::TextDelta { text: t, .. } => {
                text.push_str(&t);
            }
            StreamEvent::Complete(result) => {
                saw_complete = true;
                assert!(result.is_success());
                assert!(result.total_cost_usd.is_some());
            }
            _ => {}
        }
    }

    assert!(saw_session_init, "should receive SessionInit");
    assert!(saw_complete, "should receive Complete");
    assert_eq!(text, "Hello, world!");
}

#[tokio::test]
async fn collect_text_convenience() {
    let reader = ScenarioBuilder::new()
        .system_init()
        .text_response("The answer is 42")
        .success_result(None)
        .build();

    let stream = ResponseStream::from_reader(reader);
    let text = stream.collect_text().await.expect("should succeed");

    assert_eq!(text, "The answer is 42");
}

#[tokio::test]
async fn collect_all_response() {
    let reader = ScenarioBuilder::new()
        .system_init()
        .text_response("Test response")
        .success_result(Some("Test response"))
        .build();

    let stream = ResponseStream::from_reader(reader);
    let response = stream.collect_all().await.expect("should succeed");

    assert_eq!(response.text, "Test response");
    assert!(response.is_success());
    assert!(response.session_id.is_some());
    assert!(!response.events.is_empty());
}

#[tokio::test]
async fn streaming_text_deltas() {
    let long_text = "This is a longer response that will be chunked into multiple deltas for streaming.";
    let reader = ScenarioBuilder::new()
        .system_init()
        .text_response(long_text)
        .success_result(None)
        .build();

    let mut stream = ResponseStream::from_reader(reader);
    let mut delta_count = 0;
    let mut collected_text = String::new();

    while let Some(event) = stream.next().await {
        if let Ok(StreamEvent::TextDelta { text, .. }) = event {
            delta_count += 1;
            collected_text.push_str(&text);
        }
    }

    assert!(delta_count > 1, "should receive multiple text deltas");
    assert_eq!(collected_text, long_text);
}

#[tokio::test]
async fn content_block_complete() {
    let reader = ScenarioBuilder::new()
        .system_init()
        .text_response("Hello")
        .success_result(None)
        .build();

    let mut stream = ResponseStream::from_reader(reader);
    let mut saw_block_complete = false;

    while let Some(event) = stream.next().await {
        if let Ok(StreamEvent::ContentBlockComplete { block, index }) = event {
            saw_block_complete = true;
            assert_eq!(index, 0);
            assert!(block.text().is_some());
            assert_eq!(block.text().unwrap(), "Hello");
        }
    }

    assert!(saw_block_complete, "should receive ContentBlockComplete");
}

#[tokio::test]
async fn usage_updates() {
    let reader = ScenarioBuilder::new()
        .system_init()
        .text_response("Response")
        .success_result(None)
        .build();

    let mut stream = ResponseStream::from_reader(reader);
    let mut saw_usage_update = false;

    while let Some(event) = stream.next().await {
        if let Ok(StreamEvent::UsageUpdate(usage)) = event {
            saw_usage_update = true;
            assert!(usage.input_tokens > 0);
        }
    }

    assert!(saw_usage_update, "should receive UsageUpdate");
}

#[tokio::test]
async fn ping_events_are_ignored() {
    let reader = ScenarioBuilder::new()
        .system_init()
        .ping()
        .ping()
        .text_response("Hello")
        .ping()
        .success_result(None)
        .build();

    let stream = ResponseStream::from_reader(reader);
    let text = stream.collect_text().await.expect("should succeed");

    // Pings should be silently ignored
    assert_eq!(text, "Hello");
}

#[tokio::test]
async fn error_result_in_collect_text() {
    let reader = ScenarioBuilder::new()
        .system_init()
        .error_result("Something went wrong")
        .build();

    let stream = ResponseStream::from_reader(reader);
    let result = stream.collect_text().await;

    assert!(result.is_err());
}

#[tokio::test]
async fn session_id_available_after_init() {
    let reader = ScenarioBuilder::new()
        .session_id("custom-session-456")
        .system_init()
        .text_response("Hi")
        .success_result(None)
        .build();

    let mut stream = ResponseStream::from_reader(reader);

    // Initially no session ID
    assert!(stream.session_id().is_none());

    // Read until we get session init
    while let Some(event) = stream.next().await {
        if let Ok(StreamEvent::SessionInit(_)) = event {
            break;
        }
    }

    // Now session ID should be available
    assert_eq!(stream.session_id().unwrap().as_str(), "custom-session-456");
}

#[tokio::test]
async fn empty_response() {
    let reader = ScenarioBuilder::new()
        .system_init()
        .text_response("")
        .success_result(None)
        .build();

    let stream = ResponseStream::from_reader(reader);
    let text = stream.collect_text().await.expect("should succeed");

    assert_eq!(text, "");
}

#[tokio::test]
async fn unicode_response() {
    let unicode_text = "Hello! Here's some unicode: \u{1F600} \u{1F389} \u{2764}\u{FE0F} and Chinese: \u{4F60}\u{597D}";
    let reader = ScenarioBuilder::new()
        .system_init()
        .text_response(unicode_text)
        .success_result(None)
        .build();

    let stream = ResponseStream::from_reader(reader);
    let text = stream.collect_text().await.expect("should succeed");

    assert_eq!(text, unicode_text);
}

#[tokio::test]
async fn reader_error_propagates() {
    use libclaude::Error;

    let reader = MockReader::with_error(
        vec![],
        Error::Io(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "pipe broke",
        )),
    );

    let stream = ResponseStream::from_reader(reader);
    let result = stream.collect_text().await;

    assert!(result.is_err());
}
