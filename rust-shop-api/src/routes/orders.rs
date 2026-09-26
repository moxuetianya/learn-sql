//! 订单路由：下单（事务）与详情（json_agg 嵌套输出）

use axum::{extract::{Path, State}, Json};
use serde_json::json;
use sqlx::{QueryBuilder, Sqlite};

use crate::{
    error::{ApiResult, AppError},
    models::{CreateOrderReq, CreateOrderResp, OrderDetail},
    state::AppState,
};

/// POST /api/orders   body: {"user_id":1,"items":[{"product_id":21,"quantity":1}]}
///
/// 12 课事务 + 11 课条件更新 + 16 课批量插入的完整落地：
///   BEGIN
///     ① 批量查商品（IN，一条 SQL，防 N+1）
///     ② 逐个条件扣库存（UPDATE ... WHERE stock >= n，0 行 = OutOfStock → 回滚）
///     ③ INSERT orders RETURNING id
///     ④ 批量 INSERT order_items（QueryBuilder push_values）
///   COMMIT
pub async fn create(
    State(st): State<AppState>,
    Json(req): Json<CreateOrderReq>,
) -> ApiResult<CreateOrderResp> {
    if req.items.is_empty() {
        return Err(AppError::Conflict("订单明细不能为空".into()));
    }

    let mut tx = st.pool.begin().await?;

    // ① 批量查价（池/事务通吃的泛型 Executor，见 products.rs）
    let ids: Vec<i64> = req.items.iter().map(|i| i.product_id).collect();
    let products = super::products::get_products_by_ids(&mut *tx, &ids).await?;
    if products.len() != req.items.len() {
        return Err(AppError::NotFound); // 有商品 id 不存在 → 整单拒绝回滚
    }

    // ② 扣库存（条件更新防超卖：0 行受影响 = 库存不足）
    for item in &req.items {
        let res = sqlx::query(
            "UPDATE products SET stock = stock - ?1 WHERE id = ?2 AND stock >= ?1",
        )
        .bind(item.quantity)
        .bind(item.product_id)
        .execute(&mut *tx)
        .await?;
        if res.rows_affected() == 0 {
            return Err(AppError::OutOfStock(item.product_id)); // drop(tx) → 自动回滚
        }
    }

    // ③ 建订单（按明细算总额）
    let mut total: i64 = 0;
    for item in &req.items {
        let p = products.iter().find(|p| p.id == item.product_id)
            .ok_or(AppError::NotFound)?;
        total += p.price_cents * item.quantity;
    }
    let (order_id,): (i64,) = sqlx::query_as(
        "INSERT INTO orders (user_id, status, total_cents)
         VALUES (?1, 'paid', ?2) RETURNING id",
    )
    .bind(req.user_id)
    .bind(total)
    .fetch_one(&mut *tx)
    .await?;

    // ④ 批量写明细（一条多值 INSERT）
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        "INSERT INTO order_items (order_id, product_id, quantity, unit_price_cents) ",
    );
    qb.push_values(&req.items, |mut b, item| {
        let price = products.iter().find(|p| p.id == item.product_id)
            .map(|p| p.price_cents).unwrap_or(0);
        b.push_bind(order_id)
            .push_bind(item.product_id)
            .push_bind(item.quantity)
            .push_bind(price);
    });
    qb.build().execute(&mut *tx).await?;

    tx.commit().await?;

    Ok(Json(CreateOrderResp { order_id, total_cents: total }))
}

/// GET /api/orders/{id}
///
/// 15 课 json_group_array：一条 SQL 直接吐嵌套结构。
/// 注意：SQLite 的 JSON 函数返回 TEXT，行结构先收 String，再反序列化成 Value
/// （这是 SQLite 的惯用法；PG 的 jsonb 列可以直接映射 serde_json::Value）。
#[derive(Debug, sqlx::FromRow)]
struct OrderRow {
    id: i64,
    username: String,
    status: String,
    total_cents: i64,
    created_at: String,
    items: String, // json_group_array 的 TEXT 结果
}

pub async fn get_one(
    State(st): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<OrderDetail> {
    let row: Option<OrderRow> = sqlx::query_as(
        "SELECT o.id, u.username, o.status, o.total_cents, o.created_at,
                COALESCE(
                  json_group_array(
                    json_object('product_id', oi.product_id,
                                'name', p.name,
                                'quantity', oi.quantity,
                                'unit_price_cents', oi.unit_price_cents)
                  ) FILTER (WHERE oi.product_id IS NOT NULL), '[]'
                ) AS items
         FROM orders o
         JOIN users u ON u.id = o.user_id
         LEFT JOIN order_items oi ON oi.order_id = o.id
         LEFT JOIN products p ON p.id = oi.product_id
         WHERE o.id = ?1
         GROUP BY o.id, u.username, o.status, o.total_cents, o.created_at",
    )
    .bind(id)
    .fetch_optional(&st.pool)
    .await?;

    let r = row.ok_or(AppError::NotFound)?; // 404

    let items: serde_json::Value = serde_json::from_str(&r.items)
        .unwrap_or_else(|_| json!([])); // 解析失败兜底空数组（防御）

    Ok(Json(OrderDetail {
        id: r.id,
        username: r.username,
        status: r.status,
        total_cents: r.total_cents,
        created_at: r.created_at,
        items,
    }))
}
