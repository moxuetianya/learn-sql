# 17 · Rust sqlx 进阶

> 本课目标：掌握 `query!` 宏的编译期校验与离线模式、错误处理体系、QueryBuilder 动态 SQL、迁移工作流、连接池调优。读完这课，你就能把 sqlx 用到生产级。

---

## 1. `query!` / `query_as!`：编译期连库校验

16 课的 `query_as::<_, T>` 是**运行时**映射（列名对不上 → 运行时错误）。宏版本在**编译时**连接 `DATABASE_URL`（读 `.env`）：

```toml
# .env
DATABASE_URL=sqlite://demo.db?mode=rwc
```

```rust
// query_as! 宏：SQL、列名、类型、可空性 全部编译期检查
let rows = sqlx::query_as!(
    Product,
    "SELECT id, name, price_cents, stock FROM products WHERE category_id = ?1",
    cat_id        // 参数直接逗号传，不用 .bind()
)
.fetch_all(&pool).await?;
```

把列名改错（`nmae`）→ **cargo build 直接失败**，并指出列不存在。类型对不上（`name` 绑给 `i64`）→ 编译失败。这就是"SQL 进入 Rust 类型系统"。

### 可空性标注：`"alias!"`

宏会自动推断 NULL 可能性，两种情况需要人工干预：

```rust
// ① 表达式宏推断不出（如 SUM 对空集返回 NULL）→ 自动 Option<f64>，没问题
let s = sqlx::query!("SELECT AVG(price_cents) a FROM products WHERE id = -1")
    .fetch_one(&pool).await?;       // s.a: Option<f64> = None

// ② 你【确定】非空但宏推成 Option（如 COUNT(*) 永远有值、COALESCE 过的列）
//    用 "alias!" 断言非空：
let s = sqlx::query!(
    r#"SELECT COUNT(*) AS "cnt!", COALESCE(SUM(stock),0) AS "total!" FROM products"#
)
.fetch_one(&pool).await?;           // s.cnt: i64（不再是 Option）
```

规则：SQLite 的 `COUNT(*)` 等聚合常被推成可空，加 `!` 断言要**有依据**——SUM/AVG 在空表时真是 NULL，别乱加。

### 运行时拼接的 SQL 不能用宏

宏参数必须是编译期常量字符串。需要动态拼（分页、动态排序、可选筛选）→ 用下一节 QueryBuilder。

## 2. 离线模式：CI 不连库也能编译

CI/构建机没有数据库怎么办？`cargo sqlx prepare` 把所有宏的校验结果缓存到 `.sqlx/` 目录：

```bash
cargo install sqlx-cli --no-default-features --features sqlite
cargo sqlx prepare      # 生成 .sqlx/query-*.json，提交到 git
```

CI 里设 `SQLX_OFFLINE=true`（或默认检测到 `.sqlx/` 目录自动离线），编译不再连库。**团队规范：改了 SQL 就重新 prepare 并提交**，否则 CI 与本地行为不一致。

## 3. 错误处理：sqlx::Error 家族

```rust
match sqlx::query("INSERT INTO users (username) VALUES (?1)")
    .bind("alice")                       // 已存在（UNIQUE）
    .execute(&pool).await
{
    Ok(_) => Ok(()),
    Err(e) => match e {
        sqlx::Error::Database(db) => {
            // db.code(): '23505' (PG unique_violation) / 2067 (SQLite UNIQUE)
            // db.constraint(): 'users.username' / 'sqlite_master.users.username'
            match db.constraint().map(str::to_string).as_deref() {
                Some(c) if c.ends_with("username") => Err(AppError::UsernameTaken),
                _ => Err(e.into()),
            }
        }
        sqlx::Error::RowNotFound => Err(AppError::NotFound),
        sqlx::Error::PoolTimedOut => Err(AppError::Busy),      // 池耗尽（§6）
        _ => Err(e.into()),
    }
}
```

生产分层建议（Axum 项目实战见 20 课）：

```
sqlx::Error                        → thiserror 包一层 AppError（Db(#[from] sqlx::Error)）
Database{constraint} + 错误码映射   → 业务错误（UsernameTaken / OutOfStock / ...）
RowNotFound                        → 404
PoolTimedOut / PoolClosed          → 503 + 告警（池配置或泄漏）
```

PG 约束错误码速查：`23505` unique、`23503` foreign_key、`23514` check_violation、`40P01` deadlock、`40001` serialization_failure（重试！）。

### 死锁/序列化冲突重试封装

```rust
async fn with_retry<T, F, Fut>(pool: &SqlitePool, mut f: F) -> anyhow::Result<T>
where F: FnMut() -> Fut, Fut: std::future::Future<Output = anyhow::Result<T>> {
    for attempt in 0..3 {
        match f().await {
            Err(e) if is_retryable(&e) => {
                tokio::time::sleep(std::time::Duration::from_millis(50 * (attempt + 1))).await;
                continue;
            }
            other => return other,
        }
    }
    anyhow::bail!("重试次数耗尽")
}

fn is_retryable(e: &anyhow::Error) -> bool {
    matches!(
        e.downcast_ref::<sqlx::Error>(),
        Some(sqlx::Error::Database(db))
            if matches!(db.code().as_deref(), Some("40001") | Some("40P01"))
    )
}
```

