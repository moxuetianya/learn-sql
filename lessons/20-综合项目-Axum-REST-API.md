# 20 · 综合项目：Axum + sqlx REST API

> 本课目标：把前 19 课的所有知识组装成一个生产级形态的后端服务：分层架构、统一错误映射、事务下单、动态筛选、嵌套查询、报表接口。配套**完整可运行项目：`rust-shop-api/`**（已通过全部端点 curl 实测）。

---

## 1. 项目总览

```
rust-shop-api/
├── Cargo.toml                 # axum + sqlx + thiserror + tracing
├── .env                       # DATABASE_URL（供 sqlx 宏与运行时）
├── migrations/
│   └── 0001_init_shop.sql     # 建表 + 种子数据（与教程 shop 库一致）
└── src/
    ├── main.rs                # 组装：池/路由/监听（不写业务）
    ├── state.rs               # AppState（池的共享）
    ├── error.rs               # AppError → HTTP 状态码统一映射
    ├── models.rs              # DTO：请求/响应 + FromRow 行映射
    └── routes/
        ├── products.rs        # 列表(QueryBuilder) 详情(JOIN+聚合) 创建(RETURNING)
        ├── orders.rs          # 事务下单(防超卖) 详情(json_agg)
        └── stats.rs           # 月度营收(窗口函数)
```

| API | 方法 | 用到的课程 |
|-----|------|-----------|
| `/api/health` | GET | 06 |
| `/api/products?min&max&cat&q&page&per_page` | GET | 03/05/13（筛选+分页+索引）、17（QueryBuilder）|
| `/api/products/{id}` | GET | 05/06（JOIN+聚合+LEFT JOIN 计数）|
| `/api/products` | POST | 10/11（约束+RETURNING）、17（唯一冲突→409）|
| `/api/orders` | POST | 12（事务+条件更新防超卖）、16（批量插入+泛型 Executor）|
| `/api/orders/{id}` | GET | 15（json_group_array 嵌套输出）|
| `/api/stats/revenue` | GET | 07/09（CTE + LAG 窗口函数环比）|

```bash
cd rust-shop-api
cargo run          # http://127.0.0.1:3000，自动迁移+种子数据
```

## 2. 架构分层：小而清晰的边界

```
HTTP 请求
  │ axum 提取器（Path/Query/Json）      ← 请求即类型
  ▼
handler (routes/*)
  │ 调 SQL（sqlx）→ DTO                ← 业务编排，不写 HTTP 细节
  ▼
AppError (error.rs)
  │ IntoResponse                       ← 错误即状态码
  ▼
HTTP 响应（Json<DTO>）
```

三条纪律：

1. **handler 不碰 `StatusCode` 之外的 HTTP 细节**（映射集中在 error.rs）；
2. **SQL 与 DTO 就近对齐**（本课体量不分 repo 层；超过 ~5 张表的业务再加 `repo/` 层）；
3. **main.rs 只组装**：改业务永远不进 main。

## 3. 统一错误处理（17 课 §3 的完整落地）

```rust
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("资源不存在")]                NotFound,
    #[error("商品 {0} 库存不足")]         OutOfStock(i64),
    #[error("重复创建：{0}")]             Conflict(String),
    #[error(transparent)]                Db(#[from] sqlx::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, msg) = match &self {
            AppError::NotFound       => (StatusCode::NOT_FOUND, ...),
            AppError::OutOfStock(_)  => (StatusCode::CONFLICT, ...),
            AppError::Conflict(_)    => (StatusCode::CONFLICT, ...),
            AppError::Db(e) => match e {
                sqlx::Error::RowNotFound => (StatusCode::NOT_FOUND, ...),
                // SQLite UNIQUE 违反码 2067；PG 是 23505 —— 换库只改这一处
                sqlx::Error::Database(db) if db.code().as_deref() == Some("2067")
                                             => (StatusCode::CONFLICT, ...),
                sqlx::Error::PoolTimedOut => (StatusCode::SERVICE_UNAVAILABLE, ...),
                _ => { tracing::error!("db error: {e:?}");   // 500：细节进日志不进响应
                       (StatusCode::INTERNAL_SERVER_ERROR, ...) }
            },
        };
        (status, Json(json!({ "error": msg }))).into_response()
    }
}
```

