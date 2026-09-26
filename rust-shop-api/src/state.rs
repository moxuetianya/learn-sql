//! 应用状态：handler 之间共享数据库池
//! 池内部是 Arc 计数，AppState 直接 Clone 即可（不要再用 once_cell 包一层）。

use sqlx::sqlite::SqlitePool;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
}
