# 18 · Diesel ORM

> 本课目标：理解 Diesel 的 schema.rs 体系与表达式树类型安全，能完成 CRUD、过滤、连接、事务。Diesel 是 Rust 生态最强类型安全的 ORM——学习曲线陡，但回报是"拼错的列名根本编译不过"。
> 本课代码独立成篇，可直接建立项目跟跑（`diesel_demo/`）。

---

## 1. Diesel 的世界观：编译期 SQL 生成器

```
sqlx:    你写 SQL 字符串 ──运行时/宏校验──▶ 数据库
Diesel:  你用 Rust DSL 拼查询 ──编译期类型检查──▶ 生成 SQL ─▶ 数据库
```

- 表结构由 `diesel::table!` 宏声明（`schema.rs`，通常由 `diesel_cli` 从数据库生成）；
- 查询构造全程类型安全：`.filter(products::category_id.eq(6))` —— 列类型错了、表用错了，**编译失败**；
- 代价：思维模型重（DSL、BoxableExpression、JoinOnDsl……），动态查询组合比 sqlx 痛苦。

**选型建议回顾（README）**：领域逻辑复杂、静态查询为主、团队愿意投资学习 → Diesel；快速迭代、动态查询多、SQL 主导 → sqlx。

## 2. 建立项目

```bash
# diesel-cli（SQLite 后端）
cargo install diesel_cli --no-default-features --features sqlite
```

```toml
# Cargo.toml
[dependencies]
diesel = { version = "2", features = ["sqlite", "returning_clauses_for_sqlite_3_35"] }
# returning_clauses_for_sqlite_3_35: SQLite 3.35+ 才支持 RETURNING，要显式开
```

```bash
# 初始化与迁移
diesel setup                 # 创建 diesel.toml + migrations/ + 数据库（读 DATABASE_URL）
diesel migration generate create_products
diesel migration run         # 生成 schema.rs（diesel.toml 里配置 print_schema）
```

## 3. schema.rs：一切的起点

```rust
// src/schema.rs（diesel migration run 自动生成，或手写）
diesel::table! {
    products (id) {
        id -> Integer,
        category_id -> Integer,
        name -> Text,
        price_cents -> Integer,
        stock -> Integer,
        created_at -> Text,
    }
}

diesel::table! { users (id) { /* ... */ } }
diesel::joinable!(products -> categories (category_id));   // 声明外键关系
diesel::allow_tables_to_appear_in_same_query!(products, users, /* ... */);
```

`schema.rs` 描述"数据库长什么样"，你的 model struct 描述"行怎么映射到 Rust"：

```rust
// src/models.rs
use diesel::prelude::*;

#[derive(Queryable, Selectable)]          // 读：行 → struct
#[diesel(table_name = crate::schema::products)]
pub struct Product {
    pub id: i32,
    pub category_id: i32,
    pub name: String,
    pub price_cents: i32,
    pub stock: i32,
    pub created_at: String,
}

#[derive(Insertable)]                     // 写：struct → 行
#[diesel(table_name = crate::schema::products)]
pub struct NewProduct<'a> {
    pub category_id: i32,
    pub name: &'a str,
    pub price_cents: i32,
}
```

惯例：读用 `Queryable`（全字段），写用 `Insertable`（可空/默认列省略）——**两个 struct，不要复用**。

## 4. CRUD 四连

