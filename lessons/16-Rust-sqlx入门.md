# 16 · Rust sqlx 入门

> 本课目标：在 Rust 里连接数据库、执行前面 15 课学过的所有 SQL：连接池、查询四件套、参数绑定、enum 映射、事务。配套可运行项目：`rust-sqlx-demo/`。
> 本课假设你已完成 00-15 课（SQL 部分不再逐句解释）。

---

## 1. sqlx 是什么、为什么主线用它

```toml
[dependencies]
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "macros"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

- **不是 ORM**：你写原生 SQL，sqlx 负责连接、参数绑定、结果映射、事务、迁移；
- **异步优先**：与 axum/actix 的 tokio 生态无缝集成；
- **可选编译期校验**：`query!` 宏直接连库检查 SQL（17 课）；
- 支持 PostgreSQL / MySQL / SQLite / MSSQL。

feature 按需启用：`sqlite` / `postgres` / `mysql`（可同时开）。`macros` 提供 `query!` 家族。

## 2. 连接与连接池

```rust
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::str::FromStr;

// 方式一：URL 直连（PG: postgres://user:pass@host/db）
let pool = SqlitePoolOptions::new()
    .max_connections(5)
    .acquire_timeout(std::time::Duration::from_secs(3))
    .connect("sqlite://demo.db?mode=rwc")     // rwc = 不存在则建
    .await?;

// 方式二：选项构造（能设置 URL 放不下的参数）
let opts = SqliteConnectOptions::from_str("sqlite://demo.db?mode=rwc")?
    .foreign_keys(true)          // SQLite 必须手动开外键！
    .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);  // WAL：读写并发更好
