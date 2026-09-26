# 03 · WHERE 过滤

> 本课目标：熟练使用比较/逻辑运算符、IN/BETWEEN/LIKE、并彻底搞懂 NULL 的三值逻辑——这是写查询和后端接口筛选参数的基础。

---

## 1. WHERE 在执行顺序中的位置

回顾 01 课：**FROM → WHERE → ... → SELECT**。WHERE 作用在"原始行"上，**此时 SELECT 的别名还不存在**：

```sql
-- ❌ 报错：discount_price 在 WHERE 里不可见（先过滤后投影）
SELECT price_cents * 0.9 AS discount_price
FROM products WHERE discount_price > 100000;

-- ✅ 正确：把表达式重复一遍，或用子查询/ CTE（07 课）
SELECT price_cents * 0.9 AS discount_price
FROM products WHERE price_cents * 0.9 > 100000;
```

> SQLite 恰好"宽容"地支持别名进 WHERE，但这是非标准行为，别依赖——PG/MySQL 都会报错。

## 2. 比较运算符

```sql
=   != (或 <>)   <   <=   >   >=
```

注意 `<>` 是 SQL 标准，`!=` 各家也都支持。字符串比较按**排序规则（collation）**，默认区分大小写（MySQL 默认列不区分，PG 区分）：

```sql
SELECT 'ABC' = 'abc';   -- SQLite/PG: 0/false；MySQL 默认: 1/true

-- PG 忽略大小写用 ILIKE 或 lower() 比较
```

## 3. 逻辑运算符与三值逻辑

`AND` `OR` `NOT`，但 SQL 的布尔有三态：**TRUE / FALSE / NULL**。

真值表（记住这张就够了）：

| AND | T | F | N |     | OR | T | F | N |
|-----|---|---|---|-----|----|---|---|---|
| **T** | T | F | N |     | **T** | T | T | T |
| **F** | F | F | F |     | **F** | T | F | N |
| **N** | N | F | N |     | **N** | T | N | N |

WHERE 只保留结果为 **TRUE** 的行（FALSE 和 NULL 都被过滤掉）。

### 经典陷阱：NOT IN + NULL

```sql
-- comment 里有 NULL 行时：
SELECT COUNT(*) FROM reviews WHERE comment != '性价比高';   -- 18（漏掉了 NULL 的行）
SELECT COUNT(*) FROM reviews WHERE comment IS NOT NULL;      -- 21

-- 更致命的 NOT IN（07 课还会遇到子查询版）：
SELECT * FROM reviews WHERE rating NOT IN (3, NULL);
-- rating=3 的行: 3 NOT IN (3,NULL) → FALSE ✔ 排除
-- rating=5 的行: 5 NOT IN (3,NULL) → 5<>3 AND 5<>NULL → TRUE AND NULL → NULL ✘ 也被排除！
-- 结果：0 行。永远别让 NOT IN 的列表里出现 NULL。
```

## 4. NULL 判断：IS NULL / IS NOT NULL / 安全等号

```sql
-- 唯一正确写法
SELECT COUNT(*) FROM reviews WHERE comment IS NULL;        -- 0（种子数据都有评论，试试别的列）
SELECT COUNT(*) FROM products WHERE stock = 0;             -- 0

-- PG/MySQL 支持 NULL 安全等号：
--   PG:      IS NOT DISTINCT FROM
--   MySQL:   <=>
```

**后端实务**：筛"未支付订单"别写 `WHERE paid_at = NULL`，要写 `paid_at IS NULL`。同理更新时 `SET x = NULL` 是合法的（赋值语境），判断语境必须用 IS。

## 5. 范围与集合：BETWEEN / IN

```sql
-- BETWEEN 是闭区间 [a, b]
SELECT name, price_cents FROM products
WHERE price_cents BETWEEN 50000 AND 150000;

-- IN：集合成员
SELECT name FROM products
WHERE category_id IN (1, 3);          -- 键盘或鼠标

-- IN (子查询)：06 课后大量使用
SELECT name FROM products
WHERE id IN (SELECT product_id FROM order_items WHERE quantity >= 5);
```

`NOT IN` 语义是 `<> ALL`，配合子查询有 NULL 陷阱（见上）。**否定集合匹配优先用 `NOT EXISTS`**（07 课）。

## 6. 模糊匹配：LIKE

```sql
SELECT name FROM products WHERE name LIKE '%Pro%';      -- 任意位置含 Pro
SELECT name FROM products WHERE name LIKE 'Logitech%';  -- 以 Logitech 开头

-- 通配符:
--   %  0 个或多个任意字符
--   _  恰好 1 个任意字符
SELECT name FROM products WHERE name LIKE '________';   -- 恰好 8 个字符的名字

-- 转义字面的 % 和 _：ESCAPE 指定转义符
SELECT 'a_b' LIKE 'a\_b' ESCAPE '\';    -- 1

-- 中文字符注意：SQLite 的 LIKE 默认只对 ASCII 大小写不敏感
SELECT 'ABC' LIKE 'abc';                -- 1
```

