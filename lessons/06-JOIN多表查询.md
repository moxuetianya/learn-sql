# 06 · JOIN 多表查询

> 本课目标：彻底掌握 INNER/LEFT/RIGHT/FULL/CROSS/SELF JOIN，理解"驱动表"和 join 语义。多表查询是 SQL 的分水岭——过了这关才算入门。

---

## 1. 为什么需要 JOIN

关系数据库的核心思想：**把数据按实体拆表，用时再拼回来**。

shop 库里 `orders.user_id` 只是数字，用户名在 `users` 表；`order_items` 只有 id，商品名在 `products` 表。JOIN 就是按关联条件把行配对。

```sql
-- 最常见的两表查询：订单 + 下单人的名字
SELECT o.id, o.status, o.total_cents, u.username
FROM orders o
JOIN users u ON u.id = o.user_id       -- ON = 配对条件
LIMIT 5;
```

**别名规范**：表别名用短名（`orders o`、`users u`），且一旦起了别名，全查询只能用别名。JOIN 后引用列**一律带表前缀**（`u.username`），避免两张表有同名列（id、created_at）时分不清。

## 2. INNER JOIN：只保留配对成功的行

```sql
SELECT p.name AS product, c.name AS category
FROM products p
INNER JOIN categories c ON c.id = p.category_id      -- INNER 可省略
ORDER BY c.id;
```

用集合语言：**结果 = 左表 ⋈ 右表中满足 ON 的组合**。products.category_id 全部有效，所以 24 行都在。若某商品的 category_id 是 NULL 或不存在，该行**静默消失**——INNER JOIN 会"吃掉"配不上的行。

## 3. LEFT JOIN：保留左表全部行

```sql
-- 每个分类的商品数，包括一个商品都没有的分类
SELECT c.id, c.name, COUNT(p.id) AS product_cnt
FROM categories c
LEFT JOIN products p ON p.category_id = c.id
GROUP BY c.id, c.name
ORDER BY product_cnt;
```

规则：左表每行至少出现一次；右表配不上的列填 **NULL**（不是 0、不是空串）。

> 因此 **LEFT JOIN 后计数/判空要用右表的列**：`COUNT(p.id)`（NULL 不计数，自动 0）；`WHERE p.id IS NULL` 就是经典"反连接"——找出**没有**商品的分类：

```sql
SELECT c.name
FROM categories c
LEFT JOIN products p ON p.category_id = c.id
WHERE p.id IS NULL;      -- 反连接：shop 库现在没有这种分类，输出 0 行
```

### ⚠️ LEFT JOIN 上加 WHERE 的坑

```sql
-- 需求：所有用户及其"已完成"订单数（没下单/没完成单的用户也要 0）

-- ❌ WHERE 作用在 join 之后，把 NULL 行全杀了，退化成 INNER JOIN
SELECT u.username, COUNT(o.id)
FROM users u LEFT JOIN orders o ON o.user_id = u.id
WHERE o.status = 'completed'
GROUP BY u.username;

-- ✅ 右表条件放 ON（join 时就过滤），左表条件才放 WHERE
SELECT u.username, COUNT(o.id) AS done_cnt
FROM users u
LEFT JOIN orders o ON o.user_id = u.id AND o.status = 'completed'
GROUP BY u.username;
```

**口诀：LEFT JOIN 时，右表的筛选条件写 ON，左表的写 WHERE。**（INNER JOIN 两者等价。）

## 4. RIGHT JOIN 与 FULL JOIN

```sql
-- RIGHT JOIN：保留右表全部（等价于交换两表的 LEFT JOIN）
SELECT p.name, c.name
FROM products p RIGHT JOIN categories c ON c.id = p.category_id;

-- FULL JOIN：两边都保留（SQLite 3.39+ / PG 支持；MySQL 不支持，用 UNION 模拟）
SELECT u.username, r.id AS review_id
FROM users u FULL JOIN reviews r ON r.user_id = u.id;
```

工程习惯：**统一用 LEFT JOIN，把"主表"放左边**。RIGHT JOIN 可读性差（团队 review 时总要看半天谁是主），FULL JOIN 用于数据对账/差异比较。

## 5. 多表 JOIN：链式配对

