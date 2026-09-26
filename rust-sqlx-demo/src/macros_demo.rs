//! 17 课 §1-§2：query! / query_as! 宏 —— SQL 的编译期校验
//!
//! 这些宏在【编译期】连接 DATABASE_URL（见 .env）：
//! - 校验 SQL 语法与表/列是否存在
//! - 推断每列的类型与可空性 → 直接得到强类型结果
//!
//! 试试把下面某条 SQL 的列名改错再 cargo build —— 编译失败而不是线上事故。

use sqlx::sqlite::SqlitePool;

// 手工映射 struct：字段类型/可空性由宏在编译期对齐
#[derive(Debug, sqlx::FromRow)]
struct SalesRow {
    name: String,
    category: String,
    sold_qty: i64,
    revenue: i64,
}

pub async fn run(pool: &SqlitePool) -> anyhow::Result<()> {
    // ---------- query_as! 宏：直接得到 struct，无需手写映射 ----------
    let top: Vec<SalesRow> = sqlx::query_as!(
        SalesRow,
        r#"
        SELECT p.name,
               c.name AS category,
               COALESCE(SUM(oi.quantity), 0)                        AS "sold_qty!",
               COALESCE(SUM(oi.quantity * oi.unit_price_cents), 0)  AS "revenue!"
        FROM products p
        JOIN categories c ON c.id = p.category_id
        LEFT JOIN order_items oi ON oi.product_id = p.id
        GROUP BY p.id, p.name, c.name
        ORDER BY SUM(oi.quantity * oi.unit_price_cents) DESC
        LIMIT 5
        "#,
    )
    .fetch_all(pool)
    .await?;
    println!("销量榜（编译期校验版）:");
    for r in &top {
        println!("  {:<44} {} | ¥{:>10}.{:02}", r.name, r.sold_qty, r.revenue / 100, r.revenue % 100);
    }

    // ---------- query! 宏：拿行再 Row API 取值 ----------
    let rows = sqlx::query!(
        "SELECT id, username, role FROM users ORDER BY id LIMIT 3"
    )
    .fetch_all(pool)
    .await?;
    println!("用户样例: {rows:?}");

    // ---------- 带参数的宏（编译期也校验占位符） ----------
    let uid = 1i64;
    let orders = sqlx::query!(
        "SELECT id, status, total_cents FROM orders WHERE user_id = ?1 ORDER BY id",
        uid
    )
    .fetch_all(pool)
    .await?;
    println!("alice 的订单: {orders:?}");

    // ---------- 可空性推断演示 ----------
    // comment 列可空 → 宏推断出 Option<String>，编译期就告诉你（而非运行时 unwrap 炸）
    let reviews = sqlx::query!(
        "SELECT rating, comment FROM reviews ORDER BY id LIMIT 3"
    )
    .fetch_all(pool)
    .await?;
    for r in reviews {
        let c = r.comment.as_deref().unwrap_or("（无文字）"); // 编译器逼你处理 None
        println!("  {}★ {c}", r.rating);
    }

    // ---------- 可空表达式要加 "alias!" 标注 ----------
    // AVG 可能对空集返回 NULL → 宏推断 Option<f64>；
    // 若你确定非空，用 "avg_price!" 告诉宏（然后自负其责）
    let stat = sqlx::query!(
        r#"SELECT COUNT(*) AS "cnt!", AVG(price_cents) AS avg_price FROM products"#
    )
    .fetch_one(pool)
    .await?;
    println!("商品数 {}，均价 {:?}", stat.cnt, stat.avg_price);

    Ok(())
}
