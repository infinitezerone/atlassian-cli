use atlassian_cli::http::HttpClient;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_mock_get_retry_on_503() {
    let mock_server = MockServer::start().await;

    // 前 2 次返回 503, 第 3 次返回 200 成功
    Mock::given(method("GET"))
        .and(path("/api/flaky"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(2)
        .expect(2)
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/flaky"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "result": "recovered" })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = HttpClient::new(mock_server.uri(), "test-token", false).unwrap();
    let res = client.get("/api/flaky").await.unwrap();
    assert_eq!(res["result"], "recovered");
}

#[tokio::test]
async fn test_mock_post_idempotency_skip() {
    let mock_server = MockServer::start().await;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let test_path = format!("/api/write-{}", nanos);

    // MockServer 仅期望接收到 1 次 POST 请求
    Mock::given(method("POST"))
        .and(path(test_path.clone()))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": "created-1" })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = HttpClient::new(mock_server.uri(), "test-token", false).unwrap();
    let body = serde_json::json!({ "summary": "test idempotency item" });

    // 第 1 次执行: 真正调用 MockServer
    let res1 = client.post(&test_path, body.clone()).await.unwrap();
    assert_eq!(res1["id"], "created-1");

    // 第 2 次执行: 相同参数被幂等窗口拦截, 不再向 MockServer 发送请求
    let res2 = client.post(&test_path, body).await.unwrap();
    assert_eq!(res2["status"], "idempotent_replay");
    assert_eq!(res2["action"], "skipped");
}
