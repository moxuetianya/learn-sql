# 07 · 子查询与 CTE

> 本课目标：掌握标量/列/表子查询、相关子查询、EXISTS，以及用 CTE（WITH）重构复杂查询。这是从"能写 SQL"到"能写可维护 SQL"的跨越。

---

## 1. 子查询的四种形态

子查询 = 查询里的查询（括号包起来的 SELECT）。按"返回什么"分四类：

| 形态 | 返回 | 使用位置 | 例子 |
|------|------|---------|------|
| 标量子查询 | 1 行 1 列 | 任何能放值的地方 | `WHERE x > (SELECT AVG(...))` |
| 列子查询 | 1 列 N 行 | IN / ANY / ALL | `WHERE id IN (SELECT ...)` |
| 行子查询 | 1 行 N 列 | 行比较 | `WHERE (a,b) = (SELECT ...)` |
| 表子查询 | 结果集 | FROM / JOIN | `FROM (SELECT ...) s` |

### 1.1 标量子查询

```sql
-- 比平均价贵的商品
SELECT name, price_cents
FROM products
WHERE price_cents > (SELECT ROUND(AVG(price_cents)) FROM products)
ORDER BY price_cents DESC;
```

⚠️ 标量子查询若返回多行 → 运行时报错（PG: `more than one row returned by a subquery`）；返回 0 行 → 整体值是 NULL，`> NULL` 恒不成立，静默查出 0 行。**标量子查询 = 期望恰好一个值**，心里要有断言。

### 1.2 列子查询 + IN / ANY / ALL

```sql
-- 下过单的用户
SELECT username FROM users
WHERE id IN (SELECT user_id FROM orders);

-- ANY/SOME：大于其中任意一个（= 大于最小值）
SELECT name FROM products
WHERE price_cents > ANY (SELECT price_cents FROM products WHERE category_id = 5);

-- ALL：大于所有（= 大于最大值）
SELECT name FROM products
WHERE price_cents > ALL (SELECT price_cents FROM products WHERE category_id = 5);
```

`NOT IN` 的 NULL 陷阱（03 课）：子查询结果含 NULL 时整个查询返回 0 行。**否定存在性判断一律用 NOT EXISTS**：

```sql
-- 从未被下单的商品：推荐写法
SELECT name FROM products p
WHERE NOT EXISTS (SELECT 1 FROM order_items oi WHERE oi.product_id = p.id);
```

### 1.3 表子查询（派生表）

FROM 里的子查询必须**起别名**：

```sql
-- 每个分类的均价，再筛出均价 > 15 万的（子查询先聚合，外层过滤——绕开 WHERE 不能用聚合的限制）
SELECT cid, avg_price
FROM (
    SELECT category_id AS cid, ROUND(AVG(price_cents),0) AS avg_price
    FROM products
    GROUP BY category_id
) t
WHERE avg_price > 150000;
```

## 2. 相关子查询（correlated）

内层引用外层的列 → 每行外层都执行一次内层。**经典题型：每组 Top N**。

```sql
-- 每个分类最贵的商品（相关子查询版）
SELECT p.name, p.category_id, p.price_cents
FROM products p
WHERE p.price_cents = (
    SELECT MAX(p2.price_cents)
    FROM products p2
    WHERE p2.category_id = p.category_id      -- ← 引用外层 p：相关
)
ORDER BY p.category_id;
```

EXISTS 本质就是相关子查询的布尔化：

```sql
-- 有 5 星评价的商品
SELECT p.name
FROM products p
WHERE EXISTS (
    SELECT 1 FROM reviews r
    WHERE r.product_id = p.id AND r.rating = 5
);
```

`SELECT 1` 是惯例（数据库只关心"有没有行"，列内容无所谓）。EXISTS 找到第一行即短路，通常比 `IN (子查询)` 更稳（尤其大数据 + 可空列场景）。

### IN vs EXISTS 性能直觉

- 外层大、子查询结果集小 → IN 可能更快；
- 子查询结果集大 / 相关条件有索引 → EXISTS 通常更快；
- 现代优化器（PG）常把两者优化成同一个计划——**先写语义最清晰的，慢了再 EXPLAIN 调优**（13 课）。

## 3. CTE：WITH 子句，复杂查询的正确打开方式

CTE（Common Table Expression）= 命名的一次性视图，让长查询像搭积木：