## 4. QueryBuilder：动态 SQL 的正确姿势

列表页 `?min=&max=&cat=&q=&sort=&page=` 这类需求，宏帮不了你。`QueryBuilder` 保证**只有值是绑定的，结构是你拼的**：

```rust
use sqlx::{QueryBuilder, Sqlite};

let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
    "SELECT id, name, price_cents, stock FROM products WHERE 1=1");

if let Some(min) = filters.min {
    qb.push(" AND price_cents >= ").push_bind(min);
}
if let Some(max) = filters.max {
    qb.push(" AND price_cents <= ").push_bind(max);
}
if let Some(cat) = filters.cat {
    qb.push(" AND category_id = ").push_bind(cat);
}
if let Some(q) = &filters.q {
    qb.push(" AND name LIKE '%' || ").push_bind(q).push(" || '%'");
}
// 排序方向来自枚举白名单（16 课 §6），不能绑定也不允许用户裸传
let ord = match filters.sort { Sort::Price => "price_cents", Sort::New => "created_at" };
qb.push(" ORDER BY ").push(ord).push(" DESC LIMIT ? OFFSET ?");
qb.push_bind(per_page).push((page - 1) * per_page);

let rows: Vec<Product> = qb.build_query_as().fetch_all(&pool).await?;
```

配套的**计数查询**（分页要 total）可以复用：

```rust
let mut qc: QueryBuilder<Sqlite> = QueryBuilder::new(
    "SELECT COUNT(*) FROM products WHERE 1=1");
// ...同样的 if let push_bind ...
let (total,): (i64,) = qc.build_query_as().fetch_one(&pool).await?;
```

> 抽象建议：把 WHERE 片段构造封装成 `fn push_filters(qb: &mut QueryBuilder<DB>, f: &Filters)`，主查询与计数查询复用，避免两处漂移。

## 5. 迁移工作流：sqlx migrate

```
rust-sqlx-demo/
├── migrations/
│   ├── 0001_init_shop.sql       ← up 迁移（已含建表+种子）
│   └── 0002_xxx.sql
```

```bash
# CLI 操作（需 sqlx-cli）
cargo sqlx migrate add create_coupons   # 生成 migrations/0002_create_coupons.sql
cargo sqlx migrate run                  # 执行未跑过的迁移
cargo sqlx migrate info                 # 查看状态

# 代码内嵌：编译期把迁移打进二进制，启动即迁移
sqlx::migrate!("./migrations").run(&pool).await?;
```

规则回顾（10 课 §7）：**只增不改**、已合并的迁移文件冻结、可重放（`_sqlx_migrations` 表记录版本）。

> `migrate!()` 启动自动迁移在**单实例部署**很爽；多实例并发启动会抢迁移锁（sqlx 已处理：获取锁失败会等待），但生产更常见做法是部署流水线里显式 `sqlx migrate run`，应用启动只校验版本。

## 6. 连接池调优与泄漏排查

```rust
let pool = SqlitePoolOptions::new()
    .max_connections(8)                        // 上限：SQLite=写串行，1~核数即可；
                                               // PG 经验：(核心数*2)+有效磁盘数 起步，
                                               // 大项目用 pgbouncer 池中间层
    .min_connections(2)                        // 预热，避免冷启动突发建连
    .acquire_timeout(Duration::from_secs(3))   // 拿不到连接最多等 3s → PoolTimedOut
    .idle_timeout(Duration::from_secs(600))
    .max_lifetime(Duration::from_secs(1800))   // 防止长命连接积累状态（PG 尤其重要）
    .connect_with(opts).await?;
```

池耗尽的三大原因排查：

1. **泄漏**：手动 `pool.acquire()` 后没 drop（`fetch_*(&pool)` 风格天然不泄漏）；
2. **慢查询占坑**：一个 5s 查询 × 并发 20 = 池 20 个连接全占 → EXPLAIN（13 课）；
3. **事务跨 await 挂太久**：`tx` 活着就占连接，事务里调外部 HTTP = 池慢慢干涸（12 课规范）。

监控：`pool.size()`（当前连接数）、`pool.num_idle()`（空闲数）打点上报。

## 7. 类型系统全景（PG 视角，SQLite 对照）

