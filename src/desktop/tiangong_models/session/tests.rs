use super::*;

fn source() -> String {
    format!(
        r#"const url = "{ENDPOINT}";
function builtinModelRequestHeaders(model, userId) {{
  if (!isBuiltinModel(model)) return {{}};
  return {{
    "authorization-cer-pat": "fixture-installed-authorization",
    "X-Tiangong-End-User-Source": "tiangong-prod",
    ...userId ? {{ "X-Tiangong-End-User-Id": userId }} : {{}}
  }};
}}
"#
    )
}
fn archive(path: &Path, source: &str) {
    let package = br#"{"name":"tiangong-desktop","version":"1.0.4"}"#;
    let tree = json!({"files":{"package.json":{"offset":"0","size":package.len()},"out":{"files":{"main":{"files":{"index.js":{"offset":package.len().to_string(),"size":source.len()}}}}}}});
    let json = serde_json::to_vec(&tree).unwrap();
    let mut bytes = Vec::new();
    for n in [
        4u32,
        (json.len() + 8) as u32,
        (json.len() + 4) as u32,
        json.len() as u32,
    ] {
        bytes.extend(n.to_le_bytes());
    }
    bytes.extend(json);
    bytes.extend(package);
    bytes.extend(source.as_bytes());
    std::fs::write(path, bytes).unwrap();
}
#[test]
fn installed_adapter_reads_only_the_fixed_function_and_rejects_modified_contracts() {
    assert_eq!(
        parse_installed_headers(&source()).unwrap(),
        "fixture-installed-authorization"
    );
    for changed in [
        source().replace(ENDPOINT, "https://example.invalid"),
        source().replace("isBuiltinModel(model)", "anything(model)"),
        source().replace("tiangong-prod", "other-source"),
        source().replace("X-Tiangong-End-User-Id", "other-user"),
        format!("{}{}", source(), source()),
        source().replace("fixture-installed-authorization", "bad token"),
    ] {
        let e = parse_installed_headers(&changed).unwrap_err().to_string();
        assert!(!e.contains("fixture-installed-authorization"));
    }
}
#[test]
fn asar_ranges_links_identity_and_size_are_checked_without_executing_javascript() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("app.asar");
    archive(&path, &source());
    assert_eq!(
        installed_headers(&path).unwrap(),
        "fixture-installed-authorization"
    );
    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, &bytes[..bytes.len() - 1]).unwrap();
    assert!(installed_headers(&path).is_err());
    archive(&path, "console.log('must never execute')");
    assert!(installed_headers(&path).is_err());
    std::fs::write(&path, []).unwrap();
    assert!(installed_headers(&path).is_err());
}
#[test]
fn session_read_is_narrow_read_only_and_validates_official_environment() {
    let (_temp, db, _config) = super::super::tests::fixture();
    assert!(local_session(&db).unwrap().is_none());
    let connection = Connection::open(&db).unwrap();
    connection
        .execute_batch("CREATE TABLE app_settings (key TEXT PRIMARY KEY,value_json TEXT);")
        .unwrap();
    let value = json!({"accessToken":"fixture-account-token","userId":"fixture-user","authBaseUrl":AUTH_BASE,"expiresAt":0});
    connection
        .execute(
            "INSERT INTO app_settings VALUES('claw_oauth_session',?1)",
            [value.to_string()],
        )
        .unwrap();
    drop(connection);
    let before = std::fs::read(&db).unwrap();
    let mut session = local_session(&db).unwrap().unwrap();
    valid_session(&session).unwrap();
    assert_eq!(std::fs::read(&db).unwrap(), before);
    session.auth_base_url = "https://example.invalid/console".into();
    assert!(valid_session(&session).is_err());
    session.auth_base_url = AUTH_BASE.into();
    session.expires_at = 1;
    assert!(valid_session(&session).is_err());
    assert!(
        local_session(&db.with_extension("missing"))
            .unwrap()
            .is_none()
    );
    assert!(!db.with_extension("missing").exists());
}