**实测对照表**（全部用 curl 验证过）：

| 场景 | 响应 |
|------|------|
| `GET /api/products/999` | `404 {"error":"资源不存在"}` |
| `POST /api/orders` 商品 21 买 999 件 | `409 {"error":"商品 21 库存不足"}` |
| `POST /api/orders` items 为空 | `409 {"error":"重复创建：订单明细不能为空"}` |
| `POST /api/products` 重名 | `409 {"error":"唯一约束冲突"}`（UNIQUE 约束兜底）|
| `POST /api/orders` product_id=999 | `404`（整单回滚，不留半成品）|

## 4. 下单事务：全课程知识的巅峰 10 行

```rust
let mut tx = st.pool.begin().await?;

// ① 批量查价（泛型 Executor：函数同时接受 &Pool 和 &mut Connection，16 课）
let products = get_products_by_ids(&mut *tx, &ids).await?;

// ② 条件扣库存防超卖（12 课：UPDATE ... WHERE stock >= n）
for item in &req.items {
    let res = sqlx::query("UPDATE products SET stock = stock - ?1
                           WHERE id = ?2 AND stock >= ?1") ...
    if res.rows_affected() == 0 { return Err(AppError::OutOfStock(item.product_id)); }
}   // ↑ 提前 return → tx drop → 自动回滚，①② 全部撤销

// ③ 建订单拿自增 id（11 课 RETURNING）
let (order_id,) = sqlx::query_as("INSERT INTO orders ... RETURNING id") ...;

// ④ 批量明细（16 课 push_values，一条多值 INSERT）
qb.push_values(&req.items, |mut b, item| { ... }).build().execute(&mut *tx).await?;

tx.commit().await?;
```

**验证方式**（动手试）：

```bash
# 下单前库存
sqlite3 rust-shop-api/api.db "SELECT stock FROM products WHERE id=17"   # 100
curl -X POST localhost:3000/api/orders -H 'Content-Type: application/json' \
  -d '{"user_id":2,"items":[{"product_id":17,"quantity":2},{"product_id":18,"quantity":1}]}'
# → {"order_id":21,"total_cents":30700}
sqlite3 rust-shop-api/api.db "SELECT stock FROM products WHERE id=17"   # 98 ✔

# 超卖回滚验证：买 999 件失败后库存不变
curl -X POST localhost:3000/api/orders ... -d '{"user_id":2,"items":[{"product_id":21,"quantity":999}]}'
# → 409；库存仍为原值 ✔（回滚成功）
```

## 5. 列表接口：动态筛选 + 分页的正确姿势

```rust
// 17 课 §4 的生产化：push_filters 复用于主查询与 COUNT，永不漂移
fn push_filters<'a>(qb: &mut QueryBuilder<'a, Sqlite>, p: &'a ProductQuery) {
    if let Some(min) = p.min { qb.push(" AND p.price_cents >= ").push_bind(min); }
    if let Some(max) = p.max { qb.push(" AND p.price_cents <= ").push_bind(max); }
    if let Some(cat) = p.cat { qb.push(" AND p.category_id = ").push_bind(cat); }
    if let Some(q)   = &p.q  { qb.push(" AND p.name LIKE '%' || ").push_bind(q).push(" || '%'"); }
}
```

三个容易漏掉的细节（源码都有注释）：

1. `per_page.clamp(1, 100)`——**别信客户端**，per_page=100000 就是 DoS；
2. `page.max(1)` 且 `offset = (page-1)*per_page`——防负数偏移；
3. 生命周期 `'a` 把绑定值与 QueryBuilder 的 args 绑定——**借用检查器替你保证绑定值活到执行那一刻**。

```bash
curl "localhost:3000/api/products?min=50000&max=150000&q=Pro"
# {"items":[Razer Viper V2 Pro / Keychron Q1 Pro / Logitech G Pro X],"total":3,...}
```

