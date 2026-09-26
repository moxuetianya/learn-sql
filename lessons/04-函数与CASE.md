# 04 · 函数与 CASE 表达式

> 本课目标：掌握字符串/数值/日期/NULL 处理函数和 CASE 分支逻辑，学会方言差异的应对策略。函数是"在数据库里就把数据加工好"的关键。

---

## 1. 总原则：能在 SQL 算的别搬回 Rust

Rust 后端常见反模式：`SELECT * FROM orders` 拉全表到内存，再在 Rust 里过滤、分组、格式化。正确做法是用 SQL 函数在库内完成，只传结果。**数据向计算移动，而不是计算向数据移动。**

方言提醒：函数是 SQL 方言差异最大的部分。本课标 ✅ 的是 SQLite/PG 通用，标注方言的需注意。

## 2. 字符串函数

```sql
-- 长度（字符数 vs 字节数！）
SELECT name, LENGTH(name) AS chars, LENGTH(CAST(name AS BLOB)) AS bytes
FROM products LIMIT 3;
-- 《Rust 程序设计语言》: 12 个字符，36 字节（UTF-8 中文 3 字节）

-- 拼接：|| 是标准操作符（PG/SQLite），MySQL 用 CONCAT()
SELECT username || ' <' || email || '>' AS display FROM users LIMIT 2;

-- 大小写
SELECT UPPER('abc'), LOWER('ABC');

-- 去空白（PG：TRIM/BTRIM；SQLite 3.32+ 支持 TRIM(x, chars)）
SELECT TRIM('  hello  ');

-- 截取（SQL 标准：起始位 1，不是 0！）
SELECT SUBSTR('HHKB Professional 2', 1, 4);   -- 'HHKB'
SELECT SUBSTRING('HHKB Professional 2', 1, 4); -- PG 同义

-- 替换
SELECT REPLACE('AOC 24B2XH', 'B2', 'XX');

-- 定位（找不到返回 0，不是 -1）
SELECT INSTR('Logitech MX Master 3S', 'MX');
```

**后端高频场景**：脱敏。

```sql
-- 邮箱打码: alice@example.com → a****@example.com
SELECT username,
       SUBSTR(email, 1, 1) || '****' || SUBSTR(email, INSTR(email, '@')) AS masked
FROM users;
```

## 3. 数值函数

```sql
SELECT
    ROUND(3.14159, 2),        -- 3.14  四舍五入到 2 位
    ABS(-5),                  -- 5
    -- SQLite 整数除法自动截断；PG 视类型而定
    7 / 2,                    -- SQLite: 3   (整数/整数)
    7 / 2.0,                  -- 3.5
    7 % 2,                    -- 1  取模（PG 也写作 mod(7,2)）
    CEIL(3.2), FLOOR(3.8);    -- 4, 3  （SQLite 3.35+；PG 原生）
```

⚠️ **钱的计算**：`price_cents` 全程整数运算，只在**展示**时 `/100.0` 并 ROUND。别在中间步骤引入浮点。

## 4. 日期时间函数

```sql
-- SQLite --------------------------------------------------
SELECT datetime('now');                    -- UTC 当前时间 '2026-09-27 00:00:00'
SELECT date('now', '+7 days');             -- 加 7 天
SELECT strftime('%Y-%m', '2025-03-15');    -- '2025-03' 格式化
SELECT julianday('now') - julianday('2025-01-01');  -- 天数差

-- PostgreSQL ----------------------------------------------
SELECT now();                              -- 带时区的时间戳
SELECT now() + INTERVAL '7 days';
SELECT to_char(now(), 'YYYY-MM');
SELECT AGE(TIMESTAMP '2025-03-15', TIMESTAMP '2025-01-01');  -- 2 mons 14 days
SELECT EXTRACT(MONTH FROM TIMESTAMP '2025-03-15');           -- 3

-- 通用技巧：shop 库 created_at 是 TEXT(ISO)，字符串比较即日期比较
SELECT COUNT(*) FROM orders
WHERE created_at >= '2025-03-01' AND created_at < '2025-04-01';   -- 3 月订单 7 单
```