```sql
-- 订单明细报表：谁、买了什么、几个、单价多少、什么状态
SELECT
    o.id           AS order_id,
    u.username,
    p.name         AS product,
    oi.quantity,
    oi.unit_price_cents,
    o.status
FROM orders o
JOIN users        u  ON u.id = o.user_id
JOIN order_items  oi ON oi.order_id = o.id
JOIN products     p  ON p.id = oi.product_id
ORDER BY o.id, p.id
LIMIT 10;
```

N 张表 = N-1 个 JOIN 条件。执行时数据库按优化器选的顺序两两配对（13 课 EXPLAIN 可见），你写的顺序 ≠ 执行顺序，但**语义按你写的顺序**确定。

> ⚠️ **join 后行数膨胀**：订单 20 行，join order_items 后变 28 行（一单多件拆多行）。做"订单数统计"时要么 `COUNT(DISTINCT o.id)`，要么在子查询里先聚 order_items（07 课实战）。

## 6. CROSS JOIN：笛卡尔积

```sql
-- 21 世纪了，几乎只用于生成组合：用户 × 分类 = 推荐位矩阵
SELECT u.username, c.name AS category
FROM users u CROSS JOIN categories c
LIMIT 5;     -- 8 × 6 = 48 行

-- ⚠️ 忘写 ON 的 INNER JOIN 就是笛卡尔积（事故现场）：
SELECT COUNT(*) FROM orders o JOIN order_items oi;    -- 20×28=560 行，全是错的
```

## 7. SELF JOIN：自己配自己

```sql
-- 场景：员工表 manager_id 指向同表的员工 id。shop 库没有员工表，造一个演示：
-- 找出"同一分类下价格最接近的商品对"
SELECT a.name AS cheaper, b.name AS pricier,
       b.price_cents - a.price_cents AS diff
FROM products a
JOIN products b
  ON a.category_id = b.category_id
 AND a.id < b.id                       -- a.id < b.id 去重（避免 a,b 和 b,a 重复）
 AND b.price_cents > a.price_cents
ORDER BY diff
LIMIT 5;
```

SELF JOIN 必须**用两个不同别名**把一张表当两张用。`a.id < b.id` 是经典的"无序配对去重"技巧，会反复出现。

## 8. JOIN 图鉴速查

```
INNER:      A ∩ B        只留配对成功的
LEFT:       A 全保留     B 缺的补 NULL
RIGHT:      B 全保留     A 缺的补 NULL
FULL:       A ∪ B       两边都保留
CROSS:      A × B       所有组合
LEFT + IS NULL（反连接）: A - B
```

## 9. 综合示例：商品销售榜（多表 + 聚合）

```sql
-- 销量榜：商品名、分类名、卖出的件数、销售额，只看有效订单
SELECT
    p.name,
    c.name            AS category,
    SUM(oi.quantity)  AS sold_qty,
    SUM(oi.quantity * oi.unit_price_cents) AS revenue
FROM order_items oi
JOIN orders   o ON o.id = oi.order_id AND o.status <> 'cancelled'
JOIN products p ON p.id = oi.product_id
JOIN categories c ON c.id = p.category_id
GROUP BY p.id, p.name, c.name
ORDER BY sold_qty DESC, revenue DESC
LIMIT 10;
```

注意两个细节：

1. `ON o.id = oi.order_id AND o.status <> 'cancelled'`——INNER JOIN 的右表条件放 ON/WHERE 等价，放 ON 让"业务口径"内聚在一处；
2. GROUP BY 用 `p.id, p.name, c.name`（id 保证唯一，name 是展示冗余，SQLite/PG 的 `GROUP BY p.id` + 选 name 也合法但方言有差异，写全最稳）。

## 10. 常见坑

| 坑 | 后果 | 对策 |
|---|---|---|
| 忘写 ON | 笛卡尔积，行数爆炸 | INNER JOIN 必有 ON；上线前看 EXPLAIN 行数 |
| LEFT JOIN 右表条件写 WHERE | 退化成 INNER JOIN，"补 0"需求出错 | 右表条件进 ON |
| LEFT JOIN 后 COUNT(*) | 空组数出 1 | COUNT(右表主键) |
| join 膨胀后直接 COUNT | 一单多件把订单数数多 | COUNT(DISTINCT) 或先聚合再 join |
| 列不带表前缀 | 同名列（id/created_at）歧义报错 | join 后全部 `alias.col` |
| 用 IN 子查询代替 JOIN | 语义对但性能常更差 | 优先 JOIN，07 课对比 |