## 6. 详情接口：JOIN + LEFT JOIN 聚合计数

```sql
-- 06 课：LEFT JOIN 后必须 COUNT(右表列)，没评价的商品 review_cnt=0 而不是 1
SELECT p.id, p.name, c.name AS category, p.price_cents, p.stock,
       COUNT(r.id) AS review_cnt, ROUND(AVG(r.rating),2) AS avg_rating
FROM products p
JOIN categories c ON c.id = p.category_id
LEFT JOIN reviews r ON r.product_id = p.id
WHERE p.id = ?1
GROUP BY p.id, ...
```

```bash
curl localhost:3000/api/products/17
# {"id":17,"name":"《Rust 程序设计语言》","review_cnt":2,"avg_rating":4.5,...}
```

`fetch_optional` + `ok_or(AppError::NotFound)`——主键查询的 404 语义（16 课 fetch_one vs fetch_optional）。

## 7. 订单详情：数据库直吐嵌套 JSON

```sql
-- 15 课 json_group_array + FILTER：一条 SQL，无 N+1，无 Rust 端组装
SELECT o.id, u.username, o.status, ...,
       COALESCE(json_group_array(json_object(...))
                FILTER (WHERE oi.product_id IS NOT NULL), '[]') AS items
FROM orders o JOIN users u ... LEFT JOIN order_items oi ... LEFT JOIN products p ...
WHERE o.id = ?1 GROUP BY ...
```

Rust 侧收 `String` 再 `serde_json::from_str`（SQLite JSON 函数返回 TEXT；PG 的 jsonb 列可直接映射 `serde_json::Value`）。`COALESCE(..., '[]')` 兜底空明细订单——**LEFT JOIN 聚合的空组是 NULL 而不是 `[]`**。

## 8. 报表接口：窗口函数直达 HTTP

```bash
curl localhost:3000/api/stats/revenue
# [{"month":"2025-02","revenue":949500,"prev_revenue":null,"diff":null},
#  {"month":"2025-03","revenue":2100200,"prev_revenue":949500,"diff":1150700}, ...]
```

07 课 CTE + 09 课 `LAG` 原样搬进 handler——**你在 SQL 课学的每条语句都是后端接口的即战力**。

## 9. 生产化清单（从 demo 到上线还差什么）

| 事项 | 现状 | 生产建议 |
|------|------|---------|
| 配置 | 硬编码 3000/api.db | env/配置文件 + `？` 启动校验 |
| 日志 | tracing 基础 | tower-http TraceLayer（每个请求耗时）+ 慢查询日志 |
| 认证 | 无 | JWT 中间件 + 角色检查（admin 才能建商品）|
| 参数校验 | clamp/manual | validator crate 或 newtype |
| 幂等 | 状态机条件更新 | 加 Idempotency-Key 表（11 课方案1）|
| 软删除 | 无 | deleted_at + 查询统一过滤 |
| 测试 | 无（本课练习补）| 见下方 §10 |
| 连接池监控 | 无 | pool.size()/num_idle() 打点 |
| 迁移 | 启动时自动跑 | 流水线显式 `sqlx migrate run`（多实例抢锁）|

## 10. 常见坑（本项目实测踩过/防过的）

| 坑 | 后果 | 本项目的防线 |
|---|---|---|
| **改了迁移文件没重新编译** | `sqlx::migrate!()` 是**编译期宏**，二进制里嵌的还是旧 SQL——运行时看到的 schema 是旧版！改迁移必须 `cargo build` | 实测踩过：改 UNIQUE 约束后未编译，重启依然无约束 |
| 事务里用 `&pool` 而非 `&mut *tx` | 扣库存跑在另一个连接，防超卖失效 | 下单全程 `&mut *tx` |
| 循环单条查价 | N+1（13 课） | `IN (...)` 一条批量查 |
| 循环单条插明细 | N+1 写入 | `push_values` 多值 INSERT |
| per_page 不设上限 | 客户端一把拉全表 | `clamp(1, 100)` |
| UNIQUE 冲突直接 500 | 前端无法区分"重名"和"炸了" | error.rs 按错误码映射 409 |
| 空订单漏校验 | 建出 0 元订单 | 入口 `items.is_empty()` 拒绝 |
| LEFT JOIN 计数用 COUNT(*) | 无评价商品数出 1 | `COUNT(r.id)` |