```rust
use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;

fn establish() -> SqliteConnection {
    SqliteConnection::establish("demo.db").expect("连接失败")
}

// ---- READ： SELECT ----
fn list_cheap(conn: &mut SqliteConnection) -> QueryResult<Vec<Product>> {
    use crate::schema::products::dsl::*;

    products                          // FROM products
        .select(Product::as_select()) // SELECT 全部列（Selectable 派生）
        .filter(price_cents.lt(100_000))   // WHERE price_cents < ?
        .filter(stock.gt(0).and(category_id.eq(1)))  // AND (stock>0 AND cat=1)
        .order(price_cents.desc())         // ORDER BY price_cents DESC
        .limit(5)                          // LIMIT 5
        .load(conn)?                       // 执行 → Vec<Product>
}

// 单行：.first(conn)?.optional()?  → Option<Product>（对应 16 课 fetch_optional）
fn by_id(conn: &mut SqliteConnection, pid: i32) -> QueryResult<Option<Product>> {
    use crate::schema::products::dsl::*;
    products.find(pid).first(conn).optional()
}

// ---- CREATE： INSERT + RETURNING ----
fn create(conn: &mut SqliteConnection, np: NewProduct) -> QueryResult<Product> {
    use crate::schema::products;
    diesel::insert_into(products::table)
        .values(&np)
        .returning(Product::as_returning())   // 依赖 returning_clauses feature
        .get_result(conn)?
}

// 批量：传 Vec<NewProduct>，一条语句
// diesel::insert_into(products::table).values(&vec![np1, np2]).execute(conn)?;

// ---- UPDATE ----
fn restock(conn: &mut SqliteConnection, pid: i32, add: i32) -> QueryResult<usize> {
    use crate::schema::products::dsl::*;
    diesel::update(products.find(pid))
        .set(stock.eq(stock + add))     // SET stock = stock + ?（表达式列自增）
        .execute(conn)?                 // 返回受影响行数（防超卖判断同 sqlx）
}

// 条件更新（状态机）：
// diesel::update(orders.find(oid).filter(status.eq("paid"))).set(status.eq("shipped"))

// ---- DELETE ----
fn delete(conn: &mut SqliteConnection, pid: i32) -> QueryResult<usize> {
    use crate::schema::products::dsl::*;
    diesel::delete(products.find(pid)).execute(conn)?
}

// UPSERT：on_conflict 对应 11 课 ON CONFLICT
// diesel::insert_into(products::table)
//     .values(&np)
//     .on_conflict(products::columns::name)
//     .do_update()
//     .set((price_cents.eq(np.price_cents), stock.eq(np.stock)))
//     .execute(conn)?;
```

DSL 方法 ↔ SQL 对照（左边是 Rust，右边是等价 SQL 谓词）：

| DSL | SQL |
|-----|-----|
| `.eq()` / `.ne()` | `=` / `<>` |
| `.gt() .ge() .lt() .le()` | `>` `>=` `<` `<=` |
| `.and() .or() .not()` | AND OR NOT |
| `.is_null() / .is_not_null()` | IS NULL / IS NOT NULL |
| `.like() / .ilike()` | LIKE / ILIKE |
| `.between(a,b) / .not_between` | BETWEEN |
| `.any(vals)` / `.all(vals)` | IN / = ALL |
| `.nullable()`（列上）| 让可空列映射 Option |

## 5. JOIN 与连接表

```rust
// 两表：订单 + 用户名
#[derive(Queryable)]
struct OrderWithUser {
    id: i32,
    status: String,
    username: String,
}

fn orders_with_user(conn: &mut SqliteConnection) -> QueryResult<Vec<OrderWithUser>> {
    use crate::schema::{orders, users};

    orders::table
        .inner_join(users::table)             // 需要 joinable! 声明或 .on 显式条件
        .select((orders::id, orders::status, users::username))
        .order(orders::id)
        .limit(10)
        .load(conn)?
}
// 复杂条件：.left_join(reviews::table.on(reviews::product_id.eq(products::id)))

// 连接表聚合（销量）：GROUP BY + sum
use diesel::dsl::{count, sum, max};
let rows: Vec<(i32, Option<i64>)> = order_items::table
    .group_by(order_items::product_id)
    .select((order_items::product_id, sum(order_items::quantity)))
    .load(conn)?;
```

## 6. 事务

```rust
let result = conn.transaction(|conn| {
    diesel::update(products::table.find(pid))
        .set(products::stock.eq(products::stock - qty))
        .filter(products::stock.ge(qty))          // WHERE stock >= qty（防超卖）
        .execute(conn)?;
    if /* affected == 0 */ { return Err(...) }     // 闭包返回 Err → 自动 ROLLBACK
    diesel::insert_into(orders::table).values(...).execute(conn)?;
    Ok(order_id)
});   // Ok → COMMIT；Err → ROLLBACK。比 sqlx 更显式的"事务闭包"风格
```

注意 Diesel 是**同步** API（连接不是 Send+Sync 共享，每个任务要独占连接或用 `r2d2` 池）；异步用 `diesel-async` crate（API 相似，`AsyncConnection::transaction`）。

## 7. 动态查询：条件可选拼接

Diesel 的"动态"难点在类型（每个 filter 组合都是不同类型）。官方姿势——**累加同一个可变查询变量**：

