# 02 · SELECT 基础

> 本课目标：掌握 SELECT 的五大件——投影（列）、别名、DISTINCT、ORDER BY、LIMIT/OFFSET。这是你以后写得最多的语句骨架。

---

## 1. 最小的查询：选列（投影）

```sql
-- 所有列（开发期调试用；生产代码里别写 *，见下）
SELECT * FROM products;

-- 指定列：只取需要的，网络传输和内存都更省
SELECT name, price_cents FROM products;
```

> ⚠️ **生产禁用 `SELECT *`** 的三个理由：
> 1. 表加列后，`query_as!` 绑定的 Rust struct 可能错位/报错；
> 2. 传输不需要的 BLOB/大文本浪费带宽；
> 3. 数据库无法做"覆盖索引"优化（13 课）。
> 例外：交互式排查、`EXISTS` 子查询里 `SELECT 1` 之外的临时场景。

## 2. 列别名与表达式：SELECT 是"计算"不只是"取"

SELECT 列表里可以写**表达式**，数据库帮你算：

```sql
SELECT
    name,
    price_cents,
    price_cents * 0.9          AS discount_price,   -- 表达式 + 别名
    price_cents / 100.0        AS price_yuan,
    stock > 0                  AS in_stock          -- SQLite: 1/0; PG: true/false
FROM products
LIMIT 5;
```

别名规则：

- `AS` 可省略（`price_yuan`），但**写上更清晰**；
- 别名在 ORDER BY 里可用（05 课讲为什么 HAVING 里不行）；
- 需要空格/关键字时用双引号：`AS "unit price"`（别这么干，自找麻烦）。

## 3. 去重：DISTINCT

```sql
-- 有多少用户下过单？
SELECT COUNT(DISTINCT user_id) FROM orders;         -- 8

-- 去重看有哪些状态
SELECT DISTINCT status FROM orders;
-- pending / paid / shipped / completed / cancelled

-- DISTINCT 是"整行去重"，不是单列
SELECT DISTINCT user_id, status FROM orders ORDER BY user_id;
```

> DISTINCT 代价：需要排序或哈希去重（EXPLAIN 里能看到 `TEMP B-TREE`/`HashAggregate`），大表上很贵。能靠数据模型保证唯一（主键/UNIQUE）就别在查询时去重。

## 4. 排序：ORDER BY

```sql
-- 单列升序（ASC 可省略）
SELECT name, price_cents FROM products ORDER BY price_cents;

-- 降序
SELECT name, price_cents FROM products ORDER BY price_cents DESC;

-- 多列：先按分类，同分类内按价格降序
SELECT category_id, name, price_cents
FROM products
ORDER BY category_id ASC, price_cents DESC;

-- 按表达式/别名排序
SELECT name, price_cents * stock AS inventory_value
FROM products
ORDER BY inventory_value DESC
LIMIT 3;
```

### NULL 在排序中的位置

| 数据库 | ASC 时 NULL | DESC 时 NULL | 可控制 |
|--------|------------|-------------|--------|
| SQLite | 最前 | 最后 | `NULLS LAST`（3.30+）|
| PostgreSQL | 最后 | 最前 | `NULLS FIRST/LAST` |
| MySQL | 最前 | 最后 | 8.0 才支持 `NULLS FIRST/LAST` 语法 |

```sql
-- PostgreSQL / SQLite 3.30+：让 NULL 评论的行排最后
SELECT user_id, comment FROM reviews ORDER BY comment ASC NULLS LAST;
```

## 5. 截断：LIMIT 与 OFFSET

```sql
-- Top N：销量最高的 5 个商品（按明细数量近似）
SELECT product_id, SUM(quantity) AS sold
FROM order_items
GROUP BY product_id
ORDER BY sold DESC
LIMIT 5;

-- 跳过前 2 行
SELECT id, name FROM products ORDER BY id LIMIT 5 OFFSET 2;
```

方言差异：

```sql
LIMIT 5 OFFSET 2     -- SQLite / PostgreSQL
LIMIT 2, 5           -- MySQL（等价：跳过2取5）
OFFSET 2 ROWS FETCH NEXT 5 ROWS ONLY   -- SQL 标准（PG/Oracle 支持）
```