## 11. 练习（项目扩展路线）

按难度递增，每个都只动一两个文件：

1. **DELETE /api/products/{id}**：软删除（加 `deleted_at` 列走迁移 0002），所有列表/详情过滤已删商品。
2. **PATCH /api/orders/{id}**：状态机流转 pending→paid→shipped→completed（12 课条件更新；非法流转返回 409）。
3. **GET /api/users/{id}/orders**：用户订单列表（分页 + 每单含 items 嵌套——`json_group_array` 聚合两层）。
4. **并发压测下单**：写一个 tokio 程序 100 个并发任务同时买最后 1 件库存，验证**恰好成功 1 个**（条件更新的原子性），其余全是 409。
5. **切 PostgreSQL**：schema.pg.sql 建库 → sqlx feature 换 `postgres` → `ANY($1)` 替换 IN 拼接 → json_agg/jsonb_build_object 替换 SQLite 函数 → 错误码 2067→23505。体会"换库成本集中在哪"。
6. **加索引并验证**：给 `/api/products?cat=5&q=...` 建 `(category_id, name)` 复合索引，用 `EXPLAIN QUERY PLAN` 前后对比（13 课）。
7. 挑战：**评价接口** POST /api/products/{id}/reviews：一人一评（UNIQUE 冲突 409）、rating CHECK、事务内更新商品冗余评分列。

<details><summary>练习 4 参考实现（并发防超卖验证）</summary>

```rust
// tests/concurrent_order.rs 思路（或独立 bin）
// 前置：把商品 21 库存设为 1
// UPDATE products SET stock = 1 WHERE id = 21;
let mut handles = vec![];
for i in 0..100 {
    let url = "http://127.0.0.1:3000/api/orders".to_string();
    handles.push(tokio::spawn(async move {
        let client = reqwest::Client::new();
        client.post(&url).json(&serde_json::json!({
            "user_id": (i % 8) + 1,
            "items": [{"product_id": 21, "quantity": 1}]
        })).send().await.unwrap().status()
    }));
}
let statuses = futures::future::join_all(handles).await;
let ok = statuses.iter().filter(|s| s.as_ref().unwrap() == &200).count();
let conflict = statuses.iter().filter(|s| s.as_ref().unwrap() == &409).count();
assert_eq!(ok, 1);          // 恰好 1 单成功
assert_eq!(conflict, 99);   // 其余全部 409
// 库存恰好为 0，没有超卖，也没有少卖
```

（reqwest 需加入 dev-dependencies；本仓库未内置，作为练习自己加。）
</details>

---

## 恭喜毕业 🎓

到这里你已经走完：**SQL 语言核心 → 事务与性能 → 建模与数据库特性 → sqlx/Diesel/SeaORM → 完整后端项目**。

后续方向：

- **深入两个主力框架的内幕**：[21 · sqlx 深度剖析](21-sqlx深度剖析.md)（连接池/宏/语句缓存）、[22 · Diesel 深度剖析](22-Diesel深度剖析.md)（表达式树/类型级 DSL）；
- **PostgreSQL 深入**：JSONB 高级查询、分区表、pg_stat_statements、逻辑复制；
- **性能工程**：把 13 课的清单用到真实慢查询上，学 `pg_stat_statements`/`EXPLAIN (ANALYZE, BUFFERS)`；
- **分布式数据**：读写分离、分库分表、一致性方案（《DDIA》——shop 库里就有它）；
- 回头把每个课程的练习重做一遍——**SQL 是肌肉记忆，不是知识**。

---

← 上一课：[19 · SeaORM](19-Rust-SeaORM.md) · [返回目录](../README.md) · 配套项目：[`rust-shop-api/`](../rust-shop-api/)