let pool = SqlitePoolOptions::new().connect_with(opts).await?;
```

**为什么用池（Pool）而不是单个连接**：

- 建连是重操作（TCP + TLS + 认证 + 会话状态），必须复用；
- 池限制并发数，**保护数据库**（PG 默认每个连接一个进程，几百个连接能把它打死）；
- Axum 每个 handler 从池里 acquire，用完自动归还。

池是 `Arc` 内部计数，直接 clone 或传引用到处用，**不要**用 `once_cell` 再包一层（sqlx 池本身就是并发结构）。

## 3. 查询四件套

| 方法 | 返回 | 0 行时 | 典型用途 |
|------|------|--------|---------|
| `fetch_one` | 1 行 | **Err(RowNotFound)** | 主键查询、`RETURNING` |
| `fetch_optional` | `Option<行>` | `Ok(None)` | 按业务键查找（用户名查用户）|
| `fetch_all` | `Vec<行>` | `Ok(vec![])` | 列表查询 |
| `execute` | 影响行数等 | 无行概念 | UPDATE/DELETE/DDL |

```rust
// 恰好一行（COUNT 永远有一行）
let (cnt,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM products")
    .fetch_one(&pool).await?;

// 可能没有：登录查用户
let user: Option<(i64, String)> =
    sqlx::query_as("SELECT id, username FROM users WHERE username = ?1")
        .bind(&form.username)
        .fetch_optional(&pool).await?;
match user {
    Some((id, name)) => { /* 登录成功 */ }
    None => { /* 用户名或密码错误（别提示哪个错）*/ }
}

// 多行
let top: Vec<(String, i64)> = sqlx::query_as(
        "SELECT name, price_cents FROM products ORDER BY price_cents DESC LIMIT 5")
    .fetch_all(&pool).await?;

// 写操作看行数
let res = sqlx::query("UPDATE products SET stock = stock - ?1 WHERE id = ?2 AND stock >= ?1")
    .bind(qty).bind(pid)
    .execute(&pool).await?;
if res.rows_affected() == 0 { return Err(AppError::OutOfStock); }
```

> `query_as::<_, T>` 的 T 可以是**元组**（按列序）或 `#[derive(FromRow)]` **结构体**（按列名）。生产代码一律用结构体——加列不改序，编译器帮你对。

## 4. FromRow：行 → 结构体

```rust
#[derive(Debug, sqlx::FromRow)]
struct Product {
    id: i64,
    name: String,
    price_cents: i64,
    stock: i64,
}

let products: Vec<Product> = sqlx::query_as::<_, Product>(
    "SELECT id, name, price_cents, stock FROM products WHERE category_id = ?1")
.bind(6)
.fetch_all(&pool).await?;
```

规则：

- 字段名 = 列名（SQL 里 `AS` 别名对齐，或 `#[sqlx(rename = "price_cents")]`）；
- NULL 列 ↔ `Option<T>`（03 课的 NULL 知识直接映射到这里）；
- 嵌套类型：`sqlx::types::Json<T>`、`chrono` 时间、`rust_decimal`、`uuid`（需对应 feature）。

## 5. enum 映射：status TEXT 不再裸奔

```rust
#[derive(Debug, PartialEq, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
enum OrderStatus { Pending, Paid, Shipped, Completed, Cancelled }

// 读：TEXT → enum（脏值直接报错，比 String 更早暴露问题）
let s: Vec<(i64, OrderStatus)> =
    sqlx::query_as("SELECT id, status FROM orders LIMIT 5")
        .fetch_all(&pool).await?;

// 写：enum → TEXT
sqlx::query("UPDATE orders SET status = ?1 WHERE id = ?2")
    .bind(OrderStatus::Shipped).bind(7)
    .execute(&pool).await?;
```

PG 原生枚举更配：`CREATE TYPE order_status AS ENUM(...)` + `#[sqlx(type_name = "order_status")]`（10 课方案 3）。

## 6. 参数绑定：防注入 + 类型安全

```rust
// ✅ 永远绑定，永不 format! 拼 SQL
sqlx::query("SELECT * FROM products WHERE name LIKE '%' || ?1 || '%'")
    .bind(&q)                       // LIKE 的 % 也可以放参数里
    .fetch_all(&pool).await?;

// ✅ 列名/排序方向不能绑定（它们不是值）→ 白名单映射
let col = match sort { Sort::Price => "price_cents", Sort::Name => "name" };
let dir = if asc { "ASC" } else { "DESC" };       // 只可能是这两个值
let sql = format!("SELECT ... ORDER BY {col} {dir} LIMIT ?1");  // 列名来自白名单，安全
```

（完整动态 WHERE 见 17 课 QueryBuilder。）

## 7. IN 列表与批量写入

```rust
// SQLite/MySQL：无数组绑定 → 动态占位符
let ids = vec![1i64, 3, 5];
let ph = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
let mut q = sqlx::query_as::<_, (String,)>(&format!(
    "SELECT name FROM products WHERE id IN ({ph})"));
for id in &ids { q = q.bind(id); }

// PostgreSQL：直接绑定数组 WHERE id = ANY($1)
// sqlx::query_as::<_, (String,)>("SELECT name FROM products WHERE id = ANY($1)")
//     .bind(vec![1i64, 3, 5])

// 批量插入：QueryBuilder::push_values → 一条多值 INSERT（11 课多行插入的 Rust 版）
let mut qb = sqlx::QueryBuilder::new(
    "INSERT INTO order_items (order_id, product_id, quantity, unit_price_cents) ");
qb.push_values(items, |mut b, it| {
    b.push_bind(order_id).push_bind(it.pid)
     .push_bind(it.qty).push_bind(it.price);
});
qb.build().execute(&pool).await?;
```

**永远不要 `for item in items { query(...).execute().await }`**——N+1 写入（13 课），一次网络往返 vs 一百次。

## 8. UPSERT 与 RETURNING 的 Rust 形态

```rust
// 幂等操作：fetch_optional + DO NOTHING，None = 已存在
let claimed: Option<(i64, String)> = sqlx::query_as(
    "INSERT INTO coupon_claims (user_id, code) VALUES (?1, 'WELCOME10')
     ON CONFLICT (user_id, code) DO NOTHING
     RETURNING user_id, code")
.bind(uid)
.fetch_optional(&pool).await?;

// INSERT + RETURNING：一步拿回自增 id，不用 last_insert_rowid（有竞态）
let (id, created_at): (i64, String) = sqlx::query_as(
    "INSERT INTO users (username, email) VALUES (?1, ?2) RETURNING id, created_at")
.bind("irene").bind("irene@example.com")
.fetch_one(&pool).await?;
```

## 9. 事务：12 课的下单三步走

```rust
let mut tx = pool.begin().await?;                       // BEGIN

let res = sqlx::query("UPDATE products SET stock = stock - ?1 WHERE id = ?2 AND stock >= ?1")
    .bind(qty).bind(pid).execute(&mut *tx).await?;      // 注意 &mut *tx
if res.rows_affected() == 0 { return Err(OutOfStock); } // drop(tx) → 自动 ROLLBACK

let (order_id,): (i64,) = sqlx::query_as(
    "INSERT INTO orders (user_id, status, total_cents) VALUES (?1,'paid',?2) RETURNING id")
    .bind(uid).bind(total).fetch_one(&mut *tx).await?;

sqlx::query("INSERT INTO order_items (...) VALUES (?,?,?,?)")
    .bind(order_id)/* ... */.execute(&mut *tx).await?;

tx.commit().await?;                                     // COMMIT
```

要点（与 12 课对应）：

- `&mut *tx`：事务是排他借用，**编译期**阻止事务并发使用；
- 任何 `?` 提前返回 → `tx` drop → **自动回滚**，错误路径不需要显式 ROLLBACK；
- 事务里的所有查询必须走 `&mut *tx`，混用 `&pool` = 两个连接 = 两个事务（经典 bug）。

## 10. 运行配套项目

```bash
cd rust-sqlx-demo
cargo run     # 自动建 demo.db + 跑迁移 + 四组演示（basics/params/tx/macros）
```

对照源码看输出：`src/basics.rs`（本课 §3-§5）、`src/params.rs`（§7-§8）、`src/tx.rs`（§9）。改坏任何一条 SQL 再 `cargo run`，观察错误长什么样。

## 11. 常见坑

| 坑 | 说明 |
|---|---|
| SQLite 忘开外键 | `SqliteConnectOptions::foreign_keys(true)`，否则约束全部失效 |
| 池里 acquire 后泄漏 | 手动 `pool.acquire()` 要显式 drop；推荐 `fetch_*(&pool)` 风格自动归还 |
| 事务里用 `&pool` | 变成另一个连接、另一个事务；事务内一律 `&mut *tx` |
| `fetch_one` 查列表 | 期望多行却用 fetch_one → 只拿一行或 RowNotFound |
| 把 enum 当 String 存还无 CHECK | 脏值入库后在 enum 反序列化处爆炸，两头都加（10 课 CHECK + Rust enum）|
| 循环单条 INSERT | N 次往返；push_values / PG COPY |
| SQLite 时间是 TEXT | `created_at` 映射 `String`；要 `DateTime` 需保证写入格式（sqlx chrono 兼容 ISO8601）|

## 12. 练习

在 `rust-sqlx-demo` 基础上完成（建议复制 `src/basics.rs` 改）：

1. 写 `fn top_reviewed(pool, limit) -> Vec<(String, i64)>`：评价数最多的前 N 商品（06 课 SQL + 本课 fetch_all）。
2. 写 `fn mark_shipped(pool, order_id) -> Result<bool>`：把 paid 订单改为 shipped，返回是否成功（条件 UPDATE + rows_affected，12 课状态机）。
3. 写 `fn restock(pool, items: &[(i64, i64)])`：批量补货（product_id, 加多少），一条 SQL（push_values + UPDATE ... FROM？SQLite 不支持，改为 CASE 或逐条包一个事务；PG 可用 `UPDATE ... SET stock = stock + v.from (VALUES ...)`）。
4. 挑战：把 05 课练习 5（用户评价透视表）搬进 Rust：定义 struct 承载 `username/bad/mid/good`。
5. 思考：`fetch_optional` 和 `Option<fetch_one>` 有什么区别？（答案：后者 0 行是 Err，前者是 Ok(None)——语义完全不同）

<details><summary>答案要点</summary>

```rust
// 1.
let rows: Vec<(String, i64)> = sqlx::query_as(
    "SELECT p.name, COUNT(*) cnt FROM reviews r
     JOIN products p ON p.id = r.product_id
     GROUP BY p.id, p.name ORDER BY cnt DESC LIMIT ?1")
.bind(limit).fetch_all(pool).await?;
Ok(rows)

// 2.
let res = sqlx::query("UPDATE orders SET status = 'shipped' WHERE id = ?1 AND status = 'paid'")
    .bind(order_id).execute(pool).await?;
Ok(res.rows_affected() == 1)

// 3. SQLite 方案：一个事务包住逐条（原子性够了，24 个商品无所谓性能）
let mut tx = pool.begin().await?;
for &(pid, add) in items {
    sqlx::query("UPDATE products SET stock = stock + ?1 WHERE id = ?2")
        .bind(add).bind(pid).execute(&mut *tx).await?;
}
tx.commit().await?;

// 4. struct UserRatingBuckets { username: String, bad: i64, mid: i64, good: i64 }
//    SQL 照抄 05 课答案，注意三个 SUM 别名与字段名一致。

// 5. fetch_optional: 0 行 → Ok(None)（正常业务分支）；
//    fetch_one: 0 行 → Err(RowNotFound)（当作错误处理）。
//    选型：按主键查"必然存在"的行用 fetch_one（不存在=真错误）；
//    按业务键查"可能不存在"用 fetch_optional。
```
</details>

---

← 上一课：[15 · 视图、序列、触发器、JSON](15-视图-序列-触发器-JSON.md) | 下一课：[17 · Rust sqlx 进阶](17-Rust-sqlx进阶.md) · [返回目录](../README.md) · 配套代码：[`rust-sqlx-demo/`](../rust-sqlx-demo/)
