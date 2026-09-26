//! 16 课 §6-§8：参数绑定、动态 WHERE、批量插入、数据库侧 JSON 聚合

use sqlx::{sqlite::SqlitePool, QueryBuilder, Row};

pub async fn run(pool: &SqlitePool) -> anyhow::Result<()> {
    // ---------- 1. 位置参数与重复使用（?1 风格，PG 用 $1，写法统一） ----------
    let row: (i64, i64) = sqlx::query_as(
        "SELECT MAX(price_cents), ?1 FROM products WHERE category_id = ?1",
    )
    .bind(1) // ?1 可以复用
    .fetch_one(pool)
    .await?;
    println!("键盘类最贵价: {}", row.0);

    // ---------- 2. IN 列表：SQLite 不支持数组绑定，动态拼接占位符 ----------
    //    （PG 可直接 WHERE id = ANY($1) 绑 Vec<i64>，17 课展示）
    let ids = vec![1i64, 3, 5];
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let sql = format!("SELECT name FROM products WHERE id IN ({placeholders})");
    let mut q = sqlx::query_as::<_, (String,)>(&sql);
    for id in &ids {
        q = q.bind(id);
    }
    let names = q.fetch_all(pool).await?;
    println!("IN 查询（ids={ids:?}）: {names:?}");

    // ---------- 3. QueryBuilder：安全的动态 SQL（17 课 §4 详解） ----------
    //    列表页筛选：min/max 可选参数
    let min: Option<i64> = Some(50_000);
    let max: Option<i64> = None;
    let mut qb: QueryBuilder<sqlx::Sqlite> =
        QueryBuilder::new("SELECT name, price_cents FROM products WHERE 1=1");
    if let Some(min) = min {
        qb.push(" AND price_cents >= ").push_bind(min);
    }
    if let Some(max) = max {
        qb.push(" AND price_cents <= ").push_bind(max);
    }
    qb.push(" ORDER BY price_cents DESC LIMIT 3");
    let filtered: Vec<(String, i64)> = qb.build_query_as().fetch_all(pool).await?;
    println!("动态筛选（min=5万, max=None）: {filtered:?}");

    // ---------- 4. 批量插入：push_values 一条语句插多行 ----------
    // ⚠️ 教学点：这里【不能】用 CREATE TEMP TABLE！
    //    临时表属于创建它的那条连接，而池中每条查询可能拿到不同连接 → no such table。
    //    会话级状态（临时表/SET 变量）需要连接亲和：let mut conn = pool.acquire() 后一直用 &conn。
    //    演示用普通表代替，用完即删。
    sqlx::query("CREATE TABLE IF NOT EXISTS tmp_tags (name TEXT NOT NULL UNIQUE)").execute(pool).await?;
    let tags = ["键盘", "显示器", "椅子"];
    let mut qb: QueryBuilder<sqlx::Sqlite> =
        QueryBuilder::new("INSERT INTO tmp_tags (name) ");
    qb.push_values(tags, |mut b, t| {
        b.push_bind(t);
    });
    let res = qb.build().execute(pool).await?;
    println!("批量插入 {} 行临时表（1 条 SQL）", res.rows_affected());
    sqlx::query("DROP TABLE tmp_tags").execute(pool).await?;

    // ---------- 5. UPSERT + RETURNING（11 课 SQL 特性在 Rust 里的形态） ----------
    //    幂等领券：重复执行无副作用，返回行判断是否首次
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS coupon_claims (
             user_id INTEGER NOT NULL,
             code TEXT NOT NULL,
             PRIMARY KEY (user_id, code)
         )",
    )
    .execute(pool)
    .await?;

    for round in 1..=2 {
        let claimed: Option<(i64, String)> = sqlx::query_as(
            "INSERT INTO coupon_claims (user_id, code) VALUES (?1, 'WELCOME10')
             ON CONFLICT (user_id, code) DO NOTHING
             RETURNING user_id, code",
        )
        .bind(1)
        .fetch_optional(pool) // DO NOTHING 冲突时返回 0 行 → None
        .await?;
        println!("第 {round} 次领取: {claimed:?}");
    }
    sqlx::query("DROP TABLE coupon_claims").execute(pool).await?;

    // ---------- 6. json_group_array：一条 SQL 出嵌套结构（15 课） ----------
    let rows = sqlx::query(
        "SELECT c.name AS category,
                json_group_array(json_object('name', p.name, 'price', p.price_cents)) AS items
         FROM categories c
         JOIN products p ON p.category_id = c.id
         GROUP BY c.id, c.name
         ORDER BY c.id LIMIT 3",
    )
    .fetch_all(pool)
    .await?;

    let mut out = Vec::new();
    for r in rows {
        let cat: String = r.try_get("category")?;
        // JSON 列直接取成 serde_json::Value / 反序列化成强类型
        let items: Vec<serde_json::Value> = serde_json::from_str(r.try_get::<String, _>("items")?.as_str())?;
        out.push(serde_json::json!({ "category": cat, "items": items }));
    }
    println!("json_agg 嵌套结果: {}", serde_json::to_string_pretty(&serde_json::json!(out))?);

    Ok(())
}
