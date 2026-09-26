# 21 · sqlx 深度剖析：连接池、宏与类型系统的内幕

> 进阶专题。前置：[16 课](16-Rust-sqlx入门.md)、[17 课](17-Rust-sqlx进阶.md)（会"用"之后再来看"为什么"）。
> 本课目标：理解 sqlx 的四层架构、Pool 的连接生命周期、`query!` 宏的编译期魔法、预编译语句缓存与它的坑。学完你能够：看懂 sqlx 的报错、诊断池问题、预判宏推断的边界、写出零运行时开销的查询代码。

---

## 1. 四层架构：sqlx 的代码地图

```
┌─────────────────────────────────────────────┐
│ macros 层    query! / query_as! / query_scalar! / migrate!   │  编译期：SQL 校验 + 类型推断
├─────────────────────────────────────────────┤
│ query 层     Query / QueryAs / QueryBuilder / Executor        │  构造查询、绑定参数、执行
├─────────────────────────────────────────────┤
│ pool 层      Pool / PoolConnection / PoolOptions             │  连接复用、超时、健康检查
├─────────────────────────────────────────────┤
│ driver 层    PgConnection / SqliteConnection / MySqlConnection │  各数据库协议实现
└─────────────────────────────────────────────┘
```

关键洞察：**`Pool<DB>` 和 `Connection` 是两个世界**。上层 API（`fetch_*`）接受 `impl Executor`，而 `&Pool`、`&mut Connection`、`&mut Transaction` 都实现了 Executor——这就是 16 课"事务里必须写 `&mut *tx`"的根源：

```rust
// 简化的核心 trait（源码抽象）：
pub trait Executor<'c>: Send {
    type Database: Database;
    fn fetch_all<'q, E>(self, query: E) -> ... where E: 'q + Execute<'q, Self::Database>;
}

// 三个实现者：
impl<'c> Executor<'c> for &'c Pool<DB> { ... }        // 从池取连接 → 执行 → 归还
impl<'c> Executor<'c> for &'c mut Connection { ... }  // 直接用这条连接
// Transaction<'_, DB> : DerefMut<Target = Connection>
//   → &mut *tx 解引用为 &mut Connection → 走第二个实现
```

`Transaction` 是 `Deref` 到 `Connection` 的 RAII 包装：commit/rollback 操作事务状态，其余能力全部委托给内部连接。**事务的排他性（`&mut`）由借用检查器在编译期强制**——两个并发的 `&mut *tx` 直接编译失败，而不是运行时死锁。

## 2. 连接池内幕

### 2.1 连接的生命周期

```
           acquire() ─────────────▶ PoolConnection（Deref 到 Connection）
          ▲    │                        │
   空闲队列  │ 超时(3s)                  │ drop
          │    ▼                        ▼
      [conn] [conn] [conn]      健康？→ 归还空闲队列
          │                        破损？→ 关闭并从池中除名
          └─ 后台任务：维持 min_connections、清理超过 idle_timeout 的连接
```

要点（对应 `PoolOptions`）：

1. **`Pool` 是 `Arc` 内核**——clone/传引用都是共享同一个池，别再包 `OnceCell`；
2. **`PoolConnection` 的 Drop 就是归还**：`fetch_*(&pool)` 之所以不泄漏，是因为内部 acquire → 执行 → 连接对象 drop → 归还，全程在一个语句里；
3. **`acquire_timeout`（默认 30s）**：拿不到连接报 `PoolTimedOut`。生产建议显式设 3~5s——用户等 30 秒早就刷新页面了，不如快点失败；
4. **`max_lifetime` / `idle_timeout`**：防止"长命连接"累积问题（PG 内存膨胀、DNS 变更后旧路由、LB 摘除的节点）。到期连接被异步关闭重建；
5. **`test_before_acquire`（默认 true）**：取连接时发一个轻量探测（如 PG 的空 `Ping`）。代价一次往返，收益是消灭"拿到死连接"的偶发错误。若网络 RTT 极高可关闭，换应用层重试。

### 2.2 池耗尽的三种真因（17 课 §6 的内幕版）

