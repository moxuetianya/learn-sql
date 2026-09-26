# 01 · SQL 与关系模型入门

> 本课目标：理解表/行/列/键/类型这些底层概念，掌握 SQL 语句分类。这些概念决定你后面能否设计出好的表结构。

---

## 1. 关系模型：数据库的世界观

SQL 数据库的理论基础是 1970 年 Codd 的**关系模型**，核心只有一句话：

> **数据 = 集合 of 元组**。每张表是一个集合，每行是一个元组，每列是一个有类型的属性。

| SQL 术语 | 数学/理论术语 | Rust 类比 |
|----------|--------------|----------|
| 表 (table) | 关系 (relation) | `Vec<struct>` |
| 行 (row) | 元组 (tuple) | `struct` 实例 |
| 列 (column) | 属性 (attribute) | struct 字段 |
| 类型 (type) | 域 (domain) | 字段类型 |

两个直接推论，贯穿整个教程：

1. **行没有顺序**。`SELECT` 返回的顺序是未定义的，除非你写 `ORDER BY`。别依赖"插入顺序"。
2. **集合里不该有重复的行**。表靠主键保证每行唯一；查询结果用 `DISTINCT` 去重。

## 2. 键：把行"钉"住的东西

### 主键（Primary Key）

唯一标识一行，不可为 NULL、不可重复。

```sql
-- 常见三种写法（shop 库用第 1 种）
id INTEGER PRIMARY KEY AUTOINCREMENT,      -- SQLite
id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,  -- PostgreSQL 现代写法
id SERIAL PRIMARY KEY,                     -- PostgreSQL 传统写法
```

**实践铁律**：每张表都要有主键；用无业务含义的代理键（`id`），别用身份证号、手机号这种"看起来唯一"的 natural key（它们会变、会重号、有隐私问题）。

### 外键（Foreign Key）

指向另一张表的主键，表达"属于"关系：

```sql
CREATE TABLE orders (
    id     INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users(id),  -- 外键：订单必须属于真实用户
    ...
);
```

有了外键，数据库会**拒绝**插入 user_id=999 的订单（不存在该用户）——把数据完整性从应用层代码交还给数据库。

### 唯一约束（UNIQUE）vs 主键

| | 主键 | UNIQUE |
|---|---|---|
| 数量/表 | 最多 1 个 | 任意多个 |
| NULL | 不允许 | 允许（PG 中多个 NULL 不冲突）|
| 语义 | 行的身份 | 业务上的唯一（如 email）|

## 3. NULL：不是零，不是空串

NULL 表示"**未知/不存在**"，它是所有类型之外的第三种状态：

```sql
SELECT NULL = NULL;    -- NULL（未知 = 未知？还是未知！）
SELECT NULL IS NULL;   -- 1（判断 NULL 只能用 IS）
SELECT NULL + 1;       -- NULL（未知参与运算结果未知）
```

后端最常见的坑：`WHERE comment != '好评'` **不会**返回 comment 为 NULL 的行（03 课详讲三值逻辑）。Rust 中 NULL 对应 `Option<T>`，这个映射在 16 课。

## 4. SQL 语句分类（面试常考）

| 分类 | 全称 | 语句 | 占比 |
|------|------|------|------|
| **DQL** | Query | `SELECT` | 70%（02~09 课）|
| **DML** | Manipulation | `INSERT` `UPDATE` `DELETE` | 15%（11 课）|
| **DDL** | Definition | `CREATE` `ALTER` `DROP` | 10%（10 课）|
| **TCL** | Transaction Control | `BEGIN` `COMMIT` `ROLLBACK` | 5%（12 课）|
| DCL | Control | `GRANT` `REVOKE` | 生产环境 DBA 管 |

执行顺序（理解 05 课 GROUP BY 的关键）：

```
写的顺序:  SELECT → FROM → WHERE → GROUP BY → HAVING → ORDER BY → LIMIT
执行顺序:  FROM → WHERE → GROUP BY → HAVING → SELECT → ORDER BY → LIMIT
          （先拿数据 → 过滤 → 分组 → 过滤组 → 选列 → 排序 → 截断）
```

## 5. 数据类型：三种数据库对照

