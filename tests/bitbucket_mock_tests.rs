use atlassian_cli::bitbucket::{Bitbucket, CommentPrArgs, DiffPrArgs};
use atlassian_cli::http::HttpClient;
use atlassian_cli::module::WritePolicy;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_mock_bitbucket_diff_stat() {
    let mock_server = MockServer::start().await;

    let changes_raw = json!({
        "values": [
            {
                "path": { "toString": "src/lib.rs" },
                "type": "MODIFY",
                "percentUnchanged": 85
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/rest/api/1.0/projects/PROJ/repos/repo/pull-requests/42/changes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(changes_raw))
        .mount(&mock_server)
        .await;

    let http = HttpClient::new(mock_server.uri(), "token", false).unwrap();
    let bitbucket = Bitbucket::new(http, WritePolicy { dry_run: false, confirm: true });

    let args = DiffPrArgs {
        project: Some("PROJ".to_string()),
        repo: Some("repo".to_string()),
        id_or_url: "42".to_string(),
        stat: true,
        file: None,
        max_lines: 100,
        offset: 0,
    };

    let res = bitbucket.get_pr_diff(&args).await.unwrap();
    assert_eq!(res["stat_only"], true);
    assert_eq!(res["changed_files_count"], 1);
    assert_eq!(res["files"][0]["path"], "src/lib.rs");
    assert_eq!(res["files"][0]["type"], "MODIFY");
}

#[tokio::test]
async fn test_mock_bitbucket_add_line_comment() {
    let mock_server = MockServer::start().await;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let pr_id = format!("{}", nanos % 1000000);
    let comments_path = format!("/rest/api/1.0/projects/PROJ/repos/repo/pull-requests/{}/comments", pr_id);

    Mock::given(method("POST"))
        .and(path(comments_path))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": 8888,
            "text": "Please add unit test here",
            "author": { "displayName": "Reviewer" },
            "commentAnchor": {
                "path": "src/main.rs",
                "line": 42,
                "lineType": "ADDED",
                "fileType": "TO"
            }
        })))
        .mount(&mock_server)
        .await;

    let http = HttpClient::new(mock_server.uri(), "token", false).unwrap();
    let bitbucket = Bitbucket::new(http, WritePolicy { dry_run: false, confirm: true });

    let args = CommentPrArgs {
        project: Some("PROJ".to_string()),
        repo: Some("repo".to_string()),
        id_or_url: pr_id,
        body_pos: Some("Please add unit test here".to_string()),
        body: None,
        file: Some("src/main.rs".to_string()),
        line: Some(42),
        line_type: "ADDED".to_string(),
        file_type: "TO".to_string(),
    };

    let res = bitbucket.add_pr_comment(&args).await.unwrap();
    assert_eq!(res["status"], "success");
    assert_eq!(res["comment_id"], 8888);
    assert_eq!(res["anchor"]["file_path"], "src/main.rs");
    assert_eq!(res["anchor"]["line"], 42);
    assert_eq!(res["anchor"]["line_type"], "ADDED");
}
