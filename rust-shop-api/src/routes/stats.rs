//! 统计路由：月度营收报表（CTE + 窗口函数，07/09 课）

use axum::{extract::State, Json};
use crate::{error::ApiResult, models::MonthlyRevenue, state::AppState};

/// GET /api/stats/revenue
///
/// 09 课 LAG 窗口函数直接成为 API：
///   WITH monthly AS (按月聚合)
///   SELECT *, LAG(revenue) OVER (ORDER BY month) AS 上月 → 算环比
pub async fn revenue(State(st): State<AppState>) -> ApiResult<Vec<MonthlyRevenue>> {
    let rows: Vec<MonthlyRevenue> = sqlx::query_as(
        "WITH monthly AS (
             SELECT substr(created_at, 1, 7) AS month,
                    SUM(total_cents)          AS revenue
             FROM orders
             WHERE status <> 'cancelled'
             GROUP BY substr(created_at, 1, 7)
         )
         SELECT month, revenue,
                LAG(revenue) OVER (ORDER BY month) AS prev_revenue,
                revenue - LAG(revenue) OVER (ORDER BY month) AS diff
         FROM monthly
         ORDER BY month",
    )
    .fetch_all(&st.pool)
    .await?;

    Ok(Json(rows))
}
