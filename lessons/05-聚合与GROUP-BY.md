# 05 · 聚合与 GROUP BY

> 本课目标：掌握 COUNT/SUM/AVG/MIN/MAX、GROUP BY 分组、HAVING 过滤组。这是报表、看板、统计类 API 的核心，也是 SQL 面试的重灾区。

---

## 1. 聚合函数：把很多行压成一行

```sql
SELECT
    COUNT(*)              AS product_cnt,    -- 总行数
    COUNT(comment)        AS commented,      -- 非 NULL 的 comment 数（等下用 reviews 试）
    SUM(price_cents)      AS total_value,    -- 求和
    AVG(price_cents)      AS avg_price,      -- 平均（注意返回浮点）
    MIN(price_cents)      AS min_price,
    MAX(price_cents)      AS max_price
FROM products;
```

关键规则：

1. **聚合函数忽略 NULL**（`COUNT(*)` 除外——它数行数）。`AVG(rating)` = SUM/COUNT(非空)，NULL 行不拉低平均分。
2. 聚合函数不能嵌套：`SUM(MAX(x))` ❌。
3. **没有 GROUP BY 时，SELECT 里出现聚合函数，整个查询就只能返回一行**，非聚合列会报错（PG）或随机取值（SQLite 的坑，MySQL 老版本也是）：

```sql
-- ❌ 语义不明：name 有 24 个值，COUNT 有 1 个值，怎么办？
SELECT name, COUNT(*) FROM products;         

-- ✅ 修正：要么去掉 name，要么 GROUP BY name
SELECT COUNT(*) FROM products;
```

## 2. GROUP BY：分组聚合

"每个分类的商品数/平均价"——**"每个 XX"就是 GROUP BY 的信号**：

```sql
SELECT
    category_id,
    COUNT(*)                    AS product_cnt,
    ROUND(AVG(price_cents), 0)  AS avg_price,
    SUM(stock)                  AS total_stock
FROM products
GROUP BY category_id
ORDER BY product_cnt DESC;
```

**铁律（SQL 标准，PG 严格执行）**：

> SELECT 里的**每一个非聚合列**，都必须出现在 GROUP BY 里。

```sql
-- ❌ PG 报错: column "products.name" must appear in GROUP BY
--    SQLite "宽容" 返回随机一行的 name —— 数据静默出错，最危险
SELECT category_id, name, COUNT(*) FROM products GROUP BY category_id;

-- ✅ 想带代表商品？聚合后才合法：
SELECT category_id, COUNT(*), MAX(name) FROM products GROUP BY category_id;
```

执行顺序回顾：`FROM → WHERE → GROUP BY → HAVING → SELECT → ORDER BY`，因此：

- **WHERE 过滤的是行（分组前），HAVING 过滤的是组（分组后）**；
- WHERE 里不能用聚合结果；HAVING 可以。
- SELECT 别名在 GROUP BY/HAVING 阶段还不存在（PG）；SQLite 放宽了，但跨方言请写标准形式。

## 3. HAVING：过滤聚合结果

```sql
-- 哪些分类的平均价高于 15 万？
SELECT category_id, ROUND(AVG(price_cents),0) AS avg_price
FROM products
GROUP BY category_id
HAVING AVG(price_cents) > 150000;      -- HAVING 用聚合表达式，不用别名（标准写法）

-- WHERE + HAVING 同台：只统计鼠标以外分类，且商品数 >= 4
SELECT category_id, COUNT(*) AS cnt
FROM products
WHERE category_id <> 3
GROUP BY category_id
HAVING COUNT(*) >= 4;
```

记忆法：**WHERE 挑"人"（行），HAVING 挑"组"**。执行效率上，能下推到 WHERE 的条件尽量写 WHERE（先过滤再分组，数据量更小）。

## 4. 多列分组

```sql
-- 每个用户每种订单状态的单数
SELECT user_id, status, COUNT(*) AS cnt
FROM orders
GROUP BY user_id, status
ORDER BY user_id, cnt DESC;
```

## 5. 条件聚合：CASE + 聚合 = 透视表

一行拿到多组指标，后端报表最常用的套路：

```sql
SELECT
    COUNT(*)                                              AS total_orders,
    SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) AS completed,
    SUM(CASE WHEN status = 'cancelled' THEN 1 ELSE 0 END) AS cancelled,
    SUM(CASE WHEN status IN ('paid','shipped') THEN 1 ELSE 0 END) AS in_progress
FROM orders;
```

PG 15+/SQLite 3.46+ 有更优雅的 `FILTER` 子句：

```sql
SELECT
    COUNT(*)                                           AS total_orders,
    COUNT(*) FILTER (WHERE status = 'completed')       AS completed,
    COUNT(*) FILTER (WHERE status = 'cancelled')       AS cancelled,
    AVG(rating)   FILTER (WHERE rating >= 4)           AS good_rating_avg   -- reviews 表
FROM orders;
```

条件聚合做**行转列（透视）**：

```sql
-- 每个分类：低价(<10万)/中价(10~30万)/高价(>=30万) 商品数
SELECT category_id,
    SUM(CASE WHEN price_cents <  100000 THEN 1 ELSE 0 END) AS low,
    SUM(CASE WHEN price_cents >= 100000 AND price_cents < 300000 THEN 1 ELSE 0 END) AS mid,
    SUM(CASE WHEN price_cents >= 300000 THEN 1 ELSE 0 END) AS high
FROM products
GROUP BY category_id;
```

## 6. 按表达式/日期分组

```sql
-- 按月统计订单量与销售额（ISO 文本前缀截取）
SELECT
    substr(created_at, 1, 7) AS month,    -- '2025-03'
    COUNT(*)                AS orders,
    SUM(total_cents)        AS revenue
FROM orders
GROUP BY substr(created_at, 1, 7)
ORDER BY month;
```