## 11. Rust 视角

多表查询结果没有对应的"表"，为它建**只读投影 struct**（DTO），别想着 join 出来的行塞进某个 entity：

```rust
#[derive(sqlx::FromRow, serde::Serialize)]
struct OrderRow {
    order_id: i64,
    username: String,
    product: String,
    quantity: i64,
    unit_price_cents: i64,
    status: String,
}

let rows = sqlx::query_as::<_, OrderRow>(
    "SELECT o.id order_id, u.username, p.name product, oi.quantity,
            oi.unit_price_cents, o.status
     FROM orders o
     JOIN users u ON u.id = o.user_id
     JOIN order_items oi ON oi.order_id = o.id
     JOIN products p ON p.id = oi.product_id
     ORDER BY o.id LIMIT 10"
).fetch_all(&pool).await?;
```

这正是"SQL join + 扁平 DTO"模式——GraphQL 那种嵌套结构（订单里嵌 items 数组）要用 `json_agg`（15 课）或两次查询在 Rust 里组装。**不要**为了嵌套 JSON 在 Rust 里循环 N+1 次查询。

---

## 练习

1. 每个订单的 id、用户名、用户邮箱（三表都不要，两表即可）、订单状态，按订单 id 排序前 10。
2. 所有用户及其订单数（**没下过单的用户显示 0**），按订单数降序。
3. 有评价的商品的平均分排行：商品名、分类名、平均分、评价数（只显示有评价的），平均分相同按评价数多者优先。
4. 找出**从未被下单**的商品名（两种方法：LEFT JOIN 反连接、NOT EXISTS/NOT IN 各写一版）。
5. 每个分类的销售额（只算非取消订单），没销售额的显示 0。（注意 LEFT JOIN 放置顺序与 COUNT/SUM 的 NULL 处理）

<details><summary>答案</summary>

```sql
-- 1.
SELECT o.id, u.username, u.email, o.status
FROM orders o JOIN users u ON u.id = o.user_id
ORDER BY o.id LIMIT 10;

-- 2.
SELECT u.username, COUNT(o.id) AS order_cnt
FROM users u LEFT JOIN orders o ON o.user_id = u.id
GROUP BY u.id, u.username
ORDER BY order_cnt DESC;
-- 注意 COUNT(o.id) 而非 COUNT(*)，否则没订单的用户也数出 1

-- 3.
SELECT p.name, c.name AS category, ROUND(AVG(r.rating),2) avg_rating, COUNT(*) cnt
FROM reviews r
JOIN products p ON p.id = r.product_id
JOIN categories c ON c.id = p.category_id
GROUP BY p.id, p.name, c.name
ORDER BY avg_rating DESC, cnt DESC;

-- 4.
-- 方法A: 反连接
SELECT p.name
FROM products p
LEFT JOIN order_items oi ON oi.product_id = p.id
WHERE oi.order_id IS NULL;

-- 方法B: NOT EXISTS（推荐，见 07 课）
SELECT p.name FROM products p
WHERE NOT EXISTS (SELECT 1 FROM order_items oi WHERE oi.product_id = p.id);

-- 方法C: NOT IN（本例 product_id 非 NULL 安全可用，但子查询有 NULL 时会翻车）
SELECT name FROM products
WHERE id NOT IN (SELECT product_id FROM order_items);

-- 5. 以 categories 为主表 LEFT JOIN（先聚合子查询，避免膨胀影响）：
SELECT c.name, COALESCE(s.revenue, 0) AS revenue
FROM categories c
LEFT JOIN (
    SELECT p.category_id AS cid, SUM(oi.quantity * oi.unit_price_cents) AS revenue
    FROM order_items oi
    JOIN orders o ON o.id = oi.order_id AND o.status <> 'cancelled'
    JOIN products p ON p.id = oi.product_id
    GROUP BY p.category_id
) s ON s.cid = c.id
ORDER BY revenue DESC;
```
</details>

---

← 上一课：[05 · 聚合与 GROUP BY](05-聚合与GROUP-BY.md) | 下一课：[07 · 子查询与 CTE](07-子查询与CTE.md) · [返回目录](../README.md)