| 用途 | SQLite | PostgreSQL | MySQL |
|------|--------|-----------|-------|
| 整数 | INTEGER | SMALLINT/INT/BIGINT | TINYINT~BIGINT |
| 精确小数（钱！）| INTEGER(存分) | `NUMERIC(12,2)` | DECIMAL |
| 浮点 | REAL | REAL / DOUBLE | FLOAT/DOUBLE |
| 短文本 | TEXT | `VARCHAR(n)` | VARCHAR(n) |
| 长文本 | TEXT | TEXT | TEXT |
| 布尔 | INTEGER(0/1) | BOOLEAN | TINYINT(1) |
| 时间戳 | TEXT(ISO) | `TIMESTAMPTZ` | DATETIME |
| 日期 | TEXT | DATE | DATE |
| JSON | TEXT | `JSONB` | JSON |

**后端选型经验**：

1. **钱永远不用浮点**。用 `NUMERIC` 或整数分（shop 库用 `price_cents INTEGER`）。
2. 时间戳在 PG 用 `TIMESTAMPTZ`（带时区），避免夏令时灾难。
3. PG 的 `TEXT` 和 `VARCHAR(n)` 性能完全一样，`VARCHAR(n)` 只是加了个长度约束——需要约束就加，不需要就 TEXT。
4. SQLite 是动态类型（列类型只是"亲和性"），所以更要在应用层（Rust struct 类型）把类型守住。

## 6. 用 shop 库巩固概念

```sql
-- 每行的身份：id；业务唯一性：username/email
SELECT id, username, email FROM users LIMIT 3;

-- 外键在起作用：尝试给不存在的用户下单 → 会报 FOREIGN KEY constraint failed
INSERT INTO orders (user_id) VALUES (999);

-- 联合主键示例：order_items 一行由 (order_id, product_id) 共同标识
SELECT * FROM order_items LIMIT 3;
```

<details><summary>执行第三条会发生什么？</summary>

报错 `FOREIGN KEY constraint failed`（前提：`PRAGMA foreign_keys=ON`，见 00 课配置）。这就是把完整性校验交给数据库——如果只靠 Rust 代码先查 user 再插订单，两个请求并发时就可能插入脏数据（TOCTOU 竞态）。
</details>

## 7. Rust 视角

shop 库的 users 表在 Rust 里天然对应：

```rust
struct User {
    id: i64,
    username: String,
    email: String,
    role: String,          // 16 课：用 enum + sqlx 映射更好
    created_at: DateTime<Utc>,
    // 若列可空 → Option<T>，例如 reviews.comment: Option<String>
}
```

类型系统对照：SQL `NOT NULL` ↔ 非 `Option` 字段；SQL `NULL` ↔ `Option<T>`；SQL `CHECK` ↔ newtype + 校验（Rust 侧 + 数据库侧各守一道）。

---

## 练习

1. 为什么说"不写 ORDER BY 时 SELECT 结果顺序是未定义的"？用一条 SQL 观察 SQLite 中 `SELECT id FROM orders` 的返回顺序，思考如果它是哈希表/堆表存储会怎样。
2. `SELECT NULL = NULL, NULL IS NULL, NULL IS NOT NULL;` 三个表达式各返回什么？
3. 指出 shop 库 `reviews` 表中的主键、唯一约束、外键、CHECK 约束分别是什么（`.schema reviews`）。
4. 一个用户表要求"手机号必填且唯一、微信号可空且唯一"，写出这两列的定义（PG 语法）。

<details><summary>答案</summary>

```sql
-- 1. 关系模型中表是无序集合，返回顺序取决于存储与执行计划；
--    实测 sqlite3 通常按 rowid 顺序返回，但这不是承诺，不能依赖。
--    （13 课会看到：加了索引或换了执行计划，顺序可能变。）

-- 2. NULL, 1, 0   —— NULL 与任何值比较都是 NULL，判断空只能用 IS。

-- 3. 主键: id；唯一约束: UNIQUE(user_id, product_id)；
--    外键: user_id→users(id), product_id→products(id)；
--    CHECK: rating BETWEEN 1 AND 5。

-- 4. (PG)
-- phone TEXT NOT NULL UNIQUE,
-- wechat TEXT UNIQUE          -- PG 中 UNIQUE 列可以有多个 NULL，正好满足"可空但非空值唯一"
```
</details>

---

← 上一课：[00 · 环境搭建](00-环境搭建与示例数据库.md) | 下一课：[02 · SELECT 基础](02-SELECT基础.md) · [返回目录](../README.md)