```rust
// 真因 1：泄漏 —— 手动 acquire 后提前 return，连接没归还
let conn = pool.acquire().await?;
if bad { return Err(...); }        // ❌ conn 还活着（等 drop）
                                   // ✅ 显式 drop(conn) 或用作用域包住

// 真因 2：事务跨 await 长期持有
let tx = pool.begin().await?;
let resp = http_client.get(...).await?;   // ❌ 事务(连接)被占着等外部 IO
sqlx::query("...").execute(&mut *tx).await?;

// 真因 3：慢查询堆积 —— 8 个连接 × 每个 2s 的查询 = 200+ QPS 必然排队
```

监控接口（打进 metrics）：

```rust
let size = pool.size();          // 当前总连接数
let idle = pool.num_idle();      // 空闲数
let queued = size - idle;        // 粗略的"在忙"数（含借出）
// size 持续 == max_connections 且 idle == 0 → 池已饱和
```

### 2.3 SQLite 池的特殊性

SQLite **写串行**（整库一把写锁），池开 8 个连接只是让"读"并发；两个连接同时写 → 第二个等数据库锁（默认超时 5s 后报 `database is locked`）。策略：

- 读多写少：`max_connections(核数)` + WAL 模式（读写不互斥）；
- 写密集：`max_connections(1)` 让写排队发生在池层（更可控），或用 `BEGIN IMMEDIATE` 提前拿写锁避免死锁升级；
- `SqliteConnectOptions::busy_timeout` 控制等锁时长。

## 3. `query!` 宏：编译期发生了什么

```rust
let u = sqlx::query!("SELECT id, username FROM users WHERE id = $1", uid);
```

编译期四个步骤（`cargo build` 时）：

```
1. 读 DATABASE_URL（.env）→ 连接数据库
2. 对 SQL 执行 DESCRIBE：
     - PG:      预备语句 + 系统目录查询（列名/类型/OID/not-null 推断）
     - SQLite:  sqlite3_prepare_v2 + EXPLAIN 启发式分析
3. nullability 推断（§3.2）
4. 生成展开代码（大致等价于）：
     let u = {
         #[derive(sqlx::FromRow)]
         struct Record { id: i64, username: String }   // ← 类型已定死
         sqlx::query_as::<_, Record>(r#"SELECT ..."#)  // 运行时只剩映射
             .bind(uid)
     };
```

**这就是宏的全部秘密：编译期把 SQL 的"形状"（列名/类型/可空性）固化成一个匿名 struct，运行时和手写 `query_as` 零差别——没有运行时反射、没有解析。**

`cargo sqlx prepare` 的离线模式（17 课 §2）就是把第 2~3 步的结果缓存进 `.sqlx/query-<hash>.json`（hash = 查询文本的摘要），CI 里宏读缓存跳过连库。推论：**查询文本变一个字符，缓存 key 就变**——动态拼的 SQL 永远无法用宏（回到 QueryBuilder）。

### 3.1 宏家族与选择

| 宏 | 返回 | 场景 |
|---|---|---|
| `query!` | 行记录（`.try_get` 取列）| 偶尔取几列 |
| `query_as!` | 你的 struct | **默认选择** |
| `query_scalar!` | 单值 / `Vec<单值>` | `SELECT COUNT(*)`、一列 id |
| `query!` + `FETCH` | （18 课 Diesel 对比：无对应）| — |

```rust
let cnt: i64 = sqlx::query_scalar!(r#"SELECT COUNT(*) AS "c!" FROM products"#)
    .fetch_one(&pool).await?;
let ids: Vec<i64> = sqlx::query_scalar!("SELECT id FROM products WHERE category_id = ?1", 5)
    .fetch_all(&pool).await?;
```

### 3.2 nullability 推断：为什么总在 SQLite 上遇到 `?`

推断精度按数据库差异很大：

- **PG**：从系统目录读 `attnotnull`，外键 join / 简单投影能精确判定；聚合函数（`SUM`/`AVG`）按 SQL 语义返回 nullable；
- **SQLite**：类型系统动态，只能靠 `sqlite3_column_decltype` + EXPLAIN 启发式，**保守地把很多表达式推成 nullable**——这就是你在 SQLite 上频繁需要 `"alias!"` 的原因。

