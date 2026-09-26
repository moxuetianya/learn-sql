# 22 · Diesel 深度剖析：表达式树与类型级 DSL 的内幕

> 进阶专题。前置：[18 课](18-Rust-Diesel.md)（会用 DSL）+ 建议[21 课](21-sqlx深度剖析.md)对照阅读。
> 本课目标：理解 `table!` 宏生成了什么、`.eq()` 为什么能编译期拒绝类型错误、GROUP BY 类型安全是怎么实现的、表达式树如何变成 SQL。学完你能读懂 Diesel 的报错信息、调试生成的 SQL、权衡编译时间与灵活性。

---

## 1. 核心思想：查询是"类型"，SQL 是"证据"

sqlx 的做法是**把 SQL 文本交给数据库校验**（21 课 DESCRIBE）；Diesel 的做法截然相反：

```
sqlx:    你的字符串 ──编译期问数据库──▶ 类型
Diesel:  你的 Rust DSL ──本身就是 SQL 的抽象语法树（AST）──▶ 永远合法
```

```rust
// 这不是"调用"，是在【构造一个类型】：
let expr = products::price_cents.lt(100_000);
//            ↑ 表达式树节点        ↑ 表达式树节点        ↑ 表达式树节点
// 实际类型（可在 IDE 里看到）：
//   Lt<products::columns::price_cents, diesel::expression::bound::Bound<Integer, i64>>
//   —— 一个零大小的类型，唯一作用是"在类型层面携带 SQL 结构信息"
```

**整个查询是一个嵌套的零大小类型**，`where` 条件是类型，`SELECT` 列表是类型，JOIN 关系是类型。执行时 `to_sql()` 递归遍历这棵"类型的树"拼出 SQL 字符串 + 绑定参数。

推论（Diesel 三大特征）：

1. **构建查询零运行时开销**：AST 全是 ZST，没有分配、没有解析；
2. **改不动的东西拼不进树**：动态 SQL（运行时才知道的条件组合）是天然短板；
3. **编译时间换安全**：每个查询组合都是新类型（单态化），大型项目编译膨胀明显。

## 2. `table!` 宏展开解剖

```rust
diesel::table! {
    products (id) {
        id -> Integer,
        category_id -> Integer,
        name -> Text,
        price_cents -> Integer,
    }
}
```

大致展开为（简化示意）：

```rust
pub mod products {
    use diesel::sql_types::*;   // Integer、Text 等幻影类型（ZST，只在类型层存在）

    pub struct table;           // 表本身（ZST）
    impl QuerySource for table { ... }
    impl AsChangeset for table { ... }

    pub mod columns {           // 每列一个 ZST 单元结构体
        pub struct id;
        pub struct category_id;
        pub struct name;
        pub struct price_cents;

        impl Expression for id {
            type SqlType = Integer;        // ← 列的"SQL 类型"记录在关联类型里
        }
        impl Column for id {
            type Table = super::table;     // ← 列属于哪张表
            const NAME: &str = "id";
        }
        // star()、select_expression、AppearsOnTable<...> 一并实现
    }

    impl table {
        pub fn star() -> ... { ... }       // products::*（全列）
        pub const NAME: &str = "products";
    }
}
```

**`sql_types::Integer` 与 Rust 的 `i32` 不是一回事**：

- `Integer`：ZST，存在于类型层，描述"数据库里的整数"；
- `i32`：运行时值类型；
- 桥梁：`impl AsExpression<Integer> for i32`（i32 可以作为 Integer 表达式）+ `Queryable<Integer> for i32`（Integer 列可以解码成 i32）。

## 3. `.eq()` 为什么能拒绝 `"abc"`：约束链逐层解析

```rust
products::price_cents.eq("abc")   // ❌ 编译错误（18 课练习 5）
```

`eq` 的签名（简化）：

```rust
// diesel::expression_methods::ExpressionMethods
fn eq<Rhs>(self, rhs: Rhs) -> Eq<Self, Rhs::Expression>
where Rhs: AsExpression<Self::SqlType>       // ★ 核心约束
{
    Eq(self, rhs.as_expression())
}
```

推导链：

```
Self = price_cents，其 SqlType = Integer
约束：Rhs = &str 必须 impl AsExpression<Integer>
事实：&str 只 impl AsExpression<Text>
结论：&str 不满足约束 → E0277，报错指向 missing the trait AsExpression<diesel::sql_types::Integer>
```

同理可以看懂其他类型错误：

| 写法 | 挂在哪一环 |
|---|---|
| `price_cents.eq("abc")` | `AsExpression<Integer>` 缺失 |
| `users::id.eq(products::id)` | SqlType 都是 Integer ✅ 编得过；但 `users::id.eq(users_id 列)` 类型一致就放行（**值域/语义错误类型系统管不了**——外键关系靠你，或靠 join DSL）|
| `.filter(某字符串列.eq(1))` | Text vs Integer 不匹配 |
| `.filter(5)` | `5` 不 impl `AppearsOnTable<products::table>`——整数裸值不是表达式 |

