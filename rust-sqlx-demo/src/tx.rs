//! 16 课 §9 + 12 课：事务实战 —— 下单三步（扣库存→建订单→写明细）
//!
//! 演示点：
//! - pool.begin() / tx.commit() / drop 自动回滚
//! - 条件更新防超卖（rows_affected == 0 → 业务错误）
//! - 任一步失败，整单回滚（不留半成品）

use sqlx::sqlite::SqlitePool;

#[derive(Debug)]
enum PlaceOrderError {
    OutOfStock,
    Db(sqlx::Error),
}

impl From<sqlx::Error> for PlaceOrderError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e)
    }
}

/// 下单：buy(uid=1, pid=21 Herman Miller Aeron, qty)
/// 返回订单 id；库存不足返回 Err(OutOfStock)
async fn place_order(
    pool: &SqlitePool,
    uid: i64,
    pid: i64,
    qty: i64,
) -> Result<i64, PlaceOrderError> {
    let mut tx = pool.begin().await?; // BEGIN

    // ① 查价（演示在事务内读）
    let (price,): (i64,) = sqlx::query_as("SELECT price_cents FROM products WHERE id = ?1")
        .bind(pid)
        .fetch_one(&mut *tx)
        .await?;
    let total = price * qty;

    // ② 扣库存：条件更新防超卖（12 课原子方案）
    let res = sqlx::query(
        "UPDATE products SET stock = stock - ?1 WHERE id = ?2 AND stock >= ?1",
    )
    .bind(qty)
    .bind(pid)
    .execute(&mut *tx)
    .await?;
    if res.rows_affected() == 0 {
        // 直接 return：tx 被 drop → sqlx 自动 ROLLBACK（①不生效）
        return Err(PlaceOrderError::OutOfStock);
    }

    // ③ 建订单 + 拿自增 id（RETURNING，11 课）
    let (order_id,): (i64,) = sqlx::query_as(
        "INSERT INTO orders (user_id, status, total_cents)
         VALUES (?1, 'paid', ?2) RETURNING id",
    )
    .bind(uid)
    .bind(total)
    .fetch_one(&mut *tx)
    .await?;

    // ④ 写明细
    sqlx::query(
        "INSERT INTO order_items (order_id, product_id, quantity, unit_price_cents)
         VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(order_id)
    .bind(pid)
    .bind(qty)
    .bind(price)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?; // COMMIT：四步原子生效
    Ok(order_id)
}

pub async fn run(pool: &SqlitePool) -> anyhow::Result<()> {
    let (stock_before,): (i64,) = sqlx::query_as("SELECT stock FROM products WHERE id = 21")
        .fetch_one(pool)
        .await?;

    // 正常下单
    match place_order(pool, 1, 21, 1).await {
        Ok(order_id) => println!("下单成功，订单 id = {order_id}"),
        Err(e) => println!("下单失败: {e:?}"),
    }

    // 超卖尝试：库存只有 {stock_before-1}
    match place_order(pool, 1, 21, 100).await {
        Ok(_) => println!("异常：竟然成功了？！"),
        Err(e) => println!("超卖被拦截: {e:?}"),
    }

    // 验证：库存恰好减 1，新订单的明细完整
    let (stock_after,): (i64,) = sqlx::query_as("SELECT stock FROM products WHERE id = 21")
        .fetch_one(pool)
        .await?;
    println!(
        "库存 {stock_before} → {stock_after}（超卖尝试未产生副作用，事务已回滚）"
    );

    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT o.id, p.name FROM orders o
         JOIN order_items oi ON oi.order_id = o.id
         JOIN products p ON p.id = oi.product_id
         WHERE o.id = (SELECT MAX(id) FROM orders)",
    )
    .fetch_all(pool)
    .await?;
    println!("新订单明细: {rows:?}");
    Ok(())
}
