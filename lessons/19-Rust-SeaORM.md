# 19 · SeaORM

> 本课目标：掌握 SeaORM 的 Entity/ActiveModel 体系、关系定义、事务与迁移，理解它与 sqlx/Diesel 的定位差异。SeaORM 构建在 sqlx 之上——"喜欢 ActiveRecord 风格又不想丢掉异步生态"的选择。

---

## 1. 定位：sqlx 之上的 ActiveRecord 层

```
Diesel:  独立体系（表达式树 DSL，同步为主）
SeaORM:  sqlx 之上的一层实体框架（异步原生，ActiveRecord 风格）
sqlx:    直接写 SQL（本教程主线）
```

- Entity struct ↔ 表，ActiveModel ↔ 待写入/更新的行；
- 关系在 Rust 里声明（`Related` trait），查询时 `find_also_related` / `find_with_related`；
- 底层就是 sqlx——`Model` 可以 `.into_active_model()`，需要时随时下沉到原生 SQL。

**谁适合它**：从 Rails/Django/Node(Sequelize/Prisma) 转 Rust 的团队；CRUD 占比高、SQL 较简单、想要实体层惯用法的项目。报表级 SQL、窗口函数密集的场景还是回 sqlx 写原生。

## 2. 建立项目

```toml
[dependencies]
sea-orm = { version = "1", features = ["sqlx-sqlite", "runtime-tokio", "macros"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

```rust
// 连接（底层就是 sqlx 的池）
use sea_orm::{Database, DatabaseConnection};
let db: DatabaseConnection = Database::connect("sqlite://demo.db?mode=rwc").await?;
```

迁移用 sea-orm-cli（或直接复用 sqlx 迁移——SeaORM 不绑定迁移工具）：

```bash
cargo install sea-orm-cli
sea-orm-cli migrate generate init        # 生成 migration/src/*.rs
sea-orm-cli migrate up -d migration     # 执行
sea-orm-cli generate entity -o src/entities   # 从库反向生成 Entity
```

## 3. Entity：实体定义

```rust
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "products")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub category_id: i64,
    pub name: String,
    pub price_cents: i64,
    pub stock: i64,
    pub created_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::category::Entity",
        from = "Column::CategoryId",
        to = "super::category::Column::Id"
    )]
    Category,
}

impl Related<super::category::Entity> for Entity {
    fn to() -> RelationDef { Relation::Category.def() }
}

impl ActiveModelBehavior for ActiveModel {}   // 默认行为（钩子位）
```

三种"模型"各司其职：

| 类型 | 用途 | 可空列语义 |
|------|------|-----------|
| `Model` | 查询结果（不可变）| `Option<T>` |
| `ActiveModel` | 插入/更新载体 | `ActiveValue::Set/NotGiven/Unchanged` |
| `EntityModel` 派生 | 声明表结构 | —— |

## 4. CRUD 四连（ActiveModel 风格）

```rust
use sea_orm::*;

// ---- CREATE ----
let np = product::ActiveModel {
    category_id: Set(1),
    name: Set("Nuphy Field75".into()),
    price_cents: Set(109_900),
    stock: Set(10),
    ..Default::default()          // 其余 NotGiven（默认值/自增由库处理）
};
let created: product::Model = np.insert(&db).await?;   // INSERT ... RETURNING *

// ---- READ ----
use sea_orm::QueryFilter;
let p: Option<product::Model> = product::Entity::find_by_id(1).one(&db).await?;

let list: Vec<product::Model> = product::Entity::find()
    .filter(product::Column::PriceCents.gte(50_000))
    .filter(product::Column::CategoryId.eq(1))
    .order_by_desc(product::Column::PriceCents)
    .limit(5)
    .all(&db).await?;

// ---- UPDATE：Model → ActiveModel → 改字段 ----
let mut am: product::ActiveModel = p.unwrap().into();
am.price_cents = Set(99_900);
let updated: product::Model = am.update(&db).await?;

// 只更新个别字段（其余 Unchanged 不进 SET）：
// let am = product::ActiveModel { id: Set(3), stock: Set(0), ..Default::default() };
// am.update(&db)  → UPDATE products SET stock = 0 WHERE id = 3

