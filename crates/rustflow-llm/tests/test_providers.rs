use rustflow_llm::provider::LlmProvider;
use rustflow_llm::providers::{GlmProvider, OpenAiProvider};
use rustflow_llm::types::LlmRequest;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn serve_json_once(body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = [0; 4096];
        let _ = socket.read(&mut buffer).await.unwrap();
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        socket.write_all(response.as_bytes()).await.unwrap();
    });

    format!("http://{}", addr)
}

#[tokio::test]
async fn openai_metadata_distinguishes_requested_effective_and_served_models() {
    let base_url = serve_json_once(
        r#"{
            "model": "served-model",
            "choices": [
                {
                    "message": { "content": "hello" },
                    "finish_reason": "stop"
                }
            ],
            "usage": {
                "prompt_tokens": 3,
                "completion_tokens": 2
            }
        }"#,
    )
    .await;
    let provider = OpenAiProvider::new("test-key").with_base_url(base_url);
    let request = LlmRequest::new("requested-model", vec![]);

    let response = provider.complete(&request).await.unwrap();

    let metadata = response.metadata.unwrap();
    assert_eq!(metadata.provider, "openai");
    assert_eq!(metadata.requested_model, "requested-model");
    assert_eq!(metadata.effective_model, "requested-model");
    assert_eq!(metadata.served_model, "served-model");
}

#[tokio::test]
async fn glm_metadata_uses_default_model_as_effective_model() {
    let base_url = serve_json_once(
        r#"{
            "model": "served-glm",
            "choices": [
                {
                    "message": { "content": "hello" },
                    "finish_reason": "stop"
                }
            ],
            "usage": {
                "prompt_tokens": 3,
                "completion_tokens": 2
            }
        }"#,
    )
    .await;
    let provider = GlmProvider::new("test-key")
        .with_model("configured-glm")
        .with_base_url(base_url);
    let request = LlmRequest::new("", vec![]);

    let response = provider.complete(&request).await.unwrap();

    let metadata = response.metadata.unwrap();
    assert_eq!(metadata.provider, "glm");
    assert_eq!(metadata.requested_model, "");
    assert_eq!(metadata.effective_model, "configured-glm");
    assert_eq!(metadata.served_model, "served-glm");
}
