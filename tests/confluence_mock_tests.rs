use atlassian_cli::confluence::{Confluence, UpdatePageArgs};
use atlassian_cli::http::HttpClient;
use atlassian_cli::module::WritePolicy;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_mock_confluence_get_page_truncation() {
    let mock_server = MockServer::start().await;

    let page_raw = json!({
        "id": "123456",
        "title": "Architecture Guide",
        "space": { "key": "ENG" },
        "version": { "number": 1 },
        "body": {
            "storage": {
                "value": "<p>This is a very long documentation about system design patterns and best practices.</p>"
            }
        }
    });

    Mock::given(method("GET"))
        .and(path("/rest/api/content/123456"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_raw))
        .mount(&mock_server)
        .await;

    let http = HttpClient::new(mock_server.uri(), "token", false).unwrap();
    let confluence = Confluence::new(http, WritePolicy { dry_run: false, confirm: true });

    let res = confluence.get_page("123456", false, false, 20, 0).await.unwrap();
    assert_eq!(res["id"], "123456");
    assert_eq!(res["title"], "Architecture Guide");
    assert_eq!(res["returned_chars"], 20);
    assert_eq!(res["is_truncated"], true);
    assert!(res["hint"].as_str().unwrap().contains("--offset 20"));
}

#[tokio::test]
async fn test_mock_confluence_update_page_safe_find_replace() {
    let mock_server = MockServer::start().await;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let page_id = format!("{}", nanos % 1000000);
    let content_path = format!("/rest/api/content/{}", page_id);

    // 1. 获取原页面 (Version 1)
    Mock::given(method("GET"))
        .and(path(content_path.clone()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": page_id,
            "title": "Project Roadmap",
            "version": { "number": 1 },
            "body": {
                "storage": {
                    "value": "<p>Target Version: 1.0.0 (Old Release)</p>"
                }
            }
        })))
        .mount(&mock_server)
        .await;

    // 2. PUT 提交更新 (Version 2)
    Mock::given(method("PUT"))
        .and(path(content_path))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": page_id,
            "title": "Project Roadmap",
            "version": { "number": 2 }
        })))
        .mount(&mock_server)
        .await;

    let http = HttpClient::new(mock_server.uri(), "token", false).unwrap();
    let confluence = Confluence::new(http, WritePolicy { dry_run: false, confirm: true });

    let args = UpdatePageArgs {
        id_or_url: page_id.clone(),
        title: None,
        body: None,
        find: Some("1.0.0 (Old Release)".to_string()),
        replace: Some("1.0.1 (New Release)".to_string()),
        append: None,
        prepend: None,
    };

    let res = confluence.update_page(&args).await.unwrap();

    assert_eq!(res["status"], "success");
    assert_eq!(res["version"], 2);
    assert_eq!(res["id"], page_id);
}