// ---- DELETE ----
product::Entity::delete_by_id(3).exec(&db).await?;        // 删 1 行
// am.delete(&db)                                          // ActiveModel 删除

// ---- UPSERT ----
let np = product::ActiveModel { /* ... */ ..Default::default() };
product::Entity::insert(np.clone())
    .on_conflict(
        OnConflict::column(product::Column::Name)
            .update_columns((product::Column::PriceCents, product::Column::Stock))
            .to_owned(),
    )
    .exec(&db).await?;
```

## 5. 关系查询（ActiveRecord 的甜点）

```rust
// belongs_to：商品 + 它的分类（LEFT JOIN 一条查询）
let rows: Vec<(product::Model, Option<category::Model>)> =
    product::Entity::find()
        .find_also_related(category::Entity)
        .all(&db)
        .await?;

// has_many：分类 → 全部商品
let cats_with_products: Vec<(category::Model, Vec<product::Model>)> =
    category::Entity::find()
        .find_with_related(product::Entity)     // 一次查询分组打包，无 N+1
        .all(&db)
        .await?;

// 手写 join 取任意列
use sea_orm::QuerySelect;
let rows = order::Entity::find()
    .join(JoinType::InnerJoin, sea_orm::JoinOn::new(
        users::Relation::Order.def().rev(),     // 反向关系
    ))
    .all(&db).await?;
```

## 6. 动态查询、分页、聚合

```rust
// 动态筛选（对应 17 课 QueryBuilder 的场景）
use sea_orm::QueryFilter;
let mut q = product::Entity::find();
if let Some(min) = min_price { q = q.filter(product::Column::PriceCents.gte(min)); }
if let Some(c) = cat        { q = q.filter(product::Column::CategoryId.eq(c)); }
if let Some(kw) = q_kw      { q = q.filter(product::Column::Name.contains(kw)); }

// 分页器（自动帮你跑 COUNT + LIMIT/OFFSET 两条查询）
use sea_orm::{PaginatorTrait, QueryOrder};
let pager = q.order_by_desc(product::Column::PriceCents).paginate(&db, 20);
let total: u64 = pager.num_items().await?;             // SELECT COUNT(*)
let page1: Vec<product::Model> = pager.fetch_page(0).await?;

// 聚合
use sea_orm::QueryTrait;
let avg: Option<f64> = product::Entity::find()
    .select_only()
    .column_as(product::Column::PriceCents.avg(), "avg_price")
    .into_tuple::<Option<f64>>()
    .one(&db).await?;
```

## 7. 事务

```rust
let tx = db.begin().await?;                 // 底层即 sqlx 事务
let am = product::ActiveModel { /* ... */ ..Default::default() };
am.insert(&tx).await?;                      // 所有 exec 接受 &DatabaseConnection（事务也实现它）
// ...
tx.commit().await?;                         // 或 rollback()。drop 行为遵循 sqlx（回滚）
```

复杂订单场景推荐 SeaORM 提供的封装 `sea_orm::Transaction`：`db.transaction(|tx| async move { ... }.boxed())` —— 闭包 Err 自动回滚（Diesel 同款风格）。

## 8. 逃生舱：随时下沉到 sqlx

```rust
// 原生 SQL（返回任意行）
let rows: Vec<(i64, String)> =
    sea_orm::raw_sql("SELECT id, name FROM products WHERE price_cents > ?")
        .bind_values(vec![100_000])
        .into_tuple::<(i64, String)>()
        .all(&db).await?;

