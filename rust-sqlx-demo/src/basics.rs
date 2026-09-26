//! 16 课 §2-§5：连接池取连接后的查询基础
//!
//! - fetch_one / fetch_all / fetch_optional / execute 的区别
//! - 元组映射 vs FromRow struct 映射
//! - 字符串列 ↔ Rust enum（#[derive(sqlx::Type)]）

use sqlx::sqlite::SqlitePool;

/// 行结构体：列名（或别名）必须与字段名一致
/// 想改列名用 #[sqlx(rename = "...")]
#[derive(Debug, sqlx::FromRow)]
struct ProductRow {
    id: i64,
    name: String,
    price_cents: i64,
    stock: i64,
}

/// 枚举映射：status TEXT → enum（16 课 §5）
/// SQLite 没有 enum 类型，用 #[sqlx(rename_all = "lowercase")] 约定文本
#[derive(Debug, PartialEq, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
enum OrderStatus {
    Pending,
    Paid,
    Shipped,
    Completed,
    Cancelled,
}

pub async fn run(pool: &SqlitePool) -> anyhow::Result<()> {
    // ---------- fetch_one：恰好一行，0 行报错 RowNotFound ----------
    let (cnt,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM products")
        .fetch_one(pool)
        .await?;
    println!("商品总数: {cnt}");

    // ---------- fetch_optional：可能没有（对应 Option<T> / NULL 语义） ----------
    let maybe: Option<(i64, String)> =
        sqlx::query_as("SELECT id, name FROM products WHERE price_cents > ?1")
            .bind(2_000_000) // 全店没有这么贵的
            .fetch_optional(pool)
            .await?;
    println!("超过 2 万元的商品: {maybe:?}"); // None

    // ---------- fetch_all：多行 ----------
    let top: Vec<(String, i64)> = sqlx::query_as(
        "SELECT name, price_cents FROM products ORDER BY price_cents DESC LIMIT 3",
    )
    .fetch_all(pool)
    .await?;
    println!("最贵 Top3: {top:?}");

    // ---------- execute：不取行，看 rows_affected ----------
    let res = sqlx::query("UPDATE products SET stock = stock + 1 WHERE id = ?1")
        .bind(24)
        .execute(pool)
        .await?;
    println!("受影响行数: {}", res.rows_affected());

    // ---------- FromRow struct 映射 ----------
    let products: Vec<ProductRow> = sqlx::query_as::<_, ProductRow>(
        "SELECT id, name, price_cents, stock FROM products WHERE category_id = ?1",
    )
    .bind(6) // 椅子
    .fetch_all(pool)
    .await?;
    println!("椅子类商品: {products:?}");

    // ---------- NULL 列 → Option<T> ----------
    // comment 全非空，演示 COALESCE 前的原始可空形态
    #[derive(Debug, sqlx::FromRow)]
    struct ReviewRow {
        rating: i64,
        comment: Option<String>,
    }
    let reviews: Vec<ReviewRow> =
        sqlx::query_as("SELECT rating, comment FROM reviews ORDER BY id LIMIT 3")
            .fetch_all(pool)
            .await?;
    println!("评价（comment 为 Option<String>）: {reviews:?}");

    // ---------- enum 映射 ----------
    let statuses: Vec<(i64, OrderStatus)> =
        sqlx::query_as("SELECT id, status FROM orders ORDER BY id LIMIT 5")
            .fetch_all(pool)
            .await?;
    println!("订单状态（enum 反序列化）: {statuses:?}");
    // 写回时同样可 bind enum：
    let n = sqlx::query("UPDATE orders SET status = ?1 WHERE id = ?2")
        .bind(OrderStatus::Paid) // 序列化为 'paid'
        .bind(10)
        .execute(pool)
        .await?
        .rows_affected();
    println!("把订单10状态改回 paid（enum → TEXT）: 受影响 {n} 行");

    Ok(())
}
