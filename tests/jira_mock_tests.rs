use atlassian_cli::http::HttpClient;
use atlassian_cli::jira::{GetIssueArgs, Jira};
use atlassian_cli::module::WritePolicy;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_mock_jira_get_issue_slim() {
    let mock_server = MockServer::start().await;

    let jira_raw_issue = json!({
        "key": "PROJ-101",
        "fields": {
            "summary": "Fix login crash on startup",
            "description": "App crashes when token is empty",
            "status": { "name": "In Progress" },
            "issuetype": { "name": "Bug" },
            "priority": { "name": "High" },
            "labels": ["ios", "crash"],
            "timetracking": {},
            "assignee": {
                "name": "zhangsan",
                "displayName": "Zhang San"
            },
            "reporter": {
                "name": "lisi",
                "displayName": "Li Si"
            },
            "comment": {
                "comments": [
                    {
                        "id": "1001",
                        "author": { "name": "zhangsan", "displayName": "Zhang San" },
                        "created": "2026-08-18T10:00:00.000+0000",
                        "body": "Investigating the trace..."
                    }
                ]
            }
        }
    });

    Mock::given(method("GET"))
        .and(path("/rest/api/2/issue/PROJ-101"))
        .respond_with(ResponseTemplate::new(200).set_body_json(jira_raw_issue))
        .mount(&mock_server)
        .await;

    let http = HttpClient::new(mock_server.uri(), "token", false).unwrap();
    let jira = Jira::new(http, WritePolicy { dry_run: false, confirm: true });

    let args = GetIssueArgs {
        key: "PROJ-101".to_string(),
        raw: false,
        fields: None,
        comments_limit: 5,
    };

    let res = jira.get_issue(&args).await.unwrap();
    assert_eq!(res["key"], "PROJ-101");
    assert_eq!(res["summary"], "Fix login crash on startup");
    assert_eq!(res["status"], "In Progress");
    assert_eq!(res["assignee"]["mention_syntax"], "[~zhangsan]");
    assert_eq!(res["comments_count"], 1);
    assert_eq!(res["comments"][0]["author"]["mention_syntax"], "[~zhangsan]");
}

#[tokio::test]
async fn test_mock_jira_add_comment() {
    let mock_server = MockServer::start().await;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let key = format!("PROJ-{}", nanos % 1000000);
    let path_str = format!("/rest/api/2/issue/{}/comment", key);

    Mock::given(method("POST"))
        .and(path(path_str))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "9999",
            "body": "Fixed in PR #42",
            "author": { "displayName": "Zhang San" },
            "created": "2026-08-18T12:00:00.000+0000"
        })))
        .mount(&mock_server)
        .await;

    let http = HttpClient::new(mock_server.uri(), "token", false).unwrap();
    let jira = Jira::new(http, WritePolicy { dry_run: false, confirm: true });

    let res = jira.add_comment(&key, "Fixed in PR #42").await.unwrap();
    assert_eq!(res["status"], "success");
    assert_eq!(res["comment_id"], "9999");
    assert_eq!(res["issue"], key);
}

#[tokio::test]
async fn test_mock_jira_transition() {
    let mock_server = MockServer::start().await;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let key = format!("PROJ-{}", (nanos + 1) % 1000000);
    let trans_path = format!("/rest/api/2/issue/{}/transitions", key);

    // 1. 查询可用 transitions
    Mock::given(method("GET"))
        .and(path(trans_path.clone()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "transitions": [
                { "id": "11", "name": "Start Progress", "to": { "name": "In Progress" } },
                { "id": "21", "name": "Done", "to": { "name": "Closed" } }
            ]
        })))
        .mount(&mock_server)
        .await;

    // 2. 执行 transition
    Mock::given(method("POST"))
        .and(path(trans_path))
        .respond_with(ResponseTemplate::new(204))
        .mount(&mock_server)
        .await;

    let http = HttpClient::new(mock_server.uri(), "token", false).unwrap();
    let jira = Jira::new(http, WritePolicy { dry_run: false, confirm: true });

    let res = jira.transition(&key, "Start Progress").await.unwrap();
    assert_eq!(res["status"], "success");
    assert_eq!(res["issue"], key);
    assert_eq!(res["new_status"], "Start Progress");
}

#[tokio::test]
async fn test_mock_jira_assign_unassigned() {
    let mock_server = MockServer::start().await;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let key1 = format!("PROJ-{}", nanos % 1000000);
    let key2 = format!("PROJ-{}", (nanos + 1) % 1000000);

    Mock::given(method("PUT"))
        .and(path(format!("/rest/api/2/issue/{}/assignee", key1)))
        .respond_with(ResponseTemplate::new(204))
        .mount(&mock_server)
        .await;

    Mock::given(method("PUT"))
        .and(path(format!("/rest/api/2/issue/{}/assignee", key2)))
        .respond_with(ResponseTemplate::new(204))
        .mount(&mock_server)
        .await;

    let http = HttpClient::new(mock_server.uri(), "token", false).unwrap();
    let jira = Jira::new(http, WritePolicy { dry_run: false, confirm: true });

    let res = jira.assign_issue(&key1, "unassigned").await.unwrap();
    assert_eq!(res["status"], "success");
    assert_eq!(res["assignee"], "unassigned");

    let res_none = jira.assign_issue(&key2, "none").await.unwrap();
    assert_eq!(res_none["status"], "success");
    assert_eq!(res_none["assignee"], "unassigned");
}

#[tokio::test]
async fn test_mock_jira_search_default() {
    use wiremock::matchers::query_param;

    let mock_server = MockServer::start().await;

    let search_raw = json!({
        "total": 1,
        "startAt": 0,
        "issues": [
            {
                "key": "PROJ-202",
                "fields": {
                    "summary": "Implement feature X",
                    "status": { "name": "In Progress" },
                    "priority": { "name": "Medium" },
                    "issuetype": { "name": "Task" },
                    "assignee": { "name": "current.user", "displayName": "Current User" },
                    "reporter": { "name": "lead", "displayName": "Lead" },
                    "labels": ["backend"]
                }
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/rest/api/2/search"))
        .and(query_param("jql", "assignee = currentUser() AND resolution = Unresolved ORDER BY updated DESC"))
        .respond_with(ResponseTemplate::new(200).set_body_json(search_raw))
        .mount(&mock_server)
        .await;

    let http = HttpClient::new(mock_server.uri(), "token", false).unwrap();
    let jira = Jira::new(http, WritePolicy { dry_run: false, confirm: true });

    let default_jql = "assignee = currentUser() AND resolution = Unresolved ORDER BY updated DESC";
    let res = jira.search_issues(default_jql, 10, None, 0).await.unwrap();
    assert_eq!(res["total"], 1);
    assert_eq!(res["count"], 1);
    assert_eq!(res["issues"][0]["key"], "PROJ-202");
    assert_eq!(res["issues"][0]["summary"], "Implement feature X");
}