// 从 Model 拿到底层 sqlx 连接（极端场景）
```

**这是 SeaORM 的核心卖点：80% CRUD 用实体层，20% 复杂查询沉到 sqlx，一套池一种依赖。**（对照 18 课：Diesel 的 `sql_query` 逃生舱体验差得多，因为同步/异步体系割裂。）

## 9. 三框架速查总表

| | sqlx | Diesel | SeaORM |
|---|---|---|---|
| 抽象层级 | SQL 直写 | DSL→SQL | 实体→SQL |
| 类型安全 | 宏校验 | ★★★ 表达式树 | ★★ 实体列 |
| 异步 | ✅ | diesel-async | ✅（基于 sqlx）|
| 关系/预加载 | 手写 JOIN | join DSL | `find_with_related` |
| 迁移 | sqlx-cli | diesel-cli | sea-orm-cli（兼容）|
| 原生 SQL 逃生 | 本体 | 较差 | 好（raw_sql）|
| 心智负担 | 低 | 高 | 中 |
| 教程建议 | 必学（主线）| 理解思想 | 上手即会（有 ActiveRecord 经验 30 分钟）|

## 10. 常见坑

| 坑 | 说明 |
|---|---|
| ActiveModel 全字段 Set 再 update | 变成全列 UPDATE；只想改部分 → 从 Model::into() 转换（Unchanged）或 Default + Set 目标列 |
| 忘实现 Related / Relation 声明错 | find_related 编译错或 SQL 关联错；方向搞反用 `.rev()` |
| 分页 num_items 每页都查 COUNT | 高并发列表页可改为手写 count 或缓存总数 |
| contains() 全表扫 | 即 LIKE '%x%'，13 课教训照旧适用 |
| 把 SeaORM 当免 SQL 许可证 | 复杂报表/窗口函数还是 `raw_sql`；不懂 EXPLAIN 一样慢 |
| Entity 与迁移漂移 | 反向生成后手工 diff；CI 里加 schema 校验 |

## 11. 练习

（在 18 课的 `diesel_demo` 旁建 `sea_orm_demo`，或复用 demo.db）

1. 写 `find_products(min, cat, kw, page)`：动态筛选 + 分页（返回 `(Vec<Model>, u64)`）。
2. 写 `apply_coupon(user_id, code)` 幂等领券：`insert ... on_conflict do nothing`，用返回的主键判断是否首次。
3. `categories_with_products()`：一次查询返回分类及其前 3 个商品（`find_with_related` + limit 在子查询里……做得到吗？做不到的话说明为什么，并用两段式查询实现）。
4. 把 16 课 `place_order` 用事务闭包重写（SeaORM 版防超卖）。
5. 思考：SeaORM 的 `Model` 为什么设计成不可变，而更新必须转 ActiveModel？（提示：`ActiveValue::Unchanged` 决定 UPDATE 的 SET 列集——不可变 Model 是"读真相"，ActiveModel 是"写意图"）

<details><summary>答案要点</summary>

```rust
// 1.
let mut q = product::Entity::find();
if let Some(m) = min { q = q.filter(product::Column::PriceCents.gte(m)); }
if let Some(c) = cat { q = q.filter(product::Column::CategoryId.eq(c)); }
if let Some(k) = kw  { q = q.filter(product::Column::Name.contains(k)); }
let pager = q.paginate(&db, 20);
let total = pager.num_items().await?;
let items = pager.fetch_page(page).await?;
(items, total)

// 2.
let claim = coupon_claim::ActiveModel { user_id: Set(uid), code: Set(code), ..Default::default() };
let res = coupon_claim::Entity::insert(claim)
    .on_conflict(OnConflict::columns([coupon_claim::Column::UserId,
                                      coupon_claim::Column::Code])
        .do_nothing().to_owned())
    .exec(&db).await;
match res { Ok(r) => Some(r.last_insert_id), Err(_) => None }

// 3. find_with_related 不支持对 related 限条数（它会拉全部分组）。
//    两段式：先查分类，再 product::Entity::find()
//      .filter(product::Column::CategoryId.is_in(ids))
//      .order_by_asc(product::Column::PriceCents).all() 后 HashMap 分组取前3。
//    或者直接 raw_sql + 窗口函数（09 课 ROW_NUMBER）一条搞定。

// 4. tx 闭包：insert(claim) / update stock (ActiveModel 条件更新可用
//    product::Entity::update_many().col_expr(...) .filter(stock.gte(qty)) exec 拿 rows_affected。

// 5. 读侧不可变 → Model 可自由 clone/share 无副作用；
//    写侧 ActiveValue 三态（Set/NotGiven/Unchanged）精确表达"这列要不要进 SQL"。
//    若 Model 可变直接 save，框架无法区分"改过"与"没改"（Rails 的 dirty tracking 魔法
//    在 Rust 里显式化为类型——符合 Rust 的哲学）。
```
</details>

---

← 上一课：[18 · Diesel ORM](18-Rust-Diesel.md) | 下一课：[20 · 综合项目：Axum REST API](20-综合项目-Axum-REST-API.md) · [返回目录](../README.md)
