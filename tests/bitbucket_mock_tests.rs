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

#[tokio::test]
async fn test_mock_bitbucket_list_prs_dashboard() {
    use atlassian_cli::bitbucket::ListPrsArgs;
    use wiremock::matchers::query_param;

    let mock_server = MockServer::start().await;

    let dashboard_raw = json!({
        "size": 1,
        "limit": 10,
        "isLastPage": true,
        "values": [
            {
                "id": 101,
                "title": "Fix token memory leak",
                "state": "OPEN",
                "author": {
                    "user": {
                        "name": "john.doe",
                        "displayName": "John Doe"
                    }
                },
                "fromRef": {
                    "displayId": "feature/fix-leak",
                    "repository": {
                        "slug": "app-android",
                        "project": { "key": "MOBILE" }
                    }
                },
                "toRef": {
                    "displayId": "main",
                    "repository": {
                        "slug": "app-android",
                        "project": { "key": "MOBILE" }
                    }
                },
                "createdDate": 1700000000000_i64,
                "updatedDate": 1700000005000_i64,
                "links": {
                    "self": [{ "href": "http://127.0.0.1/projects/MOBILE/repos/app-android/pull-requests/101" }]
                }
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/rest/api/1.0/dashboard/pull-requests"))
        .and(query_param("state", "OPEN"))
        .and(query_param("role", "AUTHOR"))
        .and(query_param("limit", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(dashboard_raw))
        .mount(&mock_server)
        .await;

    let http = HttpClient::new(mock_server.uri(), "token", false).unwrap();
    let bitbucket = Bitbucket::new(http, WritePolicy { dry_run: false, confirm: true });

    let args = ListPrsArgs {
        project: None,
        repo: None,
        url: None,
        state: "OPEN".to_string(),
        role: "AUTHOR".to_string(),
        limit: 10,
    };

    let res = bitbucket.list_prs(&args).await.unwrap();
    assert_eq!(res["mode"], "dashboard");
    assert_eq!(res["role"], "AUTHOR");
    assert_eq!(res["count"], 1);
    assert_eq!(res["pull_requests"][0]["id"], 101);
    assert_eq!(res["pull_requests"][0]["project"], "MOBILE");
    assert_eq!(res["pull_requests"][0]["repo"], "app-android");
    assert_eq!(res["pull_requests"][0]["author"]["username"], "john.doe");
}

#[tokio::test]
async fn test_mock_bitbucket_list_prs_repo() {
    use atlassian_cli::bitbucket::ListPrsArgs;
    use wiremock::matchers::query_param;

    let mock_server = MockServer::start().await;

    let repo_prs_raw = json!({
        "values": [
            {
                "id": 55,
                "title": "Upgrade dependencies",
                "state": "OPEN",
                "author": {
                    "user": {
                        "name": "jane.smith",
                        "displayName": "Jane Smith"
                    }
                },
                "fromRef": { "displayId": "chore/deps" },
                "toRef": { "displayId": "main" },
                "createdDate": 1700000000000_i64,
                "updatedDate": 1700000005000_i64,
                "links": {
                    "self": [{ "href": "http://127.0.0.1/projects/CORE/repos/backend/pull-requests/55" }]
                }
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/rest/api/1.0/projects/CORE/repos/backend/pull-requests"))
        .and(query_param("state", "OPEN"))
        .and(query_param("limit", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(repo_prs_raw))
        .mount(&mock_server)
        .await;

    let http = HttpClient::new(mock_server.uri(), "token", false).unwrap();
    let bitbucket = Bitbucket::new(http, WritePolicy { dry_run: false, confirm: true });

    let args = ListPrsArgs {
        project: Some("CORE".to_string()),
        repo: Some("backend".to_string()),
        url: None,
        state: "OPEN".to_string(),
        role: "AUTHOR".to_string(),
        limit: 5,
    };

    let res = bitbucket.list_prs(&args).await.unwrap();
    assert_eq!(res["mode"], "repository");
    assert_eq!(res["project"], "CORE");
    assert_eq!(res["repo"], "backend");
    assert_eq!(res["count"], 1);
    assert_eq!(res["pull_requests"][0]["id"], 55);
    assert_eq!(res["pull_requests"][0]["author"]["username"], "jane.smith");
}

#[tokio::test]
async fn test_mock_bitbucket_create_pr_with_url() {
    use atlassian_cli::bitbucket::CreatePrArgs;

    let mock_server = MockServer::start().await;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let title = format!("Fix auth race condition {}", nanos);
    let from_branch = format!("feature/race-fix-{}", nanos);

    let create_resp = json!({
        "id": 99,
        "title": title,
        "state": "OPEN",
        "fromRef": { "displayId": from_branch },
        "toRef": { "displayId": "main" },
        "reviewers": [
            { "user": { "name": "lead.dev", "displayName": "Lead Dev" } }
        ],
        "links": {
            "self": [{ "href": "http://127.0.0.1/projects/PROJ/repos/app/pull-requests/99" }]
        }
    });

    Mock::given(method("POST"))
        .and(path("/rest/api/1.0/projects/PROJ/repos/app/pull-requests"))
        .respond_with(ResponseTemplate::new(201).set_body_json(create_resp))
        .mount(&mock_server)
        .await;

    let http = HttpClient::new(mock_server.uri(), "token", false).unwrap();
    let bitbucket = Bitbucket::new(http, WritePolicy { dry_run: false, confirm: true });

    let args = CreatePrArgs {
        project: None,
        repo: None,
        url: Some("https://gitpub.example.com/projects/PROJ/repos/app".to_string()),
        title: title.clone(),
        description: "Addresses race condition on login".to_string(),
        from: from_branch,
        to: "main".to_string(),
        reviewers: Some("lead.dev".to_string()),
        no_default_reviewers: true,
    };

    let res = bitbucket.create_pr(&args).await.unwrap();
    assert_eq!(res["status"], "success");
    assert_eq!(res["pr_id"], 99);
    assert_eq!(res["title"], title);
    assert_eq!(res["reviewers_count"], 1);
    assert_eq!(res["reviewers"][0]["username"], "lead.dev");
}