fn login_server(token: &str) -> (String, std::thread::JoinHandle<()>) {
    use rsa::{RsaPrivateKey, pkcs8::EncodePublicKey};
    let private = RsaPrivateKey::new(&mut rand::rngs::OsRng, 1024).unwrap();
    let key = private
        .to_public_key()
        .to_public_key_pem(Default::default())
        .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let token = token.to_string();
    let task = std::thread::spawn(move || {
        for step in 0..3 {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut block = [0u8; 4096];
            loop {
                let n = socket.read(&mut block).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&block[..n]);
                if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                    let length = header
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|s| s.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let request = String::from_utf8(bytes.clone()).unwrap();
            let response = match step {
                0 => {
                    assert!(request.starts_with("GET /anon/framework/api/encrypt/key "));
                    json!({"text":key})
                }
                1 => {
                    assert!(request.starts_with("POST /nvwa/login "));
                    assert!(!request.contains("fixture-password"));
                    let body: Value =
                        serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
                    assert_eq!(body["encryptType"], "3L");
                    use base64::Engine;
                    let ciphertext = base64::engine::general_purpose::STANDARD
                        .decode(body["pwd"].as_str().unwrap())
                        .unwrap();
                    let clear = private.decrypt(rsa::Pkcs1v15Encrypt, &ciphertext).unwrap();
                    assert_eq!(
                        base64::engine::general_purpose::STANDARD
                            .decode(clear)
                            .unwrap(),
                        b"fixture-password"
                    );
                    json!({"code":0,"token":token})
                }
                _ => {
                    assert!(request.starts_with("GET /nvwa/getLoginContext "));
                    assert!(request.contains(&token));
                    json!({"code":0,"context":{"id":"fixture-user","name":"合成账号"}})
                }
            }
            .to_string();
            use std::io::Write;
            write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",response.len(),response).unwrap();
        }
    });
    (base, task)
}
#[test]
fn account_login_encrypts_password_verifies_identity_and_imports_models_atomically() {
    let (temp, db, path) = super::super::tests::fixture();
    let archive_path = temp.path().join("app.asar");
    archive(&archive_path, &source());
    let (base, server) = login_server("fixture-account-token");
    let native_before = std::fs::read(&db).unwrap();
    let revision = AppConfig::load_or_default(&path).unwrap().revision.unwrap();
    let view = connect_at(
        &path,
        &db,
        ConnectRequest {
            revision,
            native: false,
            username: "fixture-account".into(),
            password: "fixture-password".into(),
        },
        Some((&base, &archive_path)),
    )
    .unwrap();
    server.join().unwrap();
    assert_eq!(view["loggedIn"], true);
    assert_eq!(view["accountName"], "合成账号");
    assert_eq!(view["models"].as_array().unwrap().len(), 2);
    assert_eq!(std::fs::read(&db).unwrap(), native_before);
    let config = AppConfig::load_or_default(&path).unwrap();
    assert_eq!(config.ai_gateway.providers.len(), 2);
    let raw = std::fs::read_to_string(&path).unwrap();
    for secret in [
        "fixture-account-token",
        "fixture-password",
        "fixture-installed-authorization",
    ] {
        assert!(!raw.contains(secret));
        assert!(!view.to_string().contains(secret));
    }
    let credential = saved_credential(&path, &config).unwrap();
    assert!(credential.pat.is_empty());
    assert_eq!(credential.session.unwrap().user_id, "fixture-user");
}

#[test]
fn invalid_account_or_unsupported_install_never_overwrites_existing_models() {
    let (temp, db, path) = super::super::tests::fixture();
    let archive_path = temp.path().join("app.asar");
    archive(&archive_path, "unsupported");
    let before = std::fs::read(&path).unwrap();
    let revision = AppConfig::load_or_default(&path).unwrap().revision.unwrap();
    assert!(
        connect_at(
            &path,
            &db,
            ConnectRequest {
                revision,
                native: false,
                username: "fixture-account".into(),
                password: "fixture-password".into()
            },
            Some(("http://127.0.0.1:1", &archive_path))
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

fn context_server(token: &str, context: Value) -> (String, std::thread::JoinHandle<()>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let token = token.to_string();
    let task = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut block = [0u8; 4096];
        while !bytes.windows(4).any(|v| v == b"\r\n\r\n") {
            let n = socket.read(&mut block).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&block[..n]);
        }
        let request = String::from_utf8(bytes).unwrap();
        assert!(request.starts_with("GET /nvwa/getLoginContext "));
        assert!(request.contains(&token));
        use std::io::Write;
        let body = json!({"code":0,"context":context}).to_string();
        write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
    });
    (base, task)
}