```sql
WITH
monthly AS (                       -- CTE 1：按月聚合
    SELECT substr(created_at,1,7) AS month,
           SUM(total_cents)        AS revenue,
           COUNT(*)                AS orders
    FROM orders
    WHERE status <> 'cancelled'
    GROUP BY substr(created_at,1,7)
),
growth AS (                        -- CTE 2：引用 CTE 1
    SELECT month, revenue,
           revenue - LAG(revenue) OVER (ORDER BY month) AS mom_diff   -- 09 课窗口函数预告
    FROM monthly
)
SELECT * FROM growth ORDER BY month;
```

### CTE vs 派生表 vs 子查询

```sql
-- 同一需求三种写法：有评价的商品均价
-- 1. 子查询（一次性）
SELECT ROUND(AVG(price_cents)) FROM products
WHERE id IN (SELECT product_id FROM reviews);

-- 2. 派生表
SELECT ROUND(AVG(price_cents)) FROM (
    SELECT p.price_cents FROM products p
    JOIN (SELECT DISTINCT product_id FROM reviews) r ON r.product_id = p.id
) t;

-- 3. CTE（可读性最好，主查询保持干净）
WITH reviewed AS (SELECT DISTINCT product_id FROM reviews)
SELECT ROUND(AVG(p.price_cents))
FROM products p JOIN reviewed r ON r.product_id = p.id;
```

**团队规范建议：超过 2 层嵌套或超过 ~30 行的查询，一律改用 CTE。** Rust 代码里嵌 5 层子查询的 SQL 是 code review 杀手。

### 递归 CTE：SQL 里的"循环"

处理树形结构（分类树、组织架构、评论楼中楼）：

```sql
-- 生成 1~10 的数列（经典入门例）
WITH RECURSIVE nums(n) AS (
    SELECT 1               -- 锚点：起点
    UNION ALL
    SELECT n + 1 FROM nums WHERE n < 10   -- 递归：引用自己
)
SELECT * FROM nums;

-- 实战场景：订单状态流转链（假设有 order_events 表 prev_status→next_status）
-- WITH RECURSIVE chain(status, depth) AS (
--     SELECT 'pending', 1
--     UNION ALL
--     SELECT e.next_status, c.depth + 1
--     FROM chain c JOIN order_events e ON e.prev_status = c.status
-- )
-- SELECT * FROM chain;
```

⚠️ 递归 CTE 忘写终止条件 → 无限循环（数据库有 max_recursion 上限兜底，PG 默认无上限会跑满资源，务必小心）。

## 4. LATERAL：相关派生表（PG）

普通派生表不能引用同层其他表；`LATERAL` 可以——每行外层驱动一次内层查询。**每组 Top N 的现代解法**：

```sql
-- PostgreSQL 专用：每个分类最贵的 2 个商品
SELECT c.name, top2.name, top2.price_cents
FROM categories c
JOIN LATERAL (
    SELECT p.name, p.price_cents
    FROM products p
    WHERE p.category_id = c.id
    ORDER BY p.price_cents DESC
    LIMIT 2
) top2 ON true
ORDER BY c.id, top2.price_cents DESC;
```

SQLite 的对应物是 `CROSS JOIN + 相关子查询`的优化器魔法，通用方案见 09 课窗口函数（`ROW_NUMBER() OVER (PARTITION BY ...)`），**窗口函数是每组 Top N 的可移植答案**。

## 5. 综合实战：用户消费报告

需求：每个用户的注册月份、订单数（有效单）、总消费、与全场人均消费的差值，按消费降序，只要消费过的人。

```sql
WITH valid_orders AS (
    SELECT * FROM orders WHERE status <> 'cancelled'
),
per_user AS (
    SELECT u.username,
           substr(u.created_at, 1, 7)          AS reg_month,
           COUNT(o.id)                          AS order_cnt,
           SUM(o.total_cents)                   AS spent
    FROM users u JOIN valid_orders o ON o.user_id = u.id
    GROUP BY u.id, u.username, u.created_at
)
SELECT username, reg_month, order_cnt, spent,
       spent - (SELECT ROUND(AVG(spent)) FROM per_user) AS diff_vs_avg
FROM per_user
ORDER BY spent DESC;
```

一个查询同时用上：CTE、JOIN、聚合、标量子查询。请动手跑，再试着重构成你自己的风格。

## 6. 常见坑