> **索引友好写法**：日期范围用 `>= 月初 AND < 次月初`（闭开区间），别对列套函数（`WHERE strftime('%Y-%m', created_at) = '2025-03'` 会让索引失效，13 课）。

## 5. NULL 处理函数（极高频）

```sql
-- COALESCE: 取第一个非 NULL 的参数（全平台标准，必须掌握）
SELECT COALESCE(NULL, NULL, 'fallback');            -- 'fallback'
SELECT COALESCE(comment, '（无评价文字）') FROM reviews;

-- NULLIF(a, b): a=b 时返回 NULL，否则 a —— 防除零神器
SELECT 100 / NULLIF(stock, 0) FROM products;        -- stock=0 → NULL 而不是报错/Infinity

-- PG 还有简洁版（SQLite 3.46+ 才支持）：
SELECT rating FILTER (WHERE rating >= 4) FROM reviews;  -- 05 课细讲
```

`COALESCE(x, 0)` 也是 Rust 端减负利器：可空列在 SQL 里给默认值，Rust struct 就能用 `i64` 而非 `Option<i64>`。

## 6. 类型转换

```sql
-- 标准 CAST
SELECT CAST('123' AS INTEGER) + 1;        -- 124
SELECT CAST(price_cents / 100.0 AS REAL); -- SQLite
SELECT CAST('2025-03-15' AS DATE);        -- PG

-- PG 惯用简写 ::（SQLite 不支持）
SELECT '123'::INT;
SELECT now()::DATE;

-- SQLite 的动态类型：TEXT 列存了 'abc' 再 CAST AS INTEGER → 0，静默出错
-- 所以应用层类型纪律在 SQLite 上更重要
```

## 7. CASE：SQL 里的 match 表达式

两种形态，对应 Rust 的 `match` 与 `if else` 链：

```sql
-- 形态1：简单 CASE（值匹配） —— 类似 match x { 1=>.., 2=>.., _=>.. }
SELECT status,
       CASE status
         WHEN 'pending'   THEN '待支付'
         WHEN 'paid'      THEN '已支付'
         WHEN 'shipped'   THEN '运输中'
         WHEN 'completed' THEN '已完成'
         ELSE '已取消'          -- 相当于 _
       END AS status_cn
FROM orders LIMIT 5;

-- 形态2：搜索 CASE（条件匹配，更常用）—— 类似 if/else if 链
SELECT name, price_cents,
       CASE
         WHEN price_cents >= 300000 THEN '高端'
         WHEN price_cents >= 100000 THEN '中端'
         ELSE '入门'
       END AS tier
FROM products;

-- CASE 是表达式：可以出现在 SELECT/WHERE/ORDER BY/GROUP BY 任何地方
-- 用途1: 自定义排序（把 pending 排最前）
SELECT id, status FROM orders
ORDER BY CASE status WHEN 'pending' THEN 0 WHEN 'paid' THEN 1 ELSE 2 END, id;

-- 用途2: 条件计数（05 课聚合前传）
SELECT
    COUNT(*)                                        AS total,
    SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) AS completed_cnt,
    SUM(CASE WHEN status = 'cancelled' THEN 1 ELSE 0 END) AS cancelled_cnt
FROM orders;
```

> 注意：CASE 找到第一个满足的分支就停（惰性），无 ELSE 且都不匹配返回 NULL——写聚合时永远带 ELSE。

## 8. 综合示例：订单状态报表

```sql
SELECT
    o.id,
    u.username,
    o.total_cents,
    ROUND(o.total_cents / 100.0, 2)                          AS total_yuan,
    CASE WHEN o.total_cents >= 300000 THEN '大单'
         WHEN o.total_cents >= 100000 THEN '中单'
         ELSE '小单' END                                      AS grade,
    COALESCE(NULLIF(o.status, 'cancelled'), '正常流程')       AS flow
FROM orders o
JOIN users u ON u.id = o.user_id      -- JOIN 06 课讲，先照抄
ORDER BY o.total_cents DESC
LIMIT 10;
```

