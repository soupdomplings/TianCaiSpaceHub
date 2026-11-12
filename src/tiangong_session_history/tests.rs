use super::*;
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{fs, io::Write};

struct Fixture {
    _root: tempfile::TempDir,
    database: PathBuf,
    config: PathBuf,
    workspace: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let desktop = root.path().join("desktop");
        fs::create_dir(&desktop).unwrap();
        let database = desktop.join("electron-data.db");
        let config = root.path().join("hub.toml");
        let workspace = desktop.join("workspace");
        fs::create_dir(&workspace).unwrap();
        let db = Connection::open(&database).unwrap();
        // The public 1.0.4 schema, with message payload columns retained to
        // verify that ownership operations never rewrite bodies or attachments.
        db.execute_batch("CREATE TABLE sessions (
            session_id TEXT PRIMARY KEY,title TEXT NOT NULL DEFAULT '',user_id TEXT NOT NULL DEFAULT 'local',
            agent_id TEXT DEFAULT '',project_id TEXT DEFAULT 'default',model_name TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'active',message_count INTEGER NOT NULL DEFAULT 0,
            current_step INTEGER NOT NULL DEFAULT 0,max_steps INTEGER DEFAULT 20,extra_data TEXT DEFAULT '{}',
            error TEXT DEFAULT '',created_at TEXT NOT NULL DEFAULT (datetime('now')),updated_at TEXT NOT NULL DEFAULT (datetime('now')));
          CREATE TABLE session_messages(id INTEGER PRIMARY KEY,session_id TEXT NOT NULL,message_index INTEGER NOT NULL,
            role TEXT NOT NULL,tool_name TEXT DEFAULT '',event_type TEXT DEFAULT '',source TEXT DEFAULT '',created_at TEXT DEFAULT (datetime('now')));
          CREATE TABLE scenarios(scenario_id TEXT PRIMARY KEY,name TEXT NOT NULL,work_dir TEXT NOT NULL DEFAULT '',
            sort_order INTEGER NOT NULL DEFAULT 0,created_at TEXT DEFAULT (datetime('now')),updated_at TEXT DEFAULT (datetime('now')));
          CREATE TABLE tasks(task_id TEXT PRIMARY KEY,scenario_id TEXT NOT NULL,title TEXT DEFAULT '',session_id TEXT NOT NULL DEFAULT '',
            model_id TEXT DEFAULT '',model_options_json TEXT DEFAULT '{}',sort_order INTEGER DEFAULT 0,completed_at TEXT,
            completed_viewed_at TEXT,created_at TEXT DEFAULT (datetime('now')),updated_at TEXT DEFAULT (datetime('now')));
          CREATE TABLE task_messages(id INTEGER PRIMARY KEY,task_id TEXT NOT NULL,role TEXT NOT NULL,content TEXT DEFAULT '',
            thinking_steps TEXT,files_json TEXT,usage_json TEXT,user_error_json TEXT,execution_json TEXT,created_at TEXT DEFAULT (datetime('now')));
          CREATE INDEX task_messages_task ON task_messages(task_id);
          CREATE TABLE compaction_records(id INTEGER PRIMARY KEY,session_id TEXT,boundary_uuid TEXT,summary TEXT);
          CREATE TABLE memory_entries(id INTEGER PRIMARY KEY,user_id TEXT NOT NULL,project_id TEXT NOT NULL,session_id TEXT,
            content TEXT,category TEXT DEFAULT 'general',source TEXT DEFAULT 'realtime',is_merged INTEGER DEFAULT 0);
          CREATE VIRTUAL TABLE memory_index USING fts5(user_id UNINDEXED,project_id UNINDEXED,layer UNINDEXED,slug UNINDEXED,
            title,content,source_session UNINDEXED,created_at UNINDEXED);
          CREATE TABLE memory_sync_state(user_id TEXT,project_id TEXT,layer INTEGER,slug TEXT,md5_hash TEXT,
            updated_at TEXT,PRIMARY KEY(user_id,project_id,layer,slug));
          CREATE TABLE model_configs(model_id TEXT PRIMARY KEY,api_key TEXT);
          INSERT INTO model_configs VALUES('unchanged-model','SYNTHETIC-SECRET-NOT-A-REAL-KEY');
          INSERT INTO scenarios(scenario_id,name) VALUES('default','默认项目');").unwrap();
        Self {
            _root: root,
            database,
            config,
            workspace,
        }
    }

    fn db(&self) -> Connection {
        Connection::open(&self.database).unwrap()
    }

    fn add(&self, session: &str, owner: &str, project: &str, scenario: &str, work: &Path) {
        let task = format!("task-{session}");
        let db = self.db();
        db.execute("INSERT INTO sessions(session_id,title,user_id,agent_id,project_id,model_name,message_count,extra_data)
            VALUES(?1,'历史标题',?2,'desktop-orchestration',?3,'model-original',1,?4)",
            params![session,owner,project,json!({"project":{"id":project,"path":work}}).to_string()]).unwrap();
        db.execute(
            "INSERT INTO tasks(task_id,scenario_id,title,session_id,model_id,model_options_json)
            VALUES(?1,?2,'任务标题',?3,'configured-model-id','{\"temperature\":0.4}')",
            params![task, scenario, session],
        )
        .unwrap();
        db.execute(
            "INSERT INTO session_messages(session_id,message_index,role,event_type,source)
            VALUES(?1,0,'user','text','desktop')",
            [session],
        )
        .unwrap();
        db.execute(
            "INSERT INTO task_messages(task_id,role,content,files_json,execution_json)
            VALUES(?1,'system','BODY-MUST-STAY','[{\"file_path\":\"original/附件.png\"}]',?2)",
            params![
                task,
                json!({"version":1,"status":"completed","blocks":[],"runId":"run-original",
                "turnIndex":0,"lastSeq":1,"finalAnswer":"BODY-MUST-STAY"})
                .to_string()
            ],
        )
        .unwrap();
        db.execute("INSERT INTO compaction_records(session_id,boundary_uuid,summary) VALUES(?1,'boundary','SUMMARY-MUST-STAY')",[session]).unwrap();
        let file = self.file_at(work, session, owner, project);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, b"{\"type\":\"user\",\"uuid\":\"u1\",\"message\":{\"content\":\"JSONL-BODY-MUST-STAY\"}}\n").unwrap();
    }

    fn add_default(&self, session: &str, owner: &str) {
        self.add(session, owner, "0", "default", &self.workspace);
    }

    fn file_at(&self, work: &Path, session: &str, owner: &str, project: &str) -> PathBuf {
        work.join(".tiangong/memory")
            .join(owner)
            .join("memory")
            .join(project)
            .join(format!(
                "session-{}.jsonl",
                session.strip_prefix("session-").unwrap_or(session)
            ))
    }

    fn file(&self, session: &str, owner: &str) -> PathBuf {
        self.file_at(&self.workspace, session, owner, "0")
    }

    fn owner(&self, session: &str) -> String {
        self.db()
            .query_row(
                "SELECT user_id FROM sessions WHERE session_id=?1",
                [session],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn move_one(&self, session: &str, owner: &str) -> Result<MoveResult> {
        move_at(
            &self.database,
            &self.config,
            &MoveRequest {
                sessions: vec![selection(session, owner)],
            },
        )
    }

    fn restore_one(&self, session: &str, owner: &str) -> Result<MoveResult> {
        restore_at(
            &self.database,
            &self.config,
            &RestoreRequest {
                sessions: vec![selection(session, owner)],
            },
        )
    }

    fn links(&self, session: &str, owner: &str) {
        let value = json!({"version":1,"futureRoot":{"keep":true},"links":[{
            "session_id":session,"title":"link title","model_id":"original","pending":false,"uncertain":false,
            "futureLink":[3,4],"context":{"task_id":format!("task-{session}"),"user_id":owner,
                "project":{"id":"0","path":self.workspace,"name":"project"},"model_name":"original",
                "max_steps":20,"owned_reply_ids":[11,12],"futureContext":{"keep":true}}
        }]});
        fs::write(
            self.config.with_extension("gmclaw-sessions.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
    }

    fn read_links(&self) -> Value {
        serde_json::from_slice(
            &fs::read(self.config.with_extension("gmclaw-sessions.json")).unwrap(),
        )
        .unwrap()
    }
}

fn selection(session: &str, user: &str) -> SessionSelection {
    SessionSelection {
        session_id: session.into(),
        user_id: user.into(),
    }
}

fn append(path: &Path, value: Value) {
    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
    writeln!(file, "{value}").unwrap();
}

#[test]
fn actual_schema_lists_task_joins_and_never_invents_an_orphan_owner() {
    let fixture = Fixture::new();
    fixture.add_default("session-alpha", "local");
    let custom = fixture._root.path().join("custom");
    fs::create_dir(&custom).unwrap();
    fixture
        .db()
        .execute(
            "INSERT INTO scenarios(scenario_id,name,work_dir) VALUES('project-b','B',?1)",
            [custom.to_string_lossy().as_ref()],
        )
        .unwrap();
    fixture.add("beta", "account-b", "project-b", "project-b", &custom);
    fixture.db().execute("INSERT INTO tasks(task_id,scenario_id,title,session_id) VALUES('orphan-task','default','未开始','orphan')",[]).unwrap();
    let list = list_at(&fixture.database, &fixture.config).unwrap();
    assert_eq!(list.entries.len(), 3);
    let alpha = list
        .entries
        .iter()
        .find(|row| row.session_id == "session-alpha")
        .unwrap();
    assert_eq!(alpha.project_id, "0");
    assert_eq!(Path::new(&alpha.work_dir), fixture.workspace);
    assert!(alpha.can_move);
    let beta = list
        .entries
        .iter()
        .find(|row| row.session_id == "beta")
        .unwrap();
    assert_eq!(Path::new(&beta.work_dir), custom);
    let orphan = list
        .entries
        .iter()
        .find(|row| row.session_id == "orphan")
        .unwrap();
    assert!(orphan.user_id.is_empty() && orphan.agent_id.is_empty());
    assert!(!orphan.can_move && !orphan.can_restore);
    assert!(fixture.move_one("orphan", "local").is_err());
}

#[test]
fn moving_only_selected_history_preserves_ids_models_messages_attachments_and_images() {
    let fixture = Fixture::new();
    fixture.add_default("session-alpha", "local");
    fixture.add_default("beta", "other-user");
    fixture.links("session-alpha", "local");
    let image = fixture
        .workspace
        .join(".tiangong/memory/images/snapshot.png");
    fs::create_dir_all(image.parent().unwrap()).unwrap();
    fs::write(&image, b"synthetic-image").unwrap();
    append(
        &fixture.file("session-alpha", "local"),
        json!({"type":"user","desktop_images":[{"path":image}],"message":{"content":"image"}}),
    );
    let source = fs::read(fixture.file("session-alpha", "local")).unwrap();
    let result = fixture.move_one("session-alpha", "local").unwrap();
    assert_eq!(result.changed, 1);
    assert_eq!(fixture.owner("session-alpha"), GATEWAY_USER_ID);
    assert_eq!(fixture.owner("beta"), "other-user");
    assert_eq!(
        fs::read(fixture.file("session-alpha", GATEWAY_USER_ID)).unwrap(),
        source
    );
    assert_eq!(
        fs::read(fixture.file("session-alpha", "local")).unwrap(),
        source
    );
    assert_eq!(fs::read(&image).unwrap(), b"synthetic-image");
    assert!(
        !fixture
            .file("session-alpha", GATEWAY_USER_ID)
            .with_file_name("session-session-alpha.jsonl")
            .exists()
    );
    let row: (String,String,String,String) = fixture.db().query_row(
        "SELECT s.agent_id,s.project_id,t.model_id,m.content FROM sessions s JOIN tasks t ON t.session_id=s.session_id
         JOIN task_messages m ON m.task_id=t.task_id WHERE s.session_id='session-alpha'",[],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap();
    assert_eq!(
        row,
        (
            "desktop-orchestration".into(),
            "0".into(),
            "configured-model-id".into(),
            "BODY-MUST-STAY".into()
        )
    );
    let files: String = fixture
        .db()
        .query_row(
            "SELECT files_json FROM task_messages WHERE task_id='task-session-alpha'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(files.contains("附件.png"));
    let record = fs::read_to_string(result.backup_path.unwrap()).unwrap();
    assert!(!record.contains("BODY-MUST-STAY") && !record.contains("SYNTHETIC-SECRET"));
    let link = fixture.read_links();
    assert_eq!(link["links"][0]["context"]["user_id"], GATEWAY_USER_ID);
    assert_eq!(
        link["links"][0]["context"]["owned_reply_ids"],
        json!([11, 12])
    );
    assert_eq!(link["futureRoot"]["keep"], true);
    assert_eq!(link["links"][0]["futureLink"], json!([3, 4]));
    assert_eq!(link["links"][0]["context"]["futureContext"]["keep"], true);
    let list = list_at(&fixture.database, &fixture.config).unwrap();
    let moved = list
        .entries
        .iter()
        .find(|row| row.session_id == "session-alpha")
        .unwrap();
    assert!(
        moved.is_gateway && moved.can_restore && !moved.can_move && moved.migration_id.is_some()
    );
}

#[test]
fn undo_brings_back_new_context_and_retains_new_db_messages_and_compaction() {
    let fixture = Fixture::new();
    fixture.add_default("alpha", "local");
    fixture.links("alpha", "local");
    fixture.move_one("alpha", "local").unwrap();
    append(
        &fixture.file("alpha", GATEWAY_USER_ID),
        json!({"type":"assistant","uuid":"u2","parentUuid":"u1","message":{"content":"new reply"}}),
    );
    append(
        &fixture.file("alpha", GATEWAY_USER_ID),
        json!({"subtype":"runtime_state","runtime_type":"cleared","runtime_data":null}),
    );
    fixture.db().execute("INSERT INTO session_messages(session_id,message_index,role,event_type) VALUES('alpha',1,'assistant','over')",[]).unwrap();
    fixture.db().execute("INSERT INTO compaction_records(session_id,boundary_uuid,summary) VALUES('alpha','new-boundary','new summary')",[]).unwrap();
    let mut link = fixture.read_links();
    link["links"][0]["context"]["owned_reply_ids"] = json!([11, 12, 21]);
    fs::write(
        fixture.config.with_extension("gmclaw-sessions.json"),
        serde_json::to_vec(&link).unwrap(),
    )
    .unwrap();
    let current = fs::read(fixture.file("alpha", GATEWAY_USER_ID)).unwrap();
    let result = fixture.restore_one("alpha", GATEWAY_USER_ID).unwrap();
    assert_eq!(fixture.owner("alpha"), "local");
    assert_eq!(fs::read(fixture.file("alpha", "local")).unwrap(), current);
    assert_eq!(
        fs::read(fixture.file("alpha", GATEWAY_USER_ID)).unwrap(),
        current
    );
    assert!(!result.warnings.is_empty());
    let counts: (i64, i64) = fixture
        .db()
        .query_row(
            "SELECT (SELECT count(*) FROM session_messages WHERE session_id='alpha'),
        (SELECT count(*) FROM compaction_records WHERE session_id='alpha')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(counts, (2, 2));
    let link = fixture.read_links();
    assert_eq!(link["links"][0]["context"]["user_id"], "local");
    assert_eq!(
        link["links"][0]["context"]["owned_reply_ids"],
        json!([11, 12, 21])
    );
    assert!(!list_at(&fixture.database, &fixture.config).unwrap().entries[0].can_restore);
}

#[test]
fn unchanged_created_copy_is_removed_on_undo_and_can_be_migrated_again() {
    let fixture = Fixture::new();
    fixture.add_default("alpha", "local");
    fixture.move_one("alpha", "local").unwrap();
    fixture.restore_one("alpha", GATEWAY_USER_ID).unwrap();
    assert!(!fixture.file("alpha", GATEWAY_USER_ID).exists());
    assert!(fixture.file("alpha", "local").is_file());
    assert!(fixture.move_one("alpha", "local").is_ok());
}

#[test]
fn independently_edited_original_and_gateway_context_refuses_the_entire_undo() {
    let fixture = Fixture::new();
    fixture.add_default("alpha", "local");
    fixture.add_default("beta", "local");
    move_at(
        &fixture.database,
        &fixture.config,
        &MoveRequest {
            sessions: vec![selection("alpha", "local"), selection("beta", "local")],
        },
    )
    .unwrap();
    append(
        &fixture.file("beta", "local"),
        json!({"type":"user","message":{"content":"other original edit"}}),
    );
    append(
        &fixture.file("beta", GATEWAY_USER_ID),
        json!({"type":"user","message":{"content":"new gateway reply"}}),
    );
    let original = fs::read(fixture.file("beta", "local")).unwrap();
    let target = fs::read(fixture.file("beta", GATEWAY_USER_ID)).unwrap();
    assert!(
        restore_at(
            &fixture.database,
            &fixture.config,
            &RestoreRequest {
                sessions: vec![
                    selection("alpha", GATEWAY_USER_ID),
                    selection("beta", GATEWAY_USER_ID)
                ]
            }
        )
        .is_err()
    );
    assert_eq!(fixture.owner("alpha"), GATEWAY_USER_ID);
    assert_eq!(fixture.owner("beta"), GATEWAY_USER_ID);
    assert_eq!(fs::read(fixture.file("beta", "local")).unwrap(), original);
    assert_eq!(
        fs::read(fixture.file("beta", GATEWAY_USER_ID)).unwrap(),
        target
    );
    assert!(fixture.file("alpha", GATEWAY_USER_ID).exists());
}

#[test]
fn session_memory_moves_but_shared_memory_and_other_sessions_stay_in_their_scope() {
    let fixture = Fixture::new();
    fixture.add_default("alpha", "local");
    let db = fixture.db();
    db.execute_batch("INSERT INTO memory_entries VALUES(1,'local','0','alpha','selected memory','general','realtime',0);
        INSERT INTO memory_entries VALUES(2,'local','0','other','other memory','general','realtime',0);
        INSERT INTO memory_index VALUES('local','0',5,'alpha-note','note','selected memory','alpha','now');
        INSERT INTO memory_index VALUES('local','0',1,'main','UserMemory','shared memory','','now');
        INSERT INTO memory_sync_state VALUES('local','0',5,'alpha-note','original-md5','now');").unwrap();
    let scope = fixture
        .file("alpha", "local")
        .parent()
        .unwrap()
        .to_path_buf();
    fs::write(
        scope.join("feedback-alpha-note.md"),
        b"selected memory file",
    )
    .unwrap();
    fs::write(scope.join("MEMORY.md"), b"shared memory file").unwrap();
    fs::write(scope.join("DAY-RECALL.md"), b"shared recall file").unwrap();
    fixture.move_one("alpha", "local").unwrap();
    let target = fixture
        .file("alpha", GATEWAY_USER_ID)
        .parent()
        .unwrap()
        .to_path_buf();
    assert_eq!(
        fs::read(target.join("feedback-alpha-note.md")).unwrap(),
        b"selected memory file"
    );
    assert!(!target.join("MEMORY.md").exists() && !target.join("DAY-RECALL.md").exists());
    assert_eq!(
        db.query_row("SELECT user_id FROM memory_entries WHERE id=1", [], |row| {
            row.get::<_, String>(0)
        })
        .unwrap(),
        GATEWAY_USER_ID
    );
    assert_eq!(
        db.query_row("SELECT user_id FROM memory_entries WHERE id=2", [], |row| {
            row.get::<_, String>(0)
        })
        .unwrap(),
        "local"
    );
    assert_eq!(
        db.query_row(
            "SELECT user_id FROM memory_index WHERE slug='main'",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "local"
    );
    db.execute_batch("INSERT INTO memory_entries VALUES(3,'ai-gateway','0','alpha','new memory','general','realtime',0);
        INSERT INTO memory_index VALUES('ai-gateway','0',5,'new-note','new','new memory','alpha','now');").unwrap();
    fs::write(target.join("reference-new-note.md"), b"new reference").unwrap();
    fixture.restore_one("alpha", GATEWAY_USER_ID).unwrap();
    assert_eq!(
        fs::read(scope.join("reference-new-note.md")).unwrap(),
        b"new reference"
    );
    assert_eq!(
        db.query_row("SELECT user_id FROM memory_entries WHERE id=3", [], |row| {
            row.get::<_, String>(0)
        })
        .unwrap(),
        "local"
    );
    assert_eq!(
        db.query_row(
            "SELECT user_id FROM memory_sync_state WHERE slug='alpha-note'",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "local"
    );
}

#[test]
fn empty_session_created_after_migration_brings_its_first_context_back() {
    let fixture = Fixture::new();
    fixture.add_default("empty", "local");
    let db = fixture.db();
    db.execute_batch("DELETE FROM session_messages;DELETE FROM task_messages;UPDATE sessions SET message_count=0;").unwrap();
    fs::remove_file(fixture.file("empty", "local")).unwrap();
    fixture.move_one("empty", "local").unwrap();
    let target = fixture.file("empty", GATEWAY_USER_ID);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(
        &target,
        b"{\"type\":\"user\",\"message\":{\"content\":\"first message\"}}\n",
    )
    .unwrap();
    db.execute(
        "UPDATE sessions SET message_count=1 WHERE session_id='empty'",
        [],
    )
    .unwrap();
    fixture.restore_one("empty", GATEWAY_USER_ID).unwrap();
    assert_eq!(
        fs::read(fixture.file("empty", "local")).unwrap(),
        fs::read(target).unwrap()
    );
}

#[test]
fn history_with_messages_but_no_jsonl_cannot_be_migrated_or_restored_incompletely() {
    let fixture = Fixture::new();
    fixture.add_default("alpha", "local");
    let source = fixture.file("alpha", "local");
    let original = fs::read(&source).unwrap();
    fs::remove_file(&source).unwrap();
    assert!(
        fixture
            .move_one("alpha", "local")
            .unwrap_err()
            .to_string()
            .contains("JSONL")
    );
    assert_eq!(fixture.owner("alpha"), "local");
    fs::write(&source, original).unwrap();
    fixture.move_one("alpha", "local").unwrap();
    fs::remove_file(fixture.file("alpha", GATEWAY_USER_ID)).unwrap();
    assert!(fixture.restore_one("alpha", GATEWAY_USER_ID).is_err());
    assert_eq!(fixture.owner("alpha"), GATEWAY_USER_ID);
}

#[test]
fn busy_unknown_and_pending_states_are_rejected_before_any_history_write() {
    for status in ["processing", "awaiting_confirmation", "unknown"] {
        let fixture = Fixture::new();
        fixture.add_default("alpha", "local");
        fixture
            .db()
            .execute("UPDATE sessions SET status=?1", [status])
            .unwrap();
        assert!(fixture.move_one("alpha", "local").is_err());
        assert_eq!(fixture.owner("alpha"), "local");
    }
    for status in ["running", "finalizing", "waiting_confirmation", "unknown"] {
        let fixture = Fixture::new();
        fixture.add_default("alpha", "local");
        fixture
            .db()
            .execute(
                "UPDATE task_messages SET execution_json=?1",
                [json!({"version":1,"status":status}).to_string()],
            )
            .unwrap();
        assert!(fixture.move_one("alpha", "local").is_err());
    }
    let fixture = Fixture::new();
    fixture.add_default("alpha", "local");
    append(
        &fixture.file("alpha", "local"),
        json!({"subtype":"runtime_state","runtime_type":"awaiting_confirmation",
        "runtime_data":{"session_state":"awaiting_confirmation","awaiting_state":{"pending_calls":[{}]}}}),
    );
    assert!(fixture.move_one("alpha", "local").is_err());
}

#[test]
fn durable_run_pending_and_unknown_im_links_are_not_migratable() {
    let fixture = Fixture::new();
    fixture.add_default("alpha", "local");
    let path = fixture
        .database
        .parent()
        .unwrap()
        .join("harness-data/chat_runs.sqlite3");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let runs = Connection::open(&path).unwrap();
    runs.execute_batch(
        "CREATE TABLE runs(run_id TEXT PRIMARY KEY,request_id TEXT,fingerprint TEXT,record TEXT);",
    )
    .unwrap();
    runs.execute(
        "INSERT INTO runs VALUES('r','q','f',?1)",
        [json!({"session_id":"alpha","status":"waiting_confirmation",
        "awaiting_confirmation":true,"prompts":[]})
        .to_string()],
    )
    .unwrap();
    assert!(fixture.move_one("alpha", "local").is_err());
    runs.execute("DELETE FROM runs", []).unwrap();
    fixture.links("alpha", "local");
    let mut links = fixture.read_links();
    links["links"][0]["uncertain"] = json!(true);
    fs::write(
        fixture.config.with_extension("gmclaw-sessions.json"),
        serde_json::to_vec(&links).unwrap(),
    )
    .unwrap();
    assert!(fixture.move_one("alpha", "local").is_err());
    assert_eq!(fixture.owner("alpha"), "local");
}

#[test]
fn existing_target_conflicts_and_stale_selection_do_not_change_owners() {
    let fixture = Fixture::new();
    fixture.add_default("alpha", "local");
    assert!(fixture.move_one("alpha", "wrong-owner").is_err());
    let target = fixture.file("alpha", GATEWAY_USER_ID);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    let body = b"{\"type\":\"user\",\"content\":\"different context\"}\n";
    fs::write(&target, body).unwrap();
    assert!(fixture.move_one("alpha", "local").is_err());
    assert_eq!(fixture.owner("alpha"), "local");
    assert_eq!(fs::read(&target).unwrap(), body);
    assert!(!ledger_path(&fixture.config).exists());
}

#[test]
fn unknown_schema_missing_database_and_illegal_paths_never_create_or_rebuild_a_db() {
    let fixture = Fixture::new();
    let missing = fixture.database.with_file_name("missing.db");
    assert!(list_at(&missing, &fixture.config).is_err());
    assert!(!missing.exists());
    fixture.add_default("alpha", "local");
    for id in ["../escape", "..", "CON", "bad:stream", "session/alpha"] {
        assert!(fixture.move_one(id, "local").is_err());
    }
    fixture
        .db()
        .execute("ALTER TABLE tasks ADD COLUMN user_id TEXT", [])
        .unwrap();
    assert!(fixture.move_one("alpha", "local").is_err());
    assert_eq!(fixture.owner("alpha"), "local");
    assert_eq!(
        fixture
            .db()
            .query_row("SELECT count(*) FROM task_messages", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn conflicting_project_directories_are_not_guessed() {
    let fixture = Fixture::new();
    fixture.add_default("alpha", "local");
    let wrong = fixture._root.path().join("different-project");
    fixture
        .db()
        .execute(
            "UPDATE sessions SET extra_data=?1",
            [json!({"project":{"id":"0","path":wrong}}).to_string()],
        )
        .unwrap();
    let list = list_at(&fixture.database, &fixture.config).unwrap();
    assert!(!list.entries[0].can_move);
    assert!(fixture.move_one("alpha", "local").is_err());
}

#[test]
fn sqlite_commit_failure_rolls_back_files_links_and_ownership() {
    let fixture = Fixture::new();
    fixture.add_default("alpha", "local");
    fixture.links("alpha", "local");
    let link_path = fixture.config.with_extension("gmclaw-sessions.json");
    let original_link = fs::read(&link_path).unwrap();
    let ledger = Ledger::load(&fixture.database, &fixture.config).unwrap();
    let original_ledger = serde_json::to_vec_pretty(&ledger).unwrap();
    let mut db = fixture.db();
    db.execute_batch("PRAGMA foreign_keys=ON;CREATE TABLE guard_parent(id INTEGER PRIMARY KEY);
        CREATE TABLE guard_child(id INTEGER REFERENCES guard_parent(id) DEFERRABLE INITIALLY DEFERRED);").unwrap();
    let transaction = db.transaction().unwrap();
    transaction
        .execute(
            "UPDATE sessions SET user_id='ai-gateway' WHERE session_id='alpha'",
            [],
        )
        .unwrap();
    transaction
        .execute("INSERT INTO guard_child VALUES(123)", [])
        .unwrap();
    let target = fixture.file("alpha", GATEWAY_USER_ID);
    let edits = vec![
        memory::Edit {
            path: target.clone(),
            before: None,
            after: Some(b"synthetic copy".to_vec()),
        },
        memory::Edit {
            path: link_path.clone(),
            before: Some(original_link.clone()),
            after: Some(b"synthetic changed link".to_vec()),
        },
    ];
    assert!(
        commit_files(
            transaction,
            &ledger_path(&fixture.config),
            &original_ledger,
            &ledger,
            edits
        )
        .is_err()
    );
    assert_eq!(fixture.owner("alpha"), "local");
    assert!(!target.exists());
    assert_eq!(fs::read(link_path).unwrap(), original_link);
}

#[cfg(windows)]
#[test]
fn windows_locked_link_write_rolls_back_the_prior_file_and_database_changes() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    fixture.add_default("alpha", "local");
    fixture.links("alpha", "local");
    let link_path = fixture.config.with_extension("gmclaw-sessions.json");
    let original_link = fs::read(&link_path).unwrap();
    // Reads still work, but Windows refuses atomic replacement of this file.
    let _busy_file = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&link_path)
        .unwrap();
    let ledger = Ledger::load(&fixture.database, &fixture.config).unwrap();
    let original_ledger = serde_json::to_vec_pretty(&ledger).unwrap();
    let mut db = fixture.db();
    let transaction = db.transaction().unwrap();
    transaction
        .execute(
            "UPDATE sessions SET user_id='ai-gateway' WHERE session_id='alpha'",
            [],
        )
        .unwrap();
    let target = fixture.file("alpha", GATEWAY_USER_ID);
    let edits = vec![
        memory::Edit {
            path: target.clone(),
            before: None,
            after: Some(b"synthetic copy".to_vec()),
        },
        memory::Edit {
            path: link_path.clone(),
            before: Some(original_link.clone()),
            after: Some(b"new link".to_vec()),
        },
    ];
    assert!(
        commit_files(
            transaction,
            &ledger_path(&fixture.config),
            &original_ledger,
            &ledger,
            edits
        )
        .is_err()
    );
    assert_eq!(fixture.owner("alpha"), "local");
    assert!(!target.exists());
    assert_eq!(fs::read(link_path).unwrap(), original_link);
}

#[test]
fn interrupted_precommit_record_can_recover_link_and_created_files() {
    let fixture = Fixture::new();
    fixture.add_default("alpha", "local");
    fixture.links("alpha", "local");
    fixture.move_one("alpha", "local").unwrap();
    fixture
        .db()
        .execute("UPDATE sessions SET user_id='local'", [])
        .unwrap();
    let mut ledger = Ledger::load(&fixture.database, &fixture.config).unwrap();
    ledger.moves[0].phase = Phase::Preparing;
    memory::write(
        &ledger_path(&fixture.config),
        Some(&serde_json::to_vec_pretty(&ledger).unwrap()),
    )
    .unwrap();
    assert!(list_at(&fixture.database, &fixture.config).unwrap().entries[0].can_restore);
    fixture.restore_one("alpha", "local").unwrap();
    assert_eq!(fixture.owner("alpha"), "local");
    assert_eq!(
        fixture.read_links()["links"][0]["context"]["user_id"],
        "local"
    );
    assert!(!fixture.file("alpha", GATEWAY_USER_ID).exists());
}

#[test]
fn links_in_unknown_scope_and_external_owner_changes_refuse_undo() {
    let fixture = Fixture::new();
    fixture.add_default("alpha", "local");
    fixture.links("alpha", "different-user");
    assert!(fixture.move_one("alpha", "local").is_err());
    fixture.links("alpha", "local");
    fixture.move_one("alpha", "local").unwrap();
    fixture
        .db()
        .execute("UPDATE sessions SET user_id='changed-externally'", [])
        .unwrap();
    assert!(!list_at(&fixture.database, &fixture.config).unwrap().entries[0].can_restore);
    assert!(fixture.restore_one("alpha", "changed-externally").is_err());
    assert_eq!(fixture.owner("alpha"), "changed-externally");
}