> ⚠️ **深分页性能陷阱**：`LIMIT 20 OFFSET 100000` 要扫过并丢弃前 100000 行。正确姿势是**游标分页（keyset pagination）**：
> ```sql
> -- 上一页最后一条 id=42，取下一页：走主键索引，O(log n)
> SELECT id, name FROM products WHERE id > 42 ORDER BY id LIMIT 20;
> ```
> 后端 API 翻页优先 keyset，页码跳转才用 OFFSET。

## 6. 没有 FROM 的 SELECT：一台袖珍计算器

```sql
SELECT 1 + 1;                 -- 2
SELECT 3.14 * 100;            -- 314.0

-- 快速看当前时间（SQLite / PG 各不同）
SELECT datetime('now');       -- SQLite
SELECT now();                 -- PostgreSQL
```

PG 中它还常用于造测试数据：`SELECT generate_series(1, 5);`

## 7. 组合起来：一条"报表味"的查询

需求：商品价值排行——按"库存 × 单价"算资金占用，只看前 5，金额转成元。

```sql
SELECT
    id,
    name,
    price_cents,
    stock,
    ROUND(price_cents * stock / 100.0, 2) AS inventory_value_yuan
FROM products
ORDER BY price_cents * stock DESC
LIMIT 5;
```

请动手执行，对照理解执行顺序：FROM → (WHERE) → SELECT（算表达式）→ ORDER BY → LIMIT。

## 8. 常见坑

| 坑 | 后果 | 正确做法 |
|---|---|---|
| 不写 ORDER BY 却期望按插入顺序 | 换库/升级后顺序乱 | 显式 ORDER BY |
| `SELECT *` 进生产代码 | 加列就崩、白传数据 | 列名写全 |
| `LIMIT` 无 `ORDER BY` | "Top 5"不确定是哪 5 条 | LIMIT 必配 ORDER BY |
| OFFSET 深分页 | 页数越深越慢 | keyset 分页 |
| 别名里用中文引号‘’或反引号 | 语法错误 | ASCII 双引号 `"` |

## 9. Rust 视角

sqlx 里 SELECT 结果映射到 struct/tuple，列序与类型都要对上：

```rust
// 元组按列顺序映射
let top: Vec<(String, i64)> = sqlx::query_as(
    "SELECT name, price_cents FROM products ORDER BY price_cents DESC LIMIT 5"
).fetch_all(&pool).await?;

// struct 用 #[derive(FromRow)] 按列名映射（16 课详解）
#[derive(sqlx::FromRow)]
struct Product { name: String, price_cents: i64 }
```

注意：SQL 里能 `ROUND(...)` 的就别拉到 Rust 里算——数据库算完再传，省带宽也省序列化。

---

## 练习

1. 查询商品名称和"按 85 折促销后的价格"（元，保留 2 位小酒），按折后价降序，取前 8。
2. 有多少种不同的 `user_id` 出现在 reviews 表？一条 SQL。
3. 查询订单表按 status 升序、created_at 降序的前 10 条（显示 id, user_id, status, created_at）。
4. 用 keyset 分页取 products 表第 3 页（每页 5 条，按 id 升序）：先取前两页最后一条 id，再取下一页。
5. 思考：`SELECT DISTINCT category_id FROM products ORDER BY category_id` 与 `SELECT id FROM categories ORDER BY id` 结果一定相同吗？什么情况下不同？

<details><summary>答案</summary>

```sql
-- 1.
SELECT name, ROUND(price_cents * 0.85 / 100.0, 2) AS sale_price
FROM products ORDER BY price_cents * 0.85 DESC LIMIT 8;

-- 2.
SELECT COUNT(DISTINCT user_id) FROM reviews;   -- 8

-- 3.
SELECT id, user_id, status, created_at
FROM orders ORDER BY status ASC, created_at DESC LIMIT 10;

-- 4. 第一页后 id=5，第二页后 id=10：
SELECT id, name FROM products WHERE id > 10 ORDER BY id LIMIT 5;   -- id 11~15

-- 5. 不一定相同。若有分类从未上过商品（category_id 不出现在 products），
--    左边查不到它；反之若 products 有脏数据指向不存在的分类，左边会多出幽灵 id。
--    这正是 06 课 JOIN 要解决的问题：以 categories 为主表 LEFT JOIN。
```
</details>

---

← 上一课：[01 · SQL 与关系模型入门](01-SQL与关系模型入门.md) | 下一课：[03 · WHERE 过滤](03-WHERE过滤.md) · [返回目录](../README.md)