`COALESCE(NULLIF(col, 'x'), 'y')` 组合："把 x 视作空，再兜底为 y"，一行解决多种脏值。

## 9. 常见坑

| 坑 | 说明 |
|---|---|
| `INSTR`/`SUBSTR` 起始位是 1 | 写成 0 在 SQLite 里也能跑但语义错乱，PG 直接报错 |
| 整数除法 | `7/2=3`；要小数先 `*1.0` 或 `/100.0` |
| 对索引列套函数过滤 | `WHERE UPPER(name)='X'` 索引失效（13 课；PG 可建函数索引） |
| MySQL 的 `CONCAT` | PG/SQLite 用 `\|\|`；跨方言代码别混 |
| CASE 忘写 ELSE | 分支外返回 NULL，聚合结果悄悄缺数 |

## 10. Rust 视角

- sqlx 的 `query!` 宏**编译期连接数据库**推断表达式类型，`COALESCE` 的返回类型会被正确推断为非空 → 直接映射 `i64`；
- 日期时间列 PG `TIMESTAMPTZ` ↔ `chrono::DateTime<Utc>`，SQLite TEXT ↔ 需要约定格式（16 课讲 SQLite 的 chrono 兼容层）；
- CASE 的分层逻辑如果太复杂（10+ 分支），考虑落库为字典表 + JOIN，别让 SQL 变成业务逻辑垃圾场。

---

## 练习

1. 把所有用户邮箱打码成 `a****@domain` 形式（首字符 + 四个星 + @ 后完整保留）。
2. 给每个订单算 `grade`：≥30 万"大单"、≥10 万"中单"、否则"小单"，统计每种 grade 的数量（提示：外层套 COUNT + GROUP BY，或等 05 课）。
3. 查询评价表中 comment 的字符长度与字节长度，找出"字符数≠字节数/3 以内"的行（即含中文的评论）。
4. 用 NULLIF 防 0：算每个商品 `price_cents / stock`（库存 0 时显示 NULL），注意 stock 恰好没有 0 的种子数据——先 `UPDATE products SET stock=0 WHERE id=11` 再算，算完 `./setup/init.sh` 重置。
5. 一条 SQL 把 users.created_at 转成 `YYYY年MM月` 格式（substr 拼接或 strftime）。

<details><summary>答案</summary>

```sql
-- 1.
SELECT username,
       SUBSTR(email, 1, 1) || '****' || SUBSTR(email, INSTR(email, '@')) AS masked
FROM users;

-- 2.
SELECT CASE WHEN total_cents >= 300000 THEN '大单'
            WHEN total_cents >= 100000 THEN '中单'
            ELSE '小单' END AS grade,
       COUNT(*)
FROM orders
GROUP BY grade;    -- GROUP BY 别名 SQLite 支持；PG 需重复表达式（05 课）

-- 3.
SELECT comment, LENGTH(comment) AS chars, LENGTH(CAST(comment AS BLOB)) AS bytes
FROM reviews
WHERE bytes > chars * 3;   -- SQLite 允许在 WHERE 用别名；PG 重复表达式

-- 4.
SELECT name, price_cents, stock, price_cents / NULLIF(stock, 0) AS unit_by_stock
FROM products;

-- 5. (SQLite)
SELECT username, strftime('%Y年%m月', created_at) FROM users;
-- 纯 substr 版: substr(created_at,1,4) || '年' || substr(created_at,6,2) || '月'
```
</details>

---

← 上一课：[03 · WHERE 过滤](03-WHERE过滤.md) | 下一课：[05 · 聚合与 GROUP BY](05-聚合与GROUP-BY.md) · [返回目录](../README.md)
