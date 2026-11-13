//! Regression coverage for upgrading the pre-v2 production schema.
//! Requires `DATABASE_URL`; CI supplies a PostgreSQL role that can create
//! isolated throwaway databases.

use std::str::FromStr;

use automl_server::run_migrations;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    AssertSqlSafe, Connection, Executor, PgConnection, PgPool,
};
use uuid::Uuid;

const EMPTY_SQLX_HISTORY: &str = r#"
    CREATE TABLE _sqlx_migrations (
        version BIGINT PRIMARY KEY,
        description TEXT NOT NULL,
        installed_on TIMESTAMPTZ NOT NULL DEFAULT now(),
        success BOOLEAN NOT NULL,
        checksum BYTEA NOT NULL,
        execution_time BIGINT NOT NULL
    );
"#;

struct TestDatabase {
    name: String,
    admin: PgConnection,
    pool: PgPool,
}

impl TestDatabase {
    async fn create() -> Self {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required");
        let options = PgConnectOptions::from_str(&url).expect("parse DATABASE_URL");
        let mut admin = PgConnection::connect_with(&options.clone().database("postgres"))
            .await
            .expect("connect to postgres admin database");
        let name = format!("automl_migration_{}", Uuid::new_v4().simple());
        admin
            .execute(AssertSqlSafe(format!("CREATE DATABASE \"{name}\"")))
            .await
            .expect("create isolated test database");
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(options.database(&name))
            .await
            .expect("connect to isolated test database");
        Self { name, admin, pool }
    }

    async fn cleanup(mut self) {
        self.pool.close().await;
        self.admin
            .execute(AssertSqlSafe(format!(
                "DROP DATABASE \"{}\" WITH (FORCE)",
                self.name
            )))
            .await
            .expect("drop isolated test database");
    }
}

#[tokio::test]
async fn representative_pre_v2_schema_is_archived_then_upgraded() {
    let db = TestDatabase::create().await;
    db.pool
        .execute(EMPTY_SQLX_HISTORY)
        .await
        .expect("install empty SQLx history left by the failed alpha.2 start");
    db.pool
        .execute(
            r#"
            CREATE TABLE scholars (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                affiliation TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP::text
            );
            CREATE TABLE papers (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                journal_slug TEXT NOT NULL,
                authors TEXT NOT NULL DEFAULT '[]',
                status TEXT NOT NULL DEFAULT 'SUBMITTED'
            );
            CREATE TABLE reviews (
                id TEXT PRIMARY KEY,
                paper_id TEXT NOT NULL REFERENCES papers(id),
                reviewer_id TEXT NOT NULL
            );
            INSERT INTO scholars (id, name) VALUES ('legacy-scholar-1', 'Legacy Scholar');
            INSERT INTO papers (id, title, journal_slug)
                VALUES ('legacy-paper-1', 'Legacy Paper', 'legacy');
            INSERT INTO reviews (id, paper_id, reviewer_id)
                VALUES ('legacy-review-1', 'legacy-paper-1', 'legacy-scholar-1');
            "#,
        )
        .await
        .expect("install representative pre-v2 schema");

    run_migrations(&db.pool)
        .await
        .expect("upgrade pre-v2 schema");
    // A second start must be a no-op, not a second archival attempt.
    run_migrations(&db.pool)
        .await
        .expect("repeat migration is idempotent");

    let public_id_type: String = sqlx::query_scalar(
        r#"SELECT data_type FROM information_schema.columns
           WHERE table_schema = 'public' AND table_name = 'scholars' AND column_name = 'id'"#,
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(public_id_type, "uuid");
    let successful_migrations: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE success")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(successful_migrations, 10);

    let archived_scholars: i64 = sqlx::query_scalar("SELECT count(*) FROM automl_pre_v2.scholars")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    let archived_papers: i64 = sqlx::query_scalar("SELECT count(*) FROM automl_pre_v2.papers")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    let archived_reviews: i64 = sqlx::query_scalar("SELECT count(*) FROM automl_pre_v2.reviews")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        (archived_scholars, archived_papers, archived_reviews),
        (1, 1, 1)
    );

    db.cleanup().await;
}

#[tokio::test]
async fn unknown_partial_schema_fails_closed() {
    let db = TestDatabase::create().await;
    db.pool
        .execute("CREATE TABLE scholars (id TEXT PRIMARY KEY, display_name TEXT NOT NULL)")
        .await
        .expect("install partial schema");

    let error = run_migrations(&db.pool)
        .await
        .expect_err("partial schema must not be guessed at");
    assert!(error
        .to_string()
        .contains("unknown or partially converted AutoML schema"));
    let migrations_table: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('public._sqlx_migrations')::text")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert!(migrations_table.is_none());

    db.cleanup().await;
}

#[tokio::test]
async fn recorded_failed_migration_is_propagated() {
    let db = TestDatabase::create().await;
    db.pool
        .execute(EMPTY_SQLX_HISTORY)
        .await
        .expect("install SQLx history");
    sqlx::query(
        r#"INSERT INTO _sqlx_migrations
           (version, description, success, checksum, execution_time)
           VALUES (1, 'failed migration', false, '\x00', 1)"#,
    )
    .execute(&db.pool)
    .await
    .expect("record failed migration");

    let error = run_migrations(&db.pool)
        .await
        .expect_err("a recorded failed migration must abort startup");
    let detail = format!("{error:#}");
    assert!(detail.contains("partially applied"), "{detail}");

    db.cleanup().await;
}
