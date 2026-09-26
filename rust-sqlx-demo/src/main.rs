//! rust-sqlx-demo —— 课程 16/17 配套可运行示例
//!
//! 运行方式（在 rust-sqlx-demo 目录）:
//! ```bash
//! cargo run            # 每次运行都会重建 demo.db，可反复演练
//! ```
//!
//! 模块导航（对应课程章节）:
//! - basics: 连接池 / fetch_* / FromRow / enum 映射      → 16 课 §2-§5
//! - params: 参数绑定 / QueryBuilder / json_agg          → 16 课 §6-§8
//! - tx:     事务 / 防超卖 / 回滚                        → 16 课 §9 + 12 课
//! - macros: query! / query_as! 编译期校验               → 17 课 §1-§2

mod basics;
mod macros_demo;
mod params;
mod tx;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::str::FromStr;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ---------- 连接池（16 课 §2） ----------
    // demo.db 不存在则创建；启动时自动执行 migrations/ 下的迁移
    let opts = SqliteConnectOptions::from_str("sqlite://demo.db?mode=rwc")?
        .foreign_keys(true); // SQLite 必须手动开外键（等价 PRAGMA foreign_keys=ON）
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .acquire_timeout(std::time::Duration::from_secs(3))
        .connect_with(opts)
        .await?;

    // 版本化迁移：执行 migrations/ 目录（17 课 §5 详解）
    sqlx::migrate!("./migrations").run(&pool).await?;

    println!("=== 演示 1：查询基础（basics） ===");
    basics::run(&pool).await?;

    println!("\n=== 演示 2：参数绑定与批量（params） ===");
    params::run(&pool).await?;

    println!("\n=== 演示 3：事务与防超卖（tx） ===");
    tx::run(&pool).await?;

    println!("\n=== 演示 4：编译期校验宏（macros_demo） ===");
    macros_demo::run(&pool).await?;

    println!("\n全部演示完成 ✔");
    Ok(())
}