最后一行引出第二道防线：

```rust
// filter 的真实签名（简化）：
fn filter<Predicate>(self, predicate: Predicate) -> FilteredQuery<...>
where Predicate: AppearsOnTable<Self::FromClause>   // ★ 表达式必须能出现在这张表上
                                                  //   + NonAggregate（不在聚合上下文）
```

- `AppearsOnTable`：`products::name` 不能出现在 `users` 的 WHERE 里（列 ↔ 表绑定）；
- 这也是为什么 **JOIN 之后过滤另一表的列要先 join**——类型系统强制了 SQL 的作用域规则。

## 4. GROUP BY 类型安全：Diesel 独有的第三道防线

```rust
// 05 课的血案在 SQL 里是静默错误，在 Diesel 里是编译错误：
products::table
    .select((products::name, diesel::dsl::count_star()))
    //          ↑ 裸列              ↑ 聚合
    .load::<(String, i64)>(conn)?;
// ❌ 编译错误（大意）: `name` must appear in the GROUP BY clause
//    或：the expression is not aggregate... NonAggregate/ValidGrouping
```

机制：`select` 要求每个表达式 impl `ValidGrouping<G>`，其中 `G` 是查询的"分组上下文"：

```
未 group_by：G = NoGroupByClause
    裸列 impl ValidGrouping<NoGroupByClause> ✅
    聚合(count_star) impl ValidGrouping<任何> ✅（聚合总是合法）
    组合 (裸列, 聚合)：裸列在"无分组"下与聚合共存 ❌

group_by(products::name) 后：G = GroupByClause<name>
    name impl ValidGrouping<GroupByClause<name>> ✅（自己就是分组键）
    (name, count_star()) 整体合法 ✅
```

**sqlx/SeaORM/任何字符串方案都做不到这一点**（数据库运行时才报错）——这是 Diesel 表达式树最深的价值：把 SQL 的语义规则编码进 trait 约束。

## 5. LEFT JOIN 的类型体操：`Nullable` 的自动提升

```rust
// 06 课：LEFT JOIN 后右表列可能为 NULL。SQL 是"悄悄变 NULL"，Diesel 是"类型上逼你处理"：
let rows: Vec<(String, Option<String>)> = categories::table
    .left_join(products::table)
    .select((categories::name, products::name.nullable()))   // ← 必须 .nullable()
    .load(conn)?;
```

机制：

```
products::name.SqlType = Text
left_join 后：QuerySource 变为 LeftJoin<categories, products>
 selectable 表达式约束：列必须 impl SelectableExpression<该 join 源>
 products::name 只 impl SelectableExpression<products>  ❌
 Nullable<wrapper> 包裹后：nullable() 产出 NullableExpression，
   impl SelectableExpression<LeftJoin<...>> ✅，SqlType 变为 Nullable<Text>
 解码端：Nullable<Text> ↔ Option<String>
```

**SQL 的隐式 NULL 语义被显式化到类型**——06 课所有"LEFT JOIN 后 NULL 的坑"（COUNT(*) 计数、IS NULL 判断）在 Diesel 里编译期就逼你想清楚。

## 6. 从 AST 到 SQL：`to_sql` 与 debug_query

```rust
use diesel::debug_query;

let q = products::table
    .select((products::id, products::name))
    .filter(products::price_cents.lt(100_000))
    .order(products::price_cents.desc())
    .limit(5);

// 不执行，只看生成的 SQL（调试必备）：
println!("{}", diesel::debug_query::<diesel::sqlite::Sqlite, _>(&q));
// SELECT `products`.`id`, `products`.`name` FROM `products`
// WHERE `products`.`price_cents` < ? ORDER BY ... DESC LIMIT ?
// VALUES 绑定单独展示（Postgres 后端显示 $1/$2）
```

执行路径（`.load()` 内部）：

```
1. QueryFragment::to_sql(ast, out)   递归遍历表达式树 → SQL 文本 + 占位符
2. 收集绑定向量（Bound<Integer, i64> 持有的 i64 → 00000064 小端字节）
3. 驱动执行（与 sqlx 同层的协议工作）
4. FromSqlRow/Queryable 按列反序列化 → 你的 struct
```

## 7. `into_boxed()`：类型擦除的十字路口

```rust
let mut q = products::table.into_boxed();   // BoxedSelectStatement
if let Some(min) = min_price { q = q.filter(...); }   // 每次赋值【同一个类型】✅
if let Some(cat) = cat      { q = q.filter(...); }
```

内幕对比：

