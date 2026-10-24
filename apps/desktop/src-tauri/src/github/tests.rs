//! GitHub integration tests against a local mock server (mockito). Tests that
//! store a token use the real OS keychain under a unique test service name
//! (`credentials::tests::TestKeychain`) and skip with a printed reason where
//! no keychain exists. No request ever reaches github.com.

use std::sync::atomic::AtomicUsize;

use mockito::{Matcher, Server, ServerGuard};

use super::*;
use crate::credentials::tests::TestKeychain;

const TOKEN: &str = "ghp_MockOnlyToken0123456789abcdefghijklmn";

fn config(server: &ServerGuard, client_id: Option<&str>) -> GithubConfig {
    GithubConfig {
        host: client::validate_base(&server.url(), "host").unwrap(),
        api_base: client::validate_base(&server.url(), "api").unwrap(),
        oauth_client_id: client_id.map(str::to_string),
        poll_unit: Duration::from_millis(1),
    }
}

async fn mock_user(server: &mut ServerGuard, scopes: Option<&str>) {
    let mut m = server
        .mock("GET", "/user")
        .match_header("authorization", format!("Bearer {TOKEN}").as_str())
        .match_header("x-github-api-version", client::API_VERSION)
        .match_header("user-agent", Matcher::Regex("^ShiftOps/".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_header("x-ratelimit-limit", "5000")
        .with_header("x-ratelimit-remaining", "4990")
        .with_header("x-ratelimit-reset", "1793000000")
        .with_body(r#"{"id": 42, "login": "octo", "name": "Octo Cat", "html_url": "https://github.com/octo"}"#);
    if let Some(s) = scopes {
        m = m.with_header("x-oauth-scopes", s);
    }
    m.create_async().await;
    server
        .mock("GET", "/user/orgs")
        .match_query(Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"[{"id": 7, "login": "acme", "description": "Acme Inc"}]"#)
        .create_async()
        .await;
}

#[derive(Default)]
struct Events(Mutex<Vec<DeviceFlowEvent>>);
impl AuthSink for Events {
    fn emit(&self, event: &DeviceFlowEvent) {
        self.0.lock().unwrap().push(event.clone());
    }
}

#[tokio::test]
async fn pat_connects_stores_only_in_keychain_and_records_metadata() {
    let Some(kc) = TestKeychain::new() else {
        return;
    };
    let mut server = Server::new_async().await;
    mock_user(&mut server, None).await;
    let db = Database::open_in_memory();
    let cfg = config(&server, None);

    let account = connect_pat(&cfg, &db, &kc.store, Secret::new(TOKEN.into()))
        .await
        .unwrap();
    assert_eq!(
        (account.login.as_str(), account.github_user_id),
        ("octo", 42)
    );
    assert_eq!(account.auth_method, AuthMethod::PersonalAccessToken);
    assert!(
        !account.scopes_reported,
        "fine-grained tokens report no scopes"
    );
    assert_eq!(account.rate_limit.as_ref().unwrap()["remaining"], 4990);
    assert_eq!(
        db.read(|c| store::organizations(c, &account.id)).unwrap()[0].login,
        "acme"
    );
    assert!(kc
        .store
        .exists(Provider::Github, CredentialKind::GithubPersonalAccessToken)
        .unwrap());

    let st = status(&cfg, &db, &kc.store).unwrap();
    assert!(st.credential_stored && !st.oauth_configured);
    let everything = format!(
        "{}{}",
        serde_json::to_string(&st).unwrap(),
        serde_json::to_string(&db.read(|c| activity::list(c, &Default::default())).unwrap())
            .unwrap()
    );
    assert!(!everything.contains("MockOnlyToken"));

    // Disconnect removes the keychain entry and marks the account.
    let gone = disconnect(&db, &kc.store).unwrap().unwrap();
    assert_eq!(gone.status, AccountStatus::Disconnected);
    assert!(!kc
        .store
        .exists(Provider::Github, CredentialKind::GithubPersonalAccessToken)
        .unwrap());
    assert!(db.read(store::active).unwrap().is_none());
}

#[tokio::test]
async fn rejected_pat_stores_nothing() {
    let Some(kc) = TestKeychain::new() else {
        return;
    };
    let mut server = Server::new_async().await;
    server
        .mock("GET", "/user")
        .with_status(401)
        .with_body(r#"{"message":"Bad credentials"}"#)
        .create_async()
        .await;
    let db = Database::open_in_memory();
    let err = connect_pat(
        &config(&server, None),
        &db,
        &kc.store,
        Secret::new(TOKEN.into()),
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::GithubAuthenticationFailed);
    assert!(!kc
        .store
        .exists(Provider::Github, CredentialKind::GithubPersonalAccessToken)
        .unwrap());
    assert!(db.read(store::active).unwrap().is_none());
    let bad = connect_pat(
        &config(&server, None),
        &db,
        &kc.store,
        Secret::new("two words".into()),
    )
    .await
    .unwrap_err();
    assert_eq!(bad.code, ErrorCode::ValidationFailed);
}

#[tokio::test]
async fn device_flow_handles_pending_slow_down_then_success() {
    let Some(kc) = TestKeychain::new() else {
        return;
    };
    let mut server = Server::new_async().await;
    server
        .mock("POST", "/login/device/code")
        .match_body(Matcher::AllOf(vec![
            Matcher::UrlEncoded("client_id".into(), "Iv1.test".into()),
            Matcher::UrlEncoded("scope".into(), OAUTH_SCOPES.into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"device_code":"dev-secret-code","user_code":"ABCD-1234","verification_uri":"https://github.com/login/device","expires_in":900,"interval":1}"#)
        .create_async()
        .await;
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    server
        .mock("POST", "/login/oauth/access_token")
        .match_body(Matcher::UrlEncoded(
            "device_code".into(),
            "dev-secret-code".into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body_from_request(move |_| match counter.fetch_add(1, Ordering::SeqCst) {
            0 => r#"{"error":"authorization_pending"}"#.into(),
            1 => r#"{"error":"slow_down","interval":3}"#.into(),
            _ => format!(
                r#"{{"access_token":"{TOKEN}","token_type":"bearer","scope":"repo,read:org"}}"#
            )
            .into(),
        })
        .create_async()
        .await;
    mock_user(&mut server, Some("repo, read:org")).await;

    let db = Arc::new(Database::open_in_memory());
    let events = Arc::new(Events::default());
    let flows = DeviceFlows::default();
    let (start, handle) = flows
        .start(
            config(&server, Some("Iv1.test")),
            db.clone(),
            kc.store.clone(),
            events.clone(),
        )
        .await
        .unwrap();
    assert_eq!(start.user_code, "ABCD-1234");
    assert!(
        !serde_json::to_string(&start)
            .unwrap()
            .contains("dev-secret-code"),
        "device code never leaves Rust"
    );
    assert_eq!(handle.await.unwrap(), DeviceFlowState::Succeeded);

    let states: Vec<_> = events
        .0
        .lock()
        .unwrap()
        .iter()
        .map(|e| (e.state, e.interval_seconds))
        .collect();
    assert_eq!(
        states,
        [
            (DeviceFlowState::Pending, 1),
            (DeviceFlowState::SlowDown, 3),
            (DeviceFlowState::Succeeded, 3)
        ]
    );
    let account = db.read(store::active).unwrap().unwrap();
    assert_eq!(account.auth_method, AuthMethod::OauthDevice);
    assert_eq!(account.scopes, json!(["repo", "read:org"]));
    assert!(kc
        .store
        .exists(Provider::Github, CredentialKind::GithubOauthToken)
        .unwrap());
}

async fn device_flow_ending_with(body: &'static str) -> (DeviceFlowState, Vec<DeviceFlowEvent>) {
    let mut server = Server::new_async().await;
    server
        .mock("POST", "/login/device/code")
        .with_status(200)
        .with_body(r#"{"device_code":"d","user_code":"U","verification_uri":"https://github.com/login/device","expires_in":900,"interval":1}"#)
        .create_async()
        .await;
    server
        .mock("POST", "/login/oauth/access_token")
        .with_status(200)
        .with_body(body)
        .create_async()
        .await;
    let events = Arc::new(Events::default());
    let (_, handle) = DeviceFlows::default()
        .start(
            config(&server, Some("Iv1.test")),
            Arc::new(Database::open_in_memory()),
            CredentialStore::new("dev.shiftops.desktop.test.unused"),
            events.clone(),
        )
        .await
        .unwrap();
    let state = handle.await.unwrap();
    let ev = events.0.lock().unwrap().clone();
    (state, ev)
}

#[tokio::test]
async fn device_flow_denied_expired_and_invalid_code() {
    let (s, ev) = device_flow_ending_with(r#"{"error":"access_denied"}"#).await;
    assert_eq!(s, DeviceFlowState::Denied);
    assert_eq!(
        ev.last().unwrap().error.as_ref().unwrap().code,
        ErrorCode::GithubAuthorizationDenied
    );
    let (s, ev) = device_flow_ending_with(r#"{"error":"expired_token"}"#).await;
    assert_eq!(s, DeviceFlowState::Expired);
    assert_eq!(
        ev.last().unwrap().error.as_ref().unwrap().code,
        ErrorCode::GithubAuthorizationExpired
    );
    let (s, ev) = device_flow_ending_with(r#"{"error":"incorrect_device_code","error_description":"The device_code provided is not valid."}"#).await;
    assert_eq!(s, DeviceFlowState::Failed);
    assert!(ev
        .last()
        .unwrap()
        .error
        .as_ref()
        .unwrap()
        .technical_details
        .as_ref()
        .unwrap()
        .contains("not valid"));
}

#[tokio::test]
async fn device_flow_cancellation_and_configuration_errors() {
    let mut server = Server::new_async().await;
    server
        .mock("POST", "/login/device/code")
        .with_status(200)
        .with_body(r#"{"device_code":"d","user_code":"U","verification_uri":"https://github.com/login/device","expires_in":900,"interval":1000}"#)
        .create_async()
        .await;
    let flows = DeviceFlows::default();
    let events = Arc::new(Events::default());
    let (start, handle) = flows
        .start(
            config(&server, Some("Iv1.test")),
            Arc::new(Database::open_in_memory()),
            CredentialStore::new("dev.shiftops.desktop.test.unused"),
            events.clone(),
        )
        .await
        .unwrap();
    assert!(flows.cancel(&start.flow_id));
    assert_eq!(handle.await.unwrap(), DeviceFlowState::Cancelled);
    assert!(!flows.cancel(&start.flow_id), "repeated cancel is harmless");

    let err = DeviceFlows::default()
        .start(
            config(&server, None),
            Arc::new(Database::open_in_memory()),
            CredentialStore::new("x"),
            events.clone(),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::GithubNotConfigured);

    let mut refusing = Server::new_async().await;
    refusing
        .mock("POST", "/login/device/code")
        .with_status(400)
        .with_body(r#"{"error":"device_flow_disabled","error_description":"Device Flow must be explicitly enabled for this App"}"#)
        .create_async()
        .await;
    let err = DeviceFlows::default()
        .start(
            config(&refusing, Some("Iv1.test")),
            Arc::new(Database::open_in_memory()),
            CredentialStore::new("x"),
            events,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::GithubNotConfigured);
    assert!(err
        .technical_details
        .unwrap()
        .contains("Device Flow must be explicitly enabled"));
}

#[tokio::test]
async fn http_errors_map_to_structured_codes() {
    let mut server = Server::new_async().await;
    let cfg = config(&server, None);
    let client = GithubClient::new(&cfg, Secret::new(TOKEN.into())).unwrap();
    server
        .mock("GET", "/primary")
        .with_status(403)
        .with_header("x-ratelimit-remaining", "0")
        .with_header("x-ratelimit-reset", "1793000000")
        .with_body(r#"{"message":"API rate limit exceeded"}"#)
        .create_async()
        .await;
    server
        .mock("GET", "/secondary")
        .with_status(403)
        .with_header("retry-after", "60")
        .with_body(r#"{"message":"You have exceeded a secondary rate limit"}"#)
        .create_async()
        .await;
    server
        .mock("GET", "/forbidden")
        .with_status(403)
        .with_body(r#"{"message":"Resource not accessible by personal access token"}"#)
        .create_async()
        .await;
    server
        .mock("GET", "/missing")
        .with_status(404)
        .with_body(r#"{"message":"Not Found"}"#)
        .create_async()
        .await;
    server
        .mock("GET", "/boom")
        .with_status(502)
        .with_body("bad gateway")
        .create_async()
        .await;
    server
        .mock("GET", "/shape")
        .with_status(200)
        .with_body(r#"{"login": 5}"#)
        .create_async()
        .await;

    let primary = client.get::<Value>("primary", &[]).await.unwrap_err();
    assert_eq!(primary.code, ErrorCode::GithubRateLimited);
    assert_eq!(primary.context.as_ref().unwrap()["secondary"], false);
    assert_eq!(primary.context.as_ref().unwrap()["resetAt"], 1793000000u64);
    let secondary = client.get::<Value>("secondary", &[]).await.unwrap_err();
    assert_eq!(secondary.context.as_ref().unwrap()["retryAfterSeconds"], 60);
    assert_eq!(
        client
            .get::<Value>("forbidden", &[])
            .await
            .unwrap_err()
            .code,
        ErrorCode::GithubPermissionDenied
    );
    assert_eq!(
        client.get::<Value>("missing", &[]).await.unwrap_err().code,
        ErrorCode::RecordNotFound
    );
    assert_eq!(
        client.get::<Value>("boom", &[]).await.unwrap_err().code,
        ErrorCode::NetworkFailed
    );
    let shape = client.get::<client::User>("shape", &[]).await.unwrap_err();
    assert_eq!(
        shape.code,
        ErrorCode::InternalError,
        "responses are validated, never trusted"
    );

    // Unreachable server: a network error, not fake data.
    let dead = GithubConfig {
        api_base: client::validate_base("http://127.0.0.1:9", "api").unwrap(),
        ..cfg
    };
    let err = GithubClient::new(&dead, Secret::new(TOKEN.into()))
        .unwrap()
        .get::<Value>("user", &[])
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NetworkFailed);
    assert!(!serde_json::to_string(&err)
        .unwrap()
        .contains("MockOnlyToken"));
}

async fn connected(server: &mut ServerGuard, kc: &TestKeychain) -> (GithubConfig, Database) {
    mock_user(server, Some("repo, read:org")).await;
    let db = Database::open_in_memory();
    let cfg = config(server, None);
    connect_pat(&cfg, &db, &kc.store, Secret::new(TOKEN.into()))
        .await
        .unwrap();
    (cfg, db)
}

fn repo_json(id: i64, owner: &str, name: &str, archived: bool) -> String {
    format!(
        r#"{{"id":{id},"name":"{name}","full_name":"{owner}/{name}","owner":{{"login":"{owner}"}},"private":false,"archived":{archived},"fork":false,"default_branch":"main","html_url":"https://github.com/{owner}/{name}","clone_url":"https://github.com/{owner}/{name}.git","ssh_url":"git@github.com:{owner}/{name}.git","description":null,"language":"TypeScript","pushed_at":"2026-10-20T10:00:00Z","updated_at":"2026-10-20T10:00:00Z","permissions":{{"pull":true,"push":false}}}}"#
    )
}

#[tokio::test]
async fn browse_paginates_filters_searches_and_imports() {
    let Some(kc) = TestKeychain::new() else {
        return;
    };
    let mut server = Server::new_async().await;
    let (cfg, db) = connected(&mut server, &kc).await;
    server
        .mock("GET", "/orgs/acme/repos")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "1".into()),
            Matcher::UrlEncoded("per_page".into(), "2".into()),
            Matcher::UrlEncoded("sort".into(), "updated".into()),
        ]))
        .with_status(200)
        .with_header("link", r#"<http://x/orgs/acme/repos?page=2>; rel="next", <http://x/orgs/acme/repos?page=5>; rel="last""#)
        .with_body(format!("[{},{}]", repo_json(1, "acme", "api", false), repo_json(2, "acme", "old", true)))
        .create_async()
        .await;
    let page = browse(
        &cfg,
        &db,
        &kc.store,
        &BrowseParams {
            owner: Some("acme".into()),
            per_page: Some(2),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(page.has_next_page);
    assert_eq!(
        page.items
            .iter()
            .map(|r| r.full_name.as_str())
            .collect::<Vec<_>>(),
        ["acme/api"],
        "archived hidden by default"
    );

    server
        .mock("GET", "/search/repositories")
        .match_query(Matcher::UrlEncoded(
            "q".into(),
            "api in:name fork:true org:acme archived:false".into(),
        ))
        .with_status(200)
        .with_body(format!(
            r#"{{"total_count":1,"items":[{}]}}"#,
            repo_json(1, "acme", "api", false)
        ))
        .create_async()
        .await;
    let found = browse(
        &cfg,
        &db,
        &kc.store,
        &BrowseParams {
            owner: Some("acme".into()),
            query: Some("api".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(found.total_count, Some(1));
    assert!(browse(
        &cfg,
        &db,
        &kc.store,
        &BrowseParams {
            owner: Some("../evil".into()),
            ..Default::default()
        }
    )
    .await
    .is_err());

    let (repo, created) = import(&db, &found.items[0].id).unwrap();
    assert!(created);
    assert_eq!(
        (repo.github_owner.as_deref(), repo.local_path.as_deref()),
        (Some("acme"), None)
    );
    assert_eq!(
        repo.trust_status,
        crate::db::repositories::TrustStatus::Untrusted
    );
}

#[tokio::test]
async fn pull_requests_with_checks_and_single_pull_mergeability() {
    let Some(kc) = TestKeychain::new() else {
        return;
    };
    let mut server = Server::new_async().await;
    let (cfg, db) = connected(&mut server, &kc).await;
    server
        .mock("GET", "/repos/acme/api")
        .with_status(200)
        .with_body(repo_json(1, "acme", "api", false))
        .create_async()
        .await;
    let pull = r#"{"id":900,"number":12,"title":"Upgrade Node","state":"open","draft":false,"merged_at":null,"user":{"login":"octo"},"html_url":"https://github.com/acme/api/pull/12","head":{"ref":"shiftops/node-20","sha":"abc123","repo":{"full_name":"acme/api"}},"base":{"ref":"main","sha":"def456","repo":{"full_name":"acme/api"}},"mergeable":true,"mergeable_state":"clean","created_at":"2026-10-20T10:00:00Z","updated_at":"2026-10-21T10:00:00Z"}"#;
    server
        .mock("GET", "/repos/acme/api/pulls")
        .match_query(Matcher::Any)
        .with_status(200)
        .with_body(format!("[{pull}]"))
        .create_async()
        .await;
    server
        .mock("GET", "/repos/acme/api/pulls/12")
        .with_status(200)
        .with_body(pull)
        .create_async()
        .await;
    server
        .mock("GET", "/repos/acme/api/commits/abc123/check-runs")
        .match_query(Matcher::Any)
        .with_status(200)
        .with_body(r#"{"total_count":2,"check_runs":[{"status":"completed","conclusion":"success"},{"status":"queued","conclusion":null}]}"#)
        .create_async()
        .await;
    let prs = pull_requests(&cfg, &db, &kc.store, "acme", "api")
        .await
        .unwrap();
    assert_eq!(prs.len(), 1);
    assert_eq!(prs[0].head_ref, "shiftops/node-20");
    assert_eq!(prs[0].checks.as_ref().unwrap()["pending"], 1);
    let one = pull_request(&cfg, &db, &kc.store, "acme", "api", 12)
        .await
        .unwrap();
    assert_eq!(
        (one.mergeable, one.mergeable_state.as_deref()),
        (Some(true), Some("clean"))
    );
}

#[tokio::test]
async fn config_files_are_decoded_redacted_and_secrets_refused() {
    let Some(kc) = TestKeychain::new() else {
        return;
    };
    let mut server = Server::new_async().await;
    let (cfg, db) = connected(&mut server, &kc).await;
    let content = base64::engine::general_purpose::STANDARD
        .encode(r#"{"engines":{"node":">=20"},"token":"supersecret123"}"#);
    server
        .mock("GET", "/repos/acme/api/contents/package.json")
        .match_query(Matcher::UrlEncoded("ref".into(), "main".into()))
        .with_status(200)
        .with_body(format!(r#"{{"type":"file","encoding":"base64","content":"{content}","size":60,"path":"package.json","sha":"s"}}"#))
        .create_async()
        .await;
    let f = file(
        &cfg,
        &db,
        &kc.store,
        "acme",
        "api",
        "package.json",
        Some("main"),
    )
    .await
    .unwrap();
    assert!(f.text.contains(">=20"));
    assert!(!f.text.contains("supersecret123"));
    let err = file(&cfg, &db, &kc.store, "acme", "api", "config/.env", None)
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::PathNotAllowed);
    assert!(
        file(&cfg, &db, &kc.store, "acme", "api", "../etc/passwd", None)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn unauthorized_calls_mark_the_account_invalid() {
    let Some(kc) = TestKeychain::new() else {
        return;
    };
    let mut server = Server::new_async().await;
    let (cfg, db) = connected(&mut server, &kc).await;
    server
        .mock("GET", "/rate_limit")
        .with_status(401)
        .with_body(r#"{"message":"Bad credentials"}"#)
        .create_async()
        .await;
    let err = rate_limit(&cfg, &db, &kc.store).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::GithubAuthenticationFailed);
    assert_eq!(
        db.read(store::active).unwrap().unwrap().status,
        AccountStatus::Invalid
    );
}

#[tokio::test]
async fn requests_can_be_cancelled() {
    let requests = Requests::default();
    let r2 = requests.clone();
    let task = tokio::spawn(async move {
        r2.run(Some("req-1".into()), async {
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok::<_, AppError>(())
        })
        .await
    });
    for _ in 0..100 {
        if requests.cancel("req-1") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(
        task.await.unwrap().unwrap_err().code,
        ErrorCode::CommandCancelled
    );
    assert!(!requests.cancel("req-1"));
}

#[test]
fn not_connected_is_a_structured_error() {
    let db = Database::open_in_memory();
    let cfg = GithubConfig {
        host: client::validate_base("https://github.com", "h").unwrap(),
        api_base: client::validate_base("https://api.github.com", "a").unwrap(),
        oauth_client_id: None,
        poll_unit: Duration::from_secs(1),
    };
    let err = active_client(&cfg, &db, &CredentialStore::new("x"))
        .err()
        .unwrap();
    assert_eq!(err.code, ErrorCode::GithubAuthenticationFailed);
}