| 坑 | 说明 |
|---|---|
| 标量子查询返回多行 | 运行时崩溃；返回 0 行则静默 NULL。想清楚"这个子查询一定恰好 1 个值吗" |
| NOT IN 遇 NULL | 0 行惨案。NOT EXISTS 替代 |
| FROM 子查询忘起别名 | 语法错误（PG 强制） |
| 相关子查询无索引支撑 | 每行执行一次内层，O(n²) 灾难。外键列建索引（13 课） |
| 递归 CTE 无终止条件 | 资源耗尽 |
| CTE 滥用 | PG 12 之前 CTE 是优化栅栏（不内联合并），老版本注意；顺便别把一个 CTE 写成 200 行怪物 |

## 7. Rust 视角

sqlx 的 `query!`/`query_as!` 宏对 CTE、子查询完全透明——**宏只关心最外层 SELECT 的列与类型**。所以重构 SQL（子查询 ↔ CTE ↔ JOIN）不影响 Rust 代码，放心在 SQL 层做可读性优化：

```rust
// 里面怎么 WITH 都行，宏只校验最终 SELECT username, order_cnt, spent
let report = sqlx::query_as::<_, (String, i64, i64)>(
    "WITH per_user AS (...) SELECT username, order_cnt, spent FROM per_user"
).fetch_all(&pool).await?;
```

17 课会讲 sqlx-cli 的 `.sql` 文件工作流：复杂查询放 `queries/` 目录离线编译校验，比内嵌字符串更好维护。

---

## 练习

1. 价格高于全店均价的商品（标量子查询），显示名称与价格，按价格降序。
2. 每个用户第一笔订单（id 最小那笔）的 id、user_id、created_at。两种解法：相关子查询 `MIN` + join，或窗口函数 `ROW_NUMBER`（预习 09 课）。
3. 找出**没有收到任何 5 星评价**的商品名（NOT EXISTS）。
4. 用递归 CTE 生成 2025 年 1~12 月的月份序列（`'2025-01'`…`'2025-12'`），再 LEFT JOIN 订单统计每月销售额（没订单的月份显示 0）。
5. 把第 5 节综合实战改成：包含从未下单的用户（订单数 0、消费 0），仍然显示与人均的差值。

<details><summary>答案</summary>

```sql
-- 1.
SELECT name, price_cents FROM products
WHERE price_cents > (SELECT AVG(price_cents) FROM products)
ORDER BY price_cents DESC;

-- 2. 相关子查询版
SELECT o.id, o.user_id, o.created_at
FROM orders o
WHERE o.id = (SELECT MIN(o2.id) FROM orders o2 WHERE o2.user_id = o.user_id)
ORDER BY o.user_id;
-- 窗口函数版（09 课详解）
SELECT id, user_id, created_at FROM (
    SELECT o.*, ROW_NUMBER() OVER (PARTITION BY user_id ORDER BY id) rn
    FROM orders o
) t WHERE rn = 1 ORDER BY user_id;

-- 3.
SELECT p.name FROM products p
WHERE NOT EXISTS (
    SELECT 1 FROM reviews r WHERE r.product_id = p.id AND r.rating = 5
)
ORDER BY p.name;

-- 4.
WITH RECURSIVE months(m) AS (
    SELECT '2025-01'
    UNION ALL
    SELECT strftime('%Y-%m', date(m || '-01', '+1 month')) FROM months WHERE m < '2025-12'
)
SELECT m.m AS month, COALESCE(SUM(o.total_cents), 0) AS revenue
FROM months m
LEFT JOIN orders o ON substr(o.created_at,1,7) = m.m AND o.status <> 'cancelled'
GROUP BY m.m
ORDER BY m.m;

-- 5. 关键：把 JOIN 换成 LEFT JOIN，聚合列 COALESCE 兜底
WITH valid AS (SELECT * FROM orders WHERE status <> 'cancelled'),
per_user AS (
    SELECT u.username, u.created_at,
           COUNT(o.id) AS order_cnt, COALESCE(SUM(o.total_cents),0) AS spent
    FROM users u LEFT JOIN valid o ON o.user_id = u.id
    GROUP BY u.id, u.username, u.created_at
)
SELECT username, order_cnt, spent,
       spent - (SELECT ROUND(AVG(spent)) FROM per_user) AS diff_vs_avg
FROM per_user ORDER BY spent DESC;
```
</details>

---

← 上一课：[06 · JOIN 多表查询](06-JOIN多表查询.md) | 下一课：[08 · 集合运算](08-集合运算.md) · [返回目录](../README.md)