| SQL 类型 | PG Rust 映射 | SQLite 映射 |
|----------|--------------|-------------|
| INTEGER/BIGINT | i32/i64 | i64 |
| TEXT/VARCHAR | String | String |
| BOOLEAN | bool | bool |
| REAL/DOUBLE | f32/f64 | f64 |
| NUMERIC/DECIMAL | rust_decimal::Decimal（feature）| 不存在（用整数分）|
| TIMESTAMPTZ | chrono::DateTime\<Utc\>（feature）| String（ISO 文本约定）|
| DATE | chrono::NaiveDate | String |
| JSONB | sqlx::types::Json\<T\>（feature）| TEXT 存 JSON + json 函数 |
| UUID | uuid::Uuid（feature）| TEXT |
| BYTEA/BLOB | Vec\<u8\> | Vec\<u8\> |

feature 开关：`sqlx = { features = ["postgres", "chrono", "uuid", "rust_decimal", "json"] }`。

## 8. 常见坑

| 坑 | 说明 |
|---|---|
| **改了迁移文件没重新编译** | `sqlx::migrate!()` 是编译期宏，SQL 内容嵌在二进制里；改完迁移必须 `cargo build` 才生效（20 课实测踩坑）|
| CI 没有 DATABASE_URL 编译失败 | `cargo sqlx prepare` + 提交 `.sqlx/`，或 `SQLX_OFFLINE=true` |
| 宏里写动态 SQL | 宏只吃字面量；动态需求用 QueryBuilder |
| `"cnt!"` 乱加 | SUM/AVG 空集真返回 NULL；只有 COUNT(*)/COALESCE 才敢断言 |
| 迁移文件改历史 | 环境间 checksum 不一致直接报错；写新迁移纠正 |
| 事务里 await 外部 IO | 连接被占 → 池耗尽；事务只包 DB 操作 |
| min_connections > max_connections | 启动 panic；min ≤ max |
| PG 连接数暴涨 | 无池中间层时 max_connections 别照抄网上 100；用 (核×2)+磁盘起步压测 |

## 9. 练习

继续在 `rust-sqlx-demo` 里做：

1. 把 `src/basics.rs` 的某条查询改成 `query!` 宏版（注意 `.env` 与数据库存在），再故意把列名写错，`cargo build` 观察编译错误信息。
2. 给 `macros_demo.rs` 增加一个查询：每个分类的库存总值，`SUM` 用 `COALESCE` 包裹并用 `"total!"` 断言。
3. 实现 `list_products(pool, filters) -> (Vec<Product>, i64)`：min/max/cat/q 可选 + price/new 排序 + 分页，主查询与计数查询复用同一 `push_filters`。
4. 写 `create_user` 的完整错误处理：UNIQUE 冲突映射 `UsernameTaken`（用 `Database::constraint()` 判断）。
5. 压力测试池：写一个循环 50 个并发任务各 sleep 100ms 再查询的程序，把 `max_connections` 设为 4，观察 PoolTimedOut 出现的条件。

<details><summary>答案要点</summary>

```rust
// 2.
let rows = sqlx::query_as!(
    CatStock,
    r#"SELECT c.name AS category, COALESCE(SUM(p.stock), 0) AS "total!"
       FROM categories c LEFT JOIN products p ON p.category_id = c.id
       GROUP BY c.id, c.name ORDER BY total DESC"#
).fetch_all(pool).await?;

// 3. 骨架
fn push_filters(qb: &mut QueryBuilder<Sqlite>, f: &Filters) {
    if let Some(min) = f.min { qb.push(" AND price_cents >= ").push_bind(min); }
    if let Some(max) = f.max { qb.push(" AND price_cents <= ").push_bind(max); }
    if let Some(cat) = f.cat { qb.push(" AND category_id = ").push_bind(cat); }
    if let Some(q) = &f.q    { qb.push(" AND name LIKE '%' || ").push_bind(q).push(" || '%'"); }
}
// 主查询: QueryBuilder::new("SELECT id,name,price_cents,stock FROM products WHERE 1=1")
//   → push_filters → push ORDER/LIMIT/OFFSET → build_query_as
// 计数查询: QueryBuilder::new("SELECT COUNT(*) FROM products WHERE 1=1")
//   → push_filters → build_query_as::<(i64,)>

// 4.
match sqlx::query("INSERT INTO users (username,email) VALUES (?1,?2)")
    .bind(&u).bind(&e).execute(pool).await
{
    Ok(_) => Ok(()),
    Err(sqlx::Error::Database(db))
        if db.constraint().map(|c| c.ends_with("username")).unwrap_or(false)
        => Err(AppError::UsernameTaken),
    Err(e) => Err(e.into()),
}

// 5. 50 并发 × (100ms 持连接 + 查询)，max=4 → 约 50/4×100ms ≈ 1.25s 内完成则无超时；
//    把 acquire_timeout 降到 200ms 或把持连接时间加长 → 观察到 PoolTimedOut。
```
</details>

---

← 上一课：[16 · Rust sqlx 入门](16-Rust-sqlx入门.md) | 下一课：[18 · Diesel ORM](18-Rust-Diesel.md) · [返回目录](../README.md) · 配套代码：[`rust-sqlx-demo/`](../rust-sqlx-demo/)