| | 未 boxed | boxed |
|---|---|---|
| WHERE 的类型 | `And<And<...>>` 嵌套（每个组合一个类型）| `Box<dyn BoxableExpression<...>>` |
| 动态拼接 | 不可能（不同分支不同类型）| ✅ 顺序 filter |
| 开销 | 零（ZST）| 每条件一次堆分配（可忽略）|
| 类型安全保留度 | 100% | **列类型/表匹配仍检查**，但 GROUP BY 完整性等深约束减弱 |
| 编译产物 | 组合爆炸（2^n 类型）| 一个实例 |

选型经验：**静态查询用普通 DSL（全类型安全）；列表页筛选用 boxed（动态性）**——对应 sqlx 的静态 `query!` vs QueryBuilder，两者哲学完全同构（21 课 §4 的宏 vs 构造器）。

## 8. 逃生舱：`sql()` 与 `sql_query()`

```rust
use diesel::sql_types::*;
use diesel::dsl::sql;

// ① 片段级：dsl::sql 注入原生 SQL 片段（保留在 DSL 树里）
let rows = products::table
    .select((products::id, sql::<Nullable<Text>>("upper(name)")))
    .filter(sql::<Bool>("price_cents % 7 = 0"))
    .load::<(i32, Option<String>)>(conn)?;
//  sql::<T> 必须标注返回类型——绕过了推断，自己负责

// ② 绑定参数的片段
sql::<Bool>("price_cents > ").bind::<Integer, _>(50_000)

// ③ 整条级：sql_query 完全裸奔（等价 sqlx 的 query）
diesel::sql_query("SELECT id, name FROM products WHERE id IN (?, ?)")
    .bind::<Integer, _>(1).bind::<Integer, _>(3)
    .load::<HashMapRow>(conn)?;   // QueryableByName（按列名取）
```

**用 `sql()` 的纪律**：只用于 Diesel DSL 表达不了的东西（数据库特有函数、窗口 OVER、CTE……18 课已见复杂查询的痛）。混拼过多 = 失去使用 Diesel 的全部理由，不如换 sqlx。

## 9. 编译时间：表达式树的真实代价

症状与对策：

| 症状 | 原因 | 对策 |
|---|---|---|
| 动态查询分支多，编译爆炸 | 2^n 组合单态化 | `into_boxed()` |
| 某模块编译特别慢 | 巨型查询类型 | 拆函数（每个函数一个查询类型）|
| 改一行模型全量重编 | schema.rs 被广泛依赖 | schema 拆模块 + 减少跨模块泛型泄漏 |
| 链接期内存高 | 泛型实例过多 | 增量编译 + 按 crate 拆分（api/repo 分 crate）|
| 想看"到底实例化了什么" | —— | `cargo rustc -- -Zself-profile`（nightly）定位耗时 pass |

经验阈值：**查询类型一旦出现在函数签名里（返回 Filtered<...>），就会像藤蔓一样蔓延**——查询构造函数返回值尽量收敛为 `Vec<Model>` / `LoadResult`，中间类型别外泄。

## 10. diesel-async：异步世界的 Diesel

```rust
// Cargo.toml: diesel-async = { version = "0.5", features = ["sqlite", "deadpool"] }
use diesel_async::{AsyncConnection, AsyncSqliteConnection, RunQueryDsl as _};

let mut conn = AsyncSqliteConnection::establish("demo.db").await?;
let rows: Vec<Product> = products::table
    .filter(products::category_id.eq(1))
    .load(&mut conn).await?;        // 同一个 DSL，异步执行

// 连接池：diesel-async 自带 deadpool 集成（AsyncDeadpoolConnection）
// 事务闭包风格与 18 课相同，但闭包是 async：
conn.transaction::<_, diesel::result::Error, _>(|conn| async move {
    diesel::update(...).set(...).execute(conn).await?;
    diesel::insert_into(...).values(...).execute(conn).await?;
    Ok(())
}).await?;
```

注意点：

- 异步版本是"第二实现"（与主 crate 并行维护），特性略滞后；
- 连接必须 `&mut`，池用 deadpool（与 sqlx 内置池不同源）；
- PG 场景支持 pipeline（多条查询一次往返），性能上是亮点。

## 11. sqlx 内幕 vs Diesel 内幕：终极对照

| 维度 | sqlx（21 课）| Diesel（本课）|
|---|---|---|
| SQL 的正确性来源 | 编译期问数据库（DESCRIBE）| DSL 构造即合法（AST）|
| 类型信息载体 | 宏生成的匿名 struct | 表达式树类型（ZST 关联类型）|
| NULL 语义 | 宏推断 + `"!"` 断言 | LEFT JOIN 类型自动 Nullable |
| GROUP BY 语义 | 不检查（运行时错）| **ValidGrouping 编译期检查** |
| 动态查询 | QueryBuilder（文本层拼接）| boxed（类型擦除但保留浅约束）|
| 语句缓存 | 每连接 LRU（文本 → prepared）| 每次执行 prepare（diesel-async/PG 有缓存策略）|
| 运行时开销 | 极低（一次解码映射）| 极低（ZST 树 + 直接拼 SQL）|
| 编译时间杀手 | 宏连库 describe | 查询类型组合单态化 |
| 深水区 | 缓存抖动/池语义 | trait 约束报错可读性、join 类型体操 |