#[test]
fn existing_native_login_imports_models_without_password_or_native_database_changes() {
    let (temp, db, path) = super::super::tests::fixture();
    let archive_path = temp.path().join("app.asar");
    archive(&archive_path, &source());
    let native = Connection::open(&db).unwrap();
    native
        .execute_batch("CREATE TABLE app_settings (key TEXT PRIMARY KEY,value_json TEXT);")
        .unwrap();
    let session = json!({"accessToken":"fixture-local-token","userId":"fixture-user","authBaseUrl":AUTH_BASE,"expiresAt":0});
    native
        .execute(
            "INSERT INTO app_settings VALUES('claw_oauth_session',?1)",
            [session.to_string()],
        )
        .unwrap();
    drop(native);
    let before = std::fs::read(&db).unwrap();
    let (base, server) = context_server(
        "fixture-local-token",
        json!({"id":"fixture-user","name":"已有登录"}),
    );
    let revision = AppConfig::load_or_default(&path).unwrap().revision.unwrap();
    let view = connect_at(
        &path,
        &db,
        ConnectRequest {
            revision,
            native: false,
            username: String::new(),
            password: String::new(),
        },
        Some((&base, &archive_path)),
    )
    .unwrap();
    server.join().unwrap();
    assert_eq!(view["loggedIn"], true);
    assert_eq!(view["accountName"], "已有登录");
    assert_eq!(view["models"].as_array().unwrap().len(), 2);
    assert_eq!(std::fs::read(&db).unwrap(), before);
    assert!(
        !std::fs::read_to_string(&path)
            .unwrap()
            .contains("fixture-local-token")
    );
    // A mismatched remote identity must not rotate existing model credentials.
    let config_before = std::fs::read(&path).unwrap();
    let (base, server) =
        context_server("fixture-local-token", json!({"id":"another-fixture-user"}));
    let revision = AppConfig::load_or_default(&path).unwrap().revision.unwrap();
    let error = connect_at(
        &path,
        &db,
        ConnectRequest {
            revision,
            native: false,
            username: String::new(),
            password: String::new(),
        },
        Some((&base, &archive_path)),
    )
    .unwrap_err()
    .to_string();
    server.join().unwrap();
    assert!(error.contains("账号已变化"));
    assert_eq!(std::fs::read(&path).unwrap(), config_before);
    assert_eq!(std::fs::read(&db).unwrap(), before);
}

#[test]
fn native_import_needs_no_account_network_and_retains_only_valid_local_identity() {
    for logged_in in [false, true] {
        let (temp, db, path) = super::super::tests::fixture();
        let archive_path = temp.path().join("app.asar");
        archive(&archive_path, &source());
        let native = Connection::open(&db).unwrap();
        native
            .execute_batch("CREATE TABLE app_settings (key TEXT PRIMARY KEY,value_json TEXT);")
            .unwrap();
        let session = json!({"accessToken":"fixture-local-token","userId":"fixture-user","authBaseUrl":AUTH_BASE,"expiresAt":if logged_in{0}else{1}});
        native
            .execute(
                "INSERT INTO app_settings VALUES('claw_oauth_session',?1)",
                [session.to_string()],
            )
            .unwrap();
        drop(native);
        let before = std::fs::read(&db).unwrap();
        let config = AppConfig::load_or_default(&path).unwrap();
        // A closed loopback port proves importing the native adapter performs
        // no login-context request. No production request is sent by this test.
        let view = connect_at(
            &path,
            &db,
            ConnectRequest {
                revision: config.revision.unwrap(),
                username: String::new(),
                password: String::new(),
                native: true,
            },
            Some(("http://127.0.0.1:1", &archive_path)),
        )
        .unwrap();
        assert_eq!(view["loggedIn"], logged_in);
        assert_eq!(view["parametersCurrent"], true);
        assert_eq!(std::fs::read(&db).unwrap(), before);
        let config = AppConfig::load_or_default(&path).unwrap();
        let credential = saved_credential(&path, &config).unwrap();
        assert_eq!(credential.session.is_some(), logged_in);
        assert_eq!(
            credential.user_id,
            if logged_in { "fixture-user" } else { "" }
        );
        assert!(
            !std::fs::read_to_string(&path)
                .unwrap()
                .contains("fixture-local-token")
        );
    }
}