```rust
fn list_products(
    conn: &mut SqliteConnection,
    min_price: Option<i32>,
    cat: Option<i32>,
) -> QueryResult<Vec<Product>> {
    use crate::schema::products::dsl::*;

    let mut q = products.into_boxed();        // BoxedQuery：擦除类型，统一变量
    if let Some(min) = min_price {
        q = q.filter(price_cents.ge(min));
    }
    if let Some(c) = cat {
        q = q.filter(category_id.eq(c));
    }
    q.order(price_cents.desc()).limit(20).load(conn)
}
```

`into_boxed()` ≈ sqlx 的 QueryBuilder（17 课），但只能在单表上如意；跨表动态 join 组合仍是 Diesel 公认痛点。

## 8. Diesel vs sqlx 决策表

| 维度 | Diesel | sqlx |
|------|--------|------|
| 类型安全 | ★★★ 表达式树全编译期 | ★★ 宏校验（或运行时）|
| 写原生 SQL | 不便（要用 `sql_query` 逃生舱）| 原生体验 |
| 动态查询 | boxed 可用但繁琐 | QueryBuilder 自然 |
| 异步 | diesel-async（第二公民）| 原生 async |
| 迁移 | diesel_cli 成熟 | sqlx-cli 成熟 |
| 学习曲线 | 陡（文档阅读量 ×3）| 平缓（会 SQL 就会大半）|
| 适合 | 静态重逻辑、强约束团队 | 绝大多数后端（本教程主线）|

## 9. 常见坑

| 坑 | 说明 |
|---|---|
| 改了迁移忘了重新生成 schema.rs | model 与 schema 漂移 → 编译错乱；`diesel migration run` 一步到位 |
| 想复用一个 struct 又读又写 | Queryable 要求全字段；用 QueryableByName + #[sqlite] 或拆两个 struct |
| SQLite RETURNING 不生效 | feature `returning_clauses_for_sqlite_3_35` 没开 |
| 连接跨 await | 同步版不能进 async fn 里的 .await 之间共享；用 r2d2 或 diesel-async |
| 动态拼 filter 类型地狱 | 忘了 `into_boxed()`，用 if/else 生成三种类型 |
| `.optional()` 忘加 | first() 0 行直接 Err(NotFound) |

## 10. 练习

建立 `diesel_demo` 项目（复用 shop 库的 products/orders 表，写一个最小 schema.rs + 3 张表迁移）：

1. 实现 `count_by_category(cat_id) -> i64`（`select(count(...)) .first`）。
2. 实现 `top_expensive(max: Option<i32>) -> Vec<Product>`（into_boxed + 可选过滤 + 排序）。
3. 把 16 课的 `place_order`（扣库存→建订单→写明细）用 `conn.transaction` 重写。
4. 写 join：订单 id + 用户名 + 商品名（orders ⋈ users，order_items ⋈ products，多表 select 元组）。
5. 思考：为什么 Diesel 的 `filter(price_cents.lt("abc"))` 编不过？（提示：`Text` 列与 `Integer` 比较——类型系统在编译期替你挡下了 SQL 里 `WHERE price < 'abc'` 的运行时错误）

<details><summary>答案要点</summary>

```rust
// 1.
use diesel::dsl::count;
let n: i64 = products::table
    .filter(products::category_id.eq(cat_id))
    .select(count(products::id))
    .first(conn)?;

// 2.
let mut q = products::table.into_boxed();
if let Some(m) = max { q = q.filter(products::price_cents.le(m)); }
let rows = q.order(products::price_cents.desc()).load::<Product>(conn)?;

// 3. 事务闭包：闭包内三步写操作，Err 短路自动回滚（§6 代码扩展 order_items 插入即可）。

// 4.
#[derive(Queryable)]
struct Row { id: i32, username: String, product: String }
let rows: Vec<Row> = orders::table
    .inner_join(users::table)
    .inner_join(order_items::table.on(order_items::order_id.eq(orders::id)))
    .inner_join(products::table.on(products::id.eq(order_items::product_id)))
    .select((orders::id, users::username, products::name))
    .load(conn)?;
// 注意：多表 join 需 allow_tables_to_appear_in_same_query! 全部声明

// 5. Diesel 列类型是幻影类型：price_cents: Integer 只能与 i32 表达式比较，
//    "abc" 是 &str → 编译错误。SQL 的运行时隐式转换错误被提前到编译期——
//    这正是"最强类型安全"的含义。
```
</details>

---

← 上一课：[17 · Rust sqlx 进阶](17-Rust-sqlx进阶.md) | 下一课：[19 · SeaORM](19-Rust-SeaORM.md) · [返回目录](../README.md)