> ⚠️ `GROUP BY substr(created_at,1,7)` 没问题（分组键允许表达式），但 13 课会讲：**WHERE 里对列套函数会让索引失效**，两者别混淆。

## 7. 经典统计场景（shop 库实战）

```sql
-- 1. 每个用户的消费总额（只算有效单），取前 5 大客户
SELECT u.id, u.username, SUM(o.total_cents) AS spent
FROM users u
JOIN orders o ON o.user_id = u.id AND o.status <> 'cancelled'
GROUP BY u.id, u.username
ORDER BY spent DESC
LIMIT 5;

-- 2. 每个商品的评价数与平均分（LEFT JOIN：没评价的商品也要出现，06 课讲为什么）
SELECT p.id, p.name,
       COUNT(r.id)                   AS review_cnt,
       ROUND(AVG(r.rating), 2)       AS avg_rating
FROM products p
LEFT JOIN reviews r ON r.product_id = p.id
GROUP BY p.id, p.name
ORDER BY avg_rating DESC, review_cnt DESC;

-- 3. 月度客单价
SELECT substr(created_at,1,7) AS month,
       COUNT(*) AS orders,
       SUM(total_cents) AS revenue,
       ROUND(AVG(total_cents), 0) AS avg_order_value
FROM orders
WHERE status <> 'cancelled'
GROUP BY substr(created_at,1,7)
ORDER BY month;
```

场景 2 的 `COUNT(r.id)`：LEFT JOIN 产生 NULL 行时，`COUNT(列)` 数非空即自动变 0，而 `COUNT(*)` 会数出 1——**LEFT JOIN 后计数必须用 COUNT(右表列)**。

## 8. 常见坑

| 坑 | 说明 |
|---|---|
| SELECT 非聚合列不在 GROUP BY | SQLite 静默给随机值，PG 报错。永远遵守铁律 |
| WHERE 里写聚合条件 | 报错。聚合条件属于 HAVING |
| `COUNT(col)` vs `COUNT(*)` | 前者忽略 NULL，后者数行。LEFT JOIN 计数用前者 |
| AVG 溢出直觉 | 整数列 AVG 返回浮点；钱先除 100.0 再 ROUND 展示 |
| HAVING 用别名 | PG 报错（别名此阶段不存在），重复聚合表达式 |
| 分组键含 NULL | 所有 NULL 落进同一组（合理行为，但要心里有数） |

## 9. Rust 视角

报表接口 = 聚合 SQL + struct 映射：

```rust
#[derive(sqlx::FromRow, Serialize)]
struct CategoryStat {
    category_id: i64,
    product_cnt: i64,
    avg_price: Option<f64>,   // 空组可能返回 NULL → Option
    total_stock: i64,
}

let stats = sqlx::query_as::<_, CategoryStat>(
    "SELECT category_id, COUNT(*) product_cnt, AVG(price_cents) avg_price, SUM(stock) total_stock
     FROM products GROUP BY category_id ORDER BY product_cnt DESC"
).fetch_all(&pool).await?;
```

类型细节：SQLite 的 `COUNT`/`SUM` 进 Rust 是 `i64`，`AVG` 是 `f64`；PG 的 `SUM(numeric)` 进来是 `String`（sqlx 为精度安全）——**方言差异会传导到 Rust 类型**，17 课细讲。

---

## 练习

1. 每个分类的商品数、总库存、平均价格（保留 0 位小数），按平均价降序。
2. 每种订单状态的订单数与总金额（一列状态、一列数量、一列金额）。
3. 评价数 ≥ 2 的商品的平均分排行（商品名、评价数、平均分，平均分降序）。
4. 2025 年 3 月（含）以后创建的订单，按用户分组：单数、总消费，只保留总消费 ≥ 20 万的用户。（WHERE + GROUP BY + HAVING 全用上）
5. 透视练习：每个用户的 1~2 星、3 星、4~5 星评价数各多少（条件聚合）。

<details><summary>答案</summary>

```sql
-- 1.
SELECT category_id, COUNT(*) cnt, SUM(stock) stock, ROUND(AVG(price_cents),0) avg_price
FROM products GROUP BY category_id ORDER BY avg_price DESC;

-- 2.
SELECT status, COUNT(*) cnt, SUM(total_cents) amount
FROM orders GROUP BY status ORDER BY amount DESC;

-- 3.
SELECT p.name, COUNT(*) cnt, ROUND(AVG(r.rating),2) avg_rating
FROM reviews r JOIN products p ON p.id = r.product_id
GROUP BY p.id, p.name
HAVING COUNT(*) >= 2
ORDER BY avg_rating DESC;

-- 4.
SELECT u.username, COUNT(*) cnt, SUM(o.total_cents) spent
FROM orders o JOIN users u ON u.id = o.user_id
WHERE o.created_at >= '2025-03-01' AND o.status <> 'cancelled'
GROUP BY u.id, u.username
HAVING SUM(o.total_cents) >= 200000
ORDER BY spent DESC;

-- 5.
SELECT u.username,
       SUM(CASE WHEN r.rating <= 2 THEN 1 ELSE 0 END) AS bad,
       SUM(CASE WHEN r.rating =  3 THEN 1 ELSE 0 END) AS mid,
       SUM(CASE WHEN r.rating >= 4 THEN 1 ELSE 0 END) AS good
FROM reviews r JOIN users u ON u.id = r.user_id
GROUP BY u.id, u.username;
```
</details>

---

← 上一课：[04 · 函数与 CASE](04-函数与CASE.md) | 下一课：[06 · JOIN 多表查询](06-JOIN多表查询.md) · [返回目录](../README.md)