处理决策树：

```
宏推断为 Option<T>，而该值业务上必非空？
├─ COUNT(*) / COALESCE(x, 默认) / 常量 → 安全地写 "alias!"
├─ SUM/AVG（空集真为 NULL）           → 保留 Option，别加 "!"
└─ SQLite 的表非空列被误推？           → 优先在 SQL 里 CAST/COALESCE 显式化，
                                         "!" 是最后手段（它关掉了编译期保护）
```

> `!` 的本质：**程序员向编译器的一次让步声明**——"数据库说可能空，但我保证不会"。每一次 `!` 都应该能说出理由，code review 值得追问。

## 4. 预编译语句缓存：看不见的性能关键

### 4.1 机制

每个连接内部维护 **LRU 语句缓存**（PG/MySQL 默认容量 100，`PgConnectOptions::statement_cache_capacity` 调整；SQLite 直接复用 `sqlite3_stmt` 句柄）：

```
第一条连接执行 "SELECT ... WHERE id = $1"
  → prepare（一次往返）→ 缓存 (SQL 文本 → prepared statement) → 执行
后续相同文本
  → 缓存命中 → 只发参数 → 执行（快）
```

这就是 sqlx "绑定参数防注入" 之外的另一半：**参数化让同一文本反复命中缓存**，同时把执行计划稳定下来（PG 侧自定义计划 vs 通用计划的取舍另说）。

### 4.2 缓存杀手：动态占位符的 IN 查询

```rust
// 16 课的 IN 拼法，每种 id 数量都是【不同的 SQL 文本】：
"WHERE id IN (?)"      // 1 个 id
"WHERE id IN (?, ?)"   // 2 个 id
"WHERE id IN (?, ?, ?)"// 3 个 id   ← 三份缓存条目
// 用户批量操作 id 数量随请求变化 → 缓存抖动（LRU 互相驱逐）+ 每个新形状一次 prepare
```

对策：

```rust
// PG：数组绑定，文本恒定，一份缓存走天下
"SELECT * FROM products WHERE id = ANY($1)"          // bind(Vec<i64>)

// SQLite：把 IN 改写为 json_each（文本恒定）
"SELECT p.* FROM products p, json_each(?1) j WHERE p.id = j.value"

// 或业务层分桶：per_page 固定 → IN 形状集合有限（如永远 20 个占位符，不足补 id=-1）
```

> 这也解释了一个"玄学"现象：压测时 QPS 忽高忽低——随机长度的 IN 查询在持续制造新的 prepared statement。**缓存命中率**应当纳入监控（PG: `pg_stat_statements` 看语句条目数）。

## 5. 结果获取策略：all / one / optional / **流式**

前三者 16 课已讲。第四个 `fetch()` 少有人用，却是大结果集的救星：

```rust
use futures::TryStreamExt;   // 需要自行加 futures 依赖

// fetch_all：全部读进内存 → Vec（结果集 100 万行 = 内存炸弹）
// fetch：返回 Stream，逐条产出、逐条处理，内存 O(1)
let mut stream = sqlx::query_as::<_, Product>("SELECT * FROM products")
    .fetch(&pool);

while let Some(p) = stream.try_next().await? {
    process(&p).await?;          // 每行到达即处理（导出/ETL 场景）
}
// 中途 return → stream drop → 取消查询（游标关闭）
```

- **导出 CSV、消息推送、消费补偿**这类"逐行处理、不需要回头"的场景用 `fetch()`；
- `fetch_many` 则把 `QueryResult`/`RowsAffected` 混在流里（多语句/存储过程场景），日常少用；
- 注意流持有连接直到耗尽/丢弃——长流 = 长期占连接，池配置要匹配（§2.2 又出现了）。

## 6. 类型系统：Encode / Decode / Type 三件套

sqlx 的列 ↔ Rust 值桥梁（以 SQLite 为例）：