一句话总结这对老对手：

> **sqlx 把"信任数据库"做成编译期服务；Diesel 把"理解 SQL 语义"做成类型系统。** 前者的上限是你写 SQL 的水平，后者的上限是 DSL 的表达能力——所以本教程主线是 sqlx（SQL 能力迁移），而 Diesel 是给"想要编译器替自己守规则"的团队的重武器。

## 12. 常见坑（深度版）

| 坑 | 内幕解释 |
|---|---|
| 报错几千行 trait 错误 | 从底部第一条 `the trait bound ... is not satisfied` 读起——那是最初断掉的约束链（§3）|
| boxed 后 select 类型对不上 | boxed 擦除的是 WHERE 侧；select 的元组结构仍需各处一致，必要时统一到同一个 select 元组 |
| `sql::<T>()` 忘标类型 | 无法推断返回类型直接报错；每个片段都显式 `sql::<Bool>` / `sql::<Nullable<Text>>` |
| 混用同步 Diesel 与 async 运行时 | 连接非 Send 跨 await；要么全同步 + spawn_blocking，要么 diesel-async |
| migration 后 schema.rs 手改 | 双份真相漂移；永远 `diesel migration run` 再生成 |
| `.nullable()` 忘在 left_join 后加 | SelectableExpression 不满足的报错极其晦涩——看到 join 相关 trait 错误先检查 nullability |
| debug_query 打印的不是最终 SQL | 绑定值在执行层展开；占位符 `?`/`$1` 正常，参数另看 |

---

## 练习

1. **读类型训练**：在 IDE 里 hover 以下表达式的类型，抄下来并画出树结构：
   `products::table.filter(products::category_id.eq(1)).filter(products::stock.gt(0))`
   （预期形如 `Select<Filter<Filter<products::table, Eq<...>>, Gt<...>>, ...>` 的嵌套）
2. **三道防线各触发一次编译错误**：分别制造 (a) 类型不匹配 `.eq("abc")`、(b) 跨表列 filter、(c) 聚合混裸列，记录每个报错的关键行（E0277 的第一条 bound），归纳"从哪读起"。
3. **debug_query 对照**：把 18 课练习 2 的 boxed 动态查询用 `debug_query` 打印，开三种不同 filter 组合，观察 SQL 差异（应只多对应的 WHERE 片段）。
4. **GROUP BY 实验**：先写错误的聚合混裸列查询（编译失败），加 `.group_by(products::category_id)` 后再编译，仍失败的话调整 select（只留分组键与聚合）直到通过——完整体验 ValidGrouping 的约束过程。
5. 思考题：为什么 Diesel 无法像 sqlx 那样提供"编译期 NULL 推断报告"（describe），却能保证 LEFT JOIN 列必然 Option？（提示：`SelectableExpression<LeftJoin<...>>` 只为 `Nullable` 包装实现——**不是推断出来的，是构造出来的**：你只能通过 `.nullable()` 得到合法表达式，类型系统关掉了"忘记处理 NULL"的门。）

<details><summary>答案要点</summary>

```text
1. 结构示例（具体类型名随版本）：
   Filter<
     Filter<FromClause(products), Eq<category_id, Bound<Integer, i32>>>,
     Gt<stock, Bound<Integer, i32>>
   >，每层都是 ZST，堆叠成"类型 AST"。

2. (a) missing AsExpression<Integer> for &str
   (b) missing AppearsOnTable<users::table>（或 SelectableExpression）
   (c) ValidGrouping/NonAggregate 相关 bound
   读法：最后一段 "required by ..." 往上第一处 bound 即断点。

3. 组合差异只体现在 WHERE 后追加片段，boxed 使三个版本同类型。

4. 錯误 → 加 group_by → 若 select 仍有非分组裸列（如 name 而分组键是
   category_id）依旧失败 → select((category_id, count_star())) 通过。

5. 推断 vs 构造：sqlx 从数据库描述"猜测"结果可空性（可能错，需 ! 纠正）；
   Diesel 的类型规则从 join 结构【强制】产生 Nullable 提升——正确性来自
   "只能这么写"，而非"检查你写了什么"。
```
</details>

---

← 上一课：[21 · sqlx 深度剖析](21-sqlx深度剖析.md) · [返回目录](../README.md) · [18 课](18-Rust-Diesel.md)（使用篇）· 配套代码：[`rust-sqlx-demo/`](../rust-sqlx-demo/)