| 方言 | 大小写 | 更强匹配 |
|------|--------|----------|
| SQLite | LIKE 不区分 ASCII 大小写 | 装 ICU / 用 FTS5（15 课）|
| PG | LIKE 区分；`ILIKE` 不区分 | `SIMILAR TO` / 正则 `~` |
| MySQL | 取决于 collation | RLIKE（正则）|

> 后端搜索框别直接 `LIKE '%kw%'`：**前导 `%` 让索引失效**（13 课），且无法处理中文分词。量大用全文索引（15 课 FTS/PG tsvector）。

## 7. 运算符优先级与括号

`NOT` > `比较` > `AND` > `OR`。**团队规范：混用时永远加括号**，可读性和正确性双赢：

```sql
-- 需求：键盘类 且（低价 或 无库存）
SELECT name, category_id, price_cents, stock
FROM products
WHERE category_id = 1
  AND (price_cents < 100000 OR stock = 0);

-- 不加括号会变成 (cat=1 AND price<100000) OR stock=0 —— 语义完全变了
```

## 8. 综合示例：一个列表页的筛选

需求：商品列表页，参数——分类 1/3、价格 5 万~15 万、关键字 "Pro"、按价格降序、第 1 页 10 条：

```sql
SELECT id, name, category_id, price_cents, stock
FROM products
WHERE category_id IN (1, 3)
  AND price_cents BETWEEN 50000 AND 150000
  AND name LIKE '%Pro%'
ORDER BY price_cents DESC
LIMIT 10;
```

这就是 Axum handler 里 `GET /products?cat=1,3&min=500&max=1500&q=Pro&page=1` 的 SQL 原型——动态拼 WHERE 时**一律用参数绑定**（16 课，防 SQL 注入）。

## 9. 常见坑

| 坑 | 现象 | 修正 |
|---|---|---|
| `= NULL` / `!= NULL` | 永远匹配不到 | `IS NULL` / `IS NOT NULL` |
| NOT IN 列表/子查询含 NULL | 返回 0 行 | NOT EXISTS 或先滤 NULL |
| WHERE 用 SELECT 别名 | PG/MySQL 报错 | 重复表达式 / 子查询 / CTE |
| `LIKE '%x%'` 大表 | 全表扫描 | 前缀匹配/全文索引 |
| AND/OR 不加括号 | 静默查出错误数据 | 强制括号 |

## 10. Rust 视角

sqlx 参数绑定，**字符串拼接是重大安全漏洞**：

```rust
// ❌ SQL 注入！用户输入 q = "x'; DROP TABLE users;--" 就完蛋
let sql = format!("SELECT * FROM products WHERE name LIKE '%{}%'", q);

// ✅ 占位符 ?（SQLite/PG: $1）由驱动安全转义
let rows: Vec<(i64, String)> = sqlx::query_as(
    "SELECT id, name FROM products WHERE name LIKE '%' || ? || '%'"
)
.bind(format!("%{}%", q))       // LIKE 的 % 放在参数里
.fetch_all(&pool).await?;
```

可空筛选参数（不传就不过滤）用动态 SQL 构造器 `QueryBuilder`（17 课）。

---

## 练习

1. 查询价格低于 100 元（10000 分）**或**库存为 0 的商品，显示名称、价格、库存。
2. 查询 2025 年 3 月创建的订单（created_at 是 TEXT，用 `LIKE '2025-03%'`），按日期升序。
3. 查询评价里 rating 为 4 或 5、且 comment 包含"书"以外任何主题的行……开玩笑的，做这个：rating IN (4,5) 且 comment 不含 "性价比"，注意 NULL。
4. `SELECT COUNT(*) FROM products WHERE price_cents BETWEEN 80000 AND 150000;` 与 `>= 80000 AND <= 150000` 是否等价？验证之。
5. 想查"名字恰好 10~12 个字符"的商品，一条 SQL（提示：LENGTH）。

<details><summary>答案</summary>

```sql
-- 1.
SELECT name, price_cents, stock FROM products
WHERE price_cents < 10000 OR stock = 0;

-- 2.
SELECT id, user_id, created_at FROM orders
WHERE created_at LIKE '2025-03%' ORDER BY created_at;

-- 3. comment 全部非空，但养成习惯写 IS NOT NULL 防御 NULL：
SELECT id, rating, comment FROM reviews
WHERE rating IN (4, 5)
  AND comment NOT LIKE '%性价比%'
  AND comment IS NOT NULL;

-- 4. 等价。BETWEEN 是 >= AND <= 的语法糖（闭区间）。
SELECT COUNT(*) FROM products WHERE price_cents >= 80000 AND price_cents <= 150000;

-- 5.
SELECT name, LENGTH(name) AS len FROM products
WHERE LENGTH(name) BETWEEN 10 AND 12;
```
</details>

---

← 上一课：[02 · SELECT 基础](02-SELECT基础.md) | 下一课：[04 · 函数与 CASE](04-函数与CASE.md) · [返回目录](../README.md)