```rust
// "我是哪种 SQL 类型"（编译期选择解码路径）
pub trait Type<DB: Database> { fn type_info() -> TypeInfo; fn compatible(ty: &TypeInfo) -> bool; }

// Rust → SQL（绑定时）
pub trait Encode<'q, DB: Database> { fn encode_by_ref(&self, buf: &mut Vec<u8>) ...; }

// SQL → Rust（读行时）
pub trait Decode<'q, DB: Database>: Sized { fn decode(value: ValueRef) -> Result<Self>; }
```

自定义类型的接入（16 课 enum 的推广——**任何 newtype 都行**）：

```rust
// 金额 newtype：杜绝 bare i64 满天飞（14 课建模 + 16 课 newtype 建议）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cents(pub i64);

impl<'r> sqlx::Encode<'r, sqlx::Sqlite> for Cents {        // 写
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> sqlx::encode::IsNull {
        self.0.encode_by_ref(buf)
    }
}
impl<'r> sqlx::Decode<'r, sqlx::Sqlite> for Cents {        // 读
    fn decode(v: sqlx::sqlite::SqliteValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        Ok(Cents(<i64 as sqlx::Decode<sqlx::Sqlite>>::decode(v)?))
    }
}
impl sqlx::Type<sqlx::Sqlite> for Cents {
    fn compatible(info: &sqlx::sqlite::SqliteTypeInfo) -> bool {
        <i64 as sqlx::Type<sqlx::Sqlite>>::compatible(info)
    }
}
// 现在 Cents 可以出现在 struct 字段 / .bind() / query_as! 推断里
```

> `query_as!` 宏推断的类型是"数据库报告的类型 + 已知的 Decode 实现"——自定义 Type/Decode 对宏同样生效（前提是离线缓存的 describe 与实现一致）。

## 7. 事务的 Drop 语义：回滚是"尽力而为"

```rust
let mut tx = pool.begin().await?;
sqlx::query("UPDATE ...").execute(&mut *tx).await?;
// 忘了 commit，tx 在函数尾 drop：
//   → Drop::drop 里【异步上下文外】发起 ROLLBACK
//   → 成功：安全回滚
//   → 失败（连接已断）：连接标记为 broken，直接丢弃（数据不会"半提交"）
```

两个推论：

1. **错误路径不需要手写 ROLLBACK**——`?` 提前返回触发 drop，自动回滚（16 课依赖的正是这个）；
2. **drop 回滚不提供错误反馈**——回滚失败你不知道。**对"回滚必须成功"的场景（审计敏感）显式 `tx.rollback().await?`**，把错误交还调用者。

SAVEPOINT（嵌套事务）：

```rust
let mut tx = pool.begin().await?;            // BEGIN
sqlx::query("UPDATE a ...").execute(&mut *tx).await?;
{
    let mut sp = tx.begin().await?;          // SAVEPOINT（不是真连接）
    sqlx::query("UPDATE b ...").execute(&mut *sp).await?;
    // sp.drop → ROLLBACK TO SAVEPOINT（只撤销 b，保留 a）
}
tx.commit().await?;                          // a 生效
```

`Transaction::begin` 在事务内产生 SAVEPOINT——**不用外部协调器就能实现"子步骤失败只回滚自己"**（下单成功 + 积分失败不阻塞订单）。

## 8. 常见坑（深度版）

| 坑 | 内幕解释 |
|---|---|
| 改了 SQL 但类型没更新 | 宏按查询文本 hash 查离线缓存——文本变了才重新 describe；`.sqlx/` 旧缓存 + 忘了 prepare = CI 编译的还是旧类型 |
| `&mut *tx` 写成 `&tx` | Transaction 是 Deref 不是 Copy；`&tx` 是 `&Transaction`，没实现 Executor |
| 池看似"泄漏"但 size 正常 | 可能是流式 fetch 未耗尽未丢弃，连接悬在 Stream 里 |
| PG 偶发 `statement cache is full` 警告 | 动态 SQL 文本形状过多撑爆 LRU（§4.2），降低形状数或调大容量 |
| `database is locked`（SQLite） | 两连接同时写；WAL + busy_timeout 或写路径单连接 |
| 事务里 drop 假装回滚了 | 极端场景连接半死，回滚静默失败；审计敏感场景显式 rollback().await |
| 宏编译极慢 | 每个 `query!` 都连库 describe；几百条查询 + SQLite 磁盘库 = 编译分钟级。PG + 局域网/容器内库明显更快 |

