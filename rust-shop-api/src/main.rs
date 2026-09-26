//! 20 课综合项目：shop REST API（Axum + sqlx + SQLite）
//!
//! 启动: cargo run   （默认 127.0.0.1:3000，自动迁移+种子数据）
//! 测试: 见 lessons/20-综合项目-Axum-REST-API.md 的 curl 示例
//!
//! 架构（20 课 §2）:
//!   main.rs    ── 组装: 池/路由/监听（不写业务）
//!   state.rs   ── AppState（池的共享）
//!   error.rs   ── AppError → HTTP 状态码的统一映射
//!   models.rs  ── 请求/响应 DTO（sqlx FromRow + serde 双派生）
//!   routes/    ── handler：参数提取 → SQL → DTO

mod error;
mod models;
mod routes;
mod state;

use axum::{routing::{get, post}, Router};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::{str::FromStr, time::Duration};
use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 结构化日志（生产必备，排查慢查询/错误的第一现场）
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rust_shop_api=debug,tower_http=info".into()),
        )
        .init();

    // ---------- 连接池（16 课 §2 / 17 课 §6） ----------
    let opts = SqliteConnectOptions::from_str("sqlite://api.db?mode=rwc")?
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .acquire_timeout(Duration::from_secs(3))
        .connect_with(opts)
        .await?;

    // 启动时自动迁移（17 课 §5；单实例部署的便捷做法）
    sqlx::migrate!("./migrations").run(&pool).await?;
    tracing::info!("迁移完成，shop API 就绪");

    // ---------- 路由 ----------
    let app = Router::new()
        .route("/api/health", get(|| async { "ok" }))
        // 商品
        .route("/api/products", get(routes::products::list).post(routes::products::create))
        .route("/api/products/{id}", get(routes::products::get_one))
        // 订单
        .route("/api/orders", post(routes::orders::create))
        .route("/api/orders/{id}", get(routes::orders::get_one))
        // 统计（窗口函数报表）
        .route("/api/stats/revenue", get(routes::stats::revenue))
        .with_state(AppState { pool });

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    tracing::info!("listening on http://127.0.0.1:3000");
    axum::serve(listener, app).await?;
    Ok(())
}