## 9. Rust 视角综合：一次完整的"内幕标注"调用

```rust
// 把 16 课的下单函数用本课知识重新注释一遍：
let mut tx = pool.begin().await?;              // ① 从池 acquire 一条连接 + BEGIN
                                               //    （连接被 tx 独占，直到 commit/drop）
let rows = sqlx::query_as::<_, Product>(sql)   // ② 文本进连接的语句缓存（命中则免 prepare）
    .bind(pid)                                 // ③ 参数走 Encode：i64 → 网络格式
    .fetch_all(&mut *tx).await?;               // ④ Deref 到 &mut Connection 的 Executor
                                               //    结果经 Decode: SQLite值 → Product
let res = sqlx::query("UPDATE ...").execute(&mut *tx).await?;
if res.rows_affected() == 0 { return Err(...); }  // ⑤ 提前 return → tx drop → 尽力 ROLLBACK
tx.commit().await?;                            // ⑥ COMMIT，连接归还池（§2.1）
```

每一步都知道引擎盖下发生了什么——这就是"深度剖析"的目的。

---

## 练习

1. **池行为观测**：在 `rust-sqlx-demo` 里写一个循环（`for i in 0..10`，每次 `tokio::time::sleep(200ms)` 持有连接），把 `max_connections` 设为 2，打印每次循环后的 `pool.size()/num_idle()`。预测并解释输出。
2. **缓存抖动复现**：分别执行 `IN` 占位符数 = 1..20 的循环查询（PG 环境观察 `\l` 的语句缓存；SQLite 观察 `PRAGMA`/EXPLAIN 是否重新 prepare），对比改写为 `id = ANY($1)`（PG）或 `json_each`（SQLite）后的行为差异。
3. **流式 vs 全量**：写两版"全表打印"（`fetch_all` vs `fetch` + `TryStreamExt`），在查询里 `LIMIT 100000`（可用 `WITH RECURSIVE` 造数据），用 `/usr/bin/time -v` 对比峰值内存。
4. **SAVEPOINT 实验**：在事务里嵌套两个 SAVEPOINT，一个 rollback 一个 commit，验证外层提交后只有未回滚的子步骤生效。
5. 思考题：为什么 `Pool` 实现 `Executor` 用的是 `&Pool`（不可变引用），而 `Connection` 必须是 `&mut Connection`？（提示：连接内部有状态机/游标，池取连接走的是内部 `Mutex<Option<Connection>>` 的所有权转移。）

<details><summary>答案要点</summary>

```text
1. size 恒为 2（首循环建连），num_idle 在 0~2 波动：
   持有中 0，归还后 2。第 3 个并发任务会等待 acquire_timeout。

2. PG: pg_stat_activity / 语句缓存条目数持续增长（每形状一条）；
   改 ANY($1) 后恒为 1 条。SQLite: prepare 在 C 层缓存，形状变化同样触发
   sqlite3_prepare_v2 重编（EXPLAIN QUERY PLAN 计划不变但句柄重建）。

3. fetch_all: 峰值内存 ≈ 全部行物化；fetch: 恒定（每行解码后即释放）。
   注意 fetch 版持连接时间长——内存换了连接占用。

4. BEGIN; SAVEPOINT a; UPDATE A; SAVEPOINT b; UPDATE B;
   ROLLBACK TO b;（B 撤销）COMMIT;（A 生效）
   sqlx 对应: tx.begin() 内嵌 sp_a/sp_b，sp_b.rollback() 后 tx.commit()。

5. 连接是"有会话状态"的资源（事务、游标、语句缓存互不同步），
   并发使用必须互斥 → &mut。池的借用是"从仓库里搬走一个"（所有权级操作），
   由内部锁保护，&self 即可。
```
</details>

---

← 上一课：[20 · 综合项目：Axum REST API](20-综合项目-Axum-REST-API.md) | 下一课：[22 · Diesel 深度剖析](22-Diesel深度剖析.md) · [返回目录](../README.md) · 配套代码：[`rust-sqlx-demo/`](../rust-sqlx-demo/)
