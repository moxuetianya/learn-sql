# 15 · 视图、序列、触发器、JSON 与全文搜索

> 本课目标：掌握数据库侧的"进阶武器"——视图/物化视图、序列、触发器、JSON 列与全文索引，以及何时**不该**用它们。本课起 PostgreSQL 专属内容变多（会明确标注）。

---

## 1. 视图（VIEW）：保存下来的查询

```sql
-- 把"商品销量榜"查询固化为视图
CREATE VIEW v_product_sales AS
SELECT
    p.id, p.name, c.name AS category,
    COALESCE(SUM(oi.quantity), 0)                          AS sold_qty,
    COALESCE(SUM(oi.quantity * oi.unit_price_cents), 0)    AS revenue
FROM products p
JOIN categories c ON c.id = p.category_id
LEFT JOIN order_items oi ON oi.product_id = p.id
LEFT JOIN orders o ON o.id = oi.order_id AND o.status <> 'cancelled'
GROUP BY p.id, p.name, c.name;

-- 用起来就像一张表
SELECT * FROM v_product_sales ORDER BY sold_qty DESC LIMIT 5;

DROP VIEW v_product_sales;   -- 练习完删掉，保持 shop 库干净
```

用途：**权限隔离**（给账号只暴露视图不给基表）、**简化重复查询**、**兼容层**（表重构后用旧视图过渡）。
限制：视图是查询的别名，**不存数据**，每次都现算；SQLite 视图只读，PG 简单视图可自动更新（INSERT/UPDATE 透传到基表，条件是无聚合/无连接等）。

## 2. 物化视图（PostgreSQL）：把查询结果落盘

```sql
-- PostgreSQL 专用
CREATE MATERIALIZED VIEW mv_daily_revenue AS
SELECT created_at::DATE AS day, SUM(total_cents) AS revenue
FROM orders
WHERE status <> 'cancelled'
GROUP BY created_at::DATE;

CREATE UNIQUE INDEX ON mv_daily_revenue(day);   -- 支持增量刷新的必需条件

SELECT * FROM mv_daily_revenue ORDER BY day;

REFRESH MATERIALIZED VIEW CONCURRENTLY mv_daily_revenue;   -- 刷新（不锁读）
DROP MATERIALIZED VIEW mv_daily_revenue;
```

**普通视图 = 每次现做菜；物化视图 = 提前做好一桌，定期重做。** 报表大屏、榜单页秒开就靠它（或定时任务写汇总表，效果等价）。代价：数据有延迟，刷新有成本。

## 3. 序列（SEQUENCE）与 ID 生成

```sql
-- PG: 手动控制的计数器（IDENTITY 列背后就是它）
CREATE SEQUENCE order_no_seq START 1000;
SELECT nextval('order_no_seq');    -- 1001
SELECT currval('order_no_seq');    -- 1001（本次会话最近一次）

-- 用途1: 业务单号
INSERT INTO orders (user_id, order_no) VALUES (1, nextval('order_no_seq'));

-- 用途2: 迁移数据时手动推进表的自增序列
SELECT setval('orders_id_seq', (SELECT MAX(id) FROM orders));
```

分布式/前置生成 ID：序列满足不了多库不重号时，用 **UUIDv7**（时间有序，索引友好）在**应用层生成**（Rust `uuid` crate v7 feature），PG 侧 `id UUID PRIMARY KEY`。

## 4. 触发器（TRIGGER）：钩子在数据变化时

```sql
-- SQLite 示例：自动维护 updated_at
CREATE TABLE audit_demo (
    id INTEGER PRIMARY KEY,
    val TEXT,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TRIGGER trg_audit_updated
AFTER UPDATE ON audit_demo
FOR EACH ROW
BEGIN
    UPDATE audit_demo SET updated_at = datetime('now') WHERE id = NEW.id;
END;

UPDATE audit_demo SET val = 'x' WHERE id = 1;
SELECT updated_at FROM audit_demo WHERE id = 1;   -- 已被触发器刷新

DROP TRIGGER trg_audit_updated;
DROP TABLE audit_demo;
```

PG 触发器常用 PL/pgSQL 写**审计日志**：`AFTER INSERT OR UPDATE OR DELETE` 把 `OLD`/`NEW` 行写入 `audit_log` 表。

**慎用触发器**——隐式逻辑是维护杀手：

- ✅ 合适：审计日志、updated_at 维护、简单派生列；
- ❌ 不合适：业务规则（跨表状态机）、发通知、调外部服务——这些放**应用层事务**里，看得见摸得着；
- 铁律：触发器里别再触发触发器（级联雪崩），别做慢操作（拖慢所有写入）。

## 5. JSON 列：半结构化数据入库

场景：商品属性（键盘=轴体/配列、椅子=腰托材质）字段差异大，为每种商品建列不现实。

```sql
-- PG: JSONB（二进制存储，可索引，推荐）；JSON（文本，保序不推荐）
ALTER TABLE products ADD COLUMN attrs JSONB;

UPDATE products SET attrs = '{"switch":"topre","layout":"60%","bluetooth":true}'
WHERE id = 1;

-- 查询与操作符
SELECT name, attrs->>'switch' AS switch FROM products WHERE id = 1;   -- ->> 取文本
SELECT name FROM products WHERE attrs @> '{"bluetooth": true}';       -- 包含匹配（可索引！）

-- GIN 索引让 JSON 查询飞起来
CREATE INDEX idx_products_attrs ON products USING GIN (attrs);
SELECT name FROM products WHERE attrs @> '{"layout": "60%"}';

-- SQLite: 用 JSON1 扩展（内置）
UPDATE products SET attrs = json('{"switch":"topre"}') WHERE id = 1;
SELECT json_extract(attrs, '$.switch') FROM products WHERE id = 1;
```

**JSONB vs 拆表的决策树**：

```
字段会参与 WHERE/JOIN/聚合统计？      → 拆成列（可索引可约束）
字段结构因记录而异且只整存整取？      → JSONB
报表要按 JSON 内字段分组？            → 拆列或生成列（PG: GENERATED ALWAYS AS (attrs->>'x') STORED）
```

## 6. 嵌套结果：json_agg 一次拉出树形（后端杀手锏）

避免 N+1 的终极形态——让数据库直接吐嵌套 JSON：

```sql
-- PG: 每个订单 + 其明细数组，一次查询
SELECT
    o.id,
    u.username,
    o.total_cents,
    json_agg(json_build_object(
        'product', p.name,
        'qty', oi.quantity,
        'price', oi.unit_price_cents
    )) AS items
FROM orders o
JOIN users u ON u.id = o.user_id
JOIN order_items oi ON oi.order_id = o.id
JOIN products p ON p.id = oi.product_id
GROUP BY o.id, u.username, o.total_cents
ORDER BY o.id
LIMIT 3;

-- SQLite 同样支持: json_group_array(json_object('product', p.name, 'qty', oi.quantity))
```

Rust 侧该列直接映射 `serde_json::Value` 或 `Vec<OrderItem>`——**一条 SQL 出整个聚合，比"N+1 + HashMap 组装"又快又简单**。

## 7. 全文搜索

`LIKE '%关键字%'` 的两个死穴：无法用 B-tree 索引、无相关性排序。正规全文搜索：

```sql
-- SQLite FTS5
CREATE VIRTUAL TABLE products_fts USING fts5(name, content=products, content_rowid=id);
INSERT INTO products_fts(rowid, name)
  SELECT id, name FROM products;                          -- 建立索引

SELECT p.name FROM products_fts f
JOIN products p ON p.id = f.rowid
WHERE products_fts MATCH '键盘 OR 键';                     -- 分词匹配
ORDER BY rank;                                             -- 相关性排序
DROP TABLE products_fts;

-- PostgreSQL: tsvector + GIN（中文需 zhparser/pg_jieba 扩展，英文内置）
--  CREATE INDEX idx_pname_fts ON products USING GIN (to_tsvector('simple', name));
--  SELECT * FROM products WHERE to_tsvector('simple', name) @@ to_tsquery('simple', 'keyboard');
```

搜索量大/需求复杂（拼音、纠错、分面）再上 Elasticsearch/Meilisearch，别把数据库逼成搜索引擎。

## 8. 何时用数据库特性 vs 应用层？（决策表）

| 需求 | 数据库侧 | 应用层（Rust）| 建议 |
|------|---------|--------------|------|
| 数据完整性 | 约束/外键/CHECK | 校验代码 | **数据库为主**（最后防线）|
| 复杂业务规则 | 触发器 | 事务代码 | **应用层**（可测试可读）|
| 权限隔离 | 视图/角色 | 接口层 | 数据库 + 接口双保险 |
| 报表加速 | 物化视图/汇总表 | 缓存 | 数据库（单一事实来源）|
| 搜索 | FTS | ES/Meili | 小用 FTS，大上专用引擎 |
| 定时任务 | pg_cron | tokio/cron | 应用层（部署简单）|

## 9. 常见坑

| 坑 | 说明 |
|---|---|
| 视图套视图套视图 | 三层以后没人看得懂，性能定位地狱；视图最多一层或两层 |
| 物化视图忘了刷新 | 数据永远停留在昨天；接定时任务并监控 |
| 触发器写业务 | 隐式行为 + 调试黑盒；审计可以，业务不行 |
| JSON 列当万能筐 | 什么都往里塞 → 无法建约束/索引，统计时哭死 |
| JSON 用 text 列手存 | 丢掉 JSONB 校验/索引/操作符；PG 一律 JSONB |
| FTS 表与源表不同步 | 增删改要用触发器/手动同步 FTS 索引表 |

## 10. Rust 视角

- JSONB ↔ `serde_json::Value` 开箱即用；要类型安全就定义 `struct ProductAttrs` + `sqlx::types::Json<T>`（自动序列化/反序列化，16 课示例）；
- `json_agg` 列 ↔ `sqlx::types::Json<Vec<OrderItemDto>>`，一行代码拿到嵌套聚合；
- 视图对 sqlx 完全透明：`query_as!` 把视图当表用，类型按视图列推断；
- 物化视图刷新用应用层定时任务（tokio + cron crate）触发 `REFRESH ... CONCURRENTLY`，比 pg_cron 部署链路简单。

---

## 练习

1. 建视图 `v_user_stats`（username、订单数、总消费、评价数，含 0），验证没下过单的用户也在。用完 DROP。
2. 给 products 加 `sales_cnt` 列，写一个 SQLite 触发器：order_items INSERT 时自动累加对应商品。（然后想想为什么生产环境更推荐下单事务里显式 UPDATE 而不是触发器）
3. PG 练习（如有 PG 环境）：给 products 加 `attrs JSONB`，写入两个属性，`@>` 查询蓝牙键盘，建 GIN 索引并 EXPLAIN 验证走索引。没 PG 则用 SQLite `json_extract` 完成查询部分。
4. 用 `json_group_array`（SQLite）一条 SQL 返回每个分类下的商品名数组（category, products_json）。
5. 思考题：为什么 `attrs->>'switch' = 'topre'`（对 JSON 表达式比较）通常比 `attrs @> '{"switch":"topre"}'` 慢？（提示：哪个能用 GIN 索引？表达式比较需要每行计算）

<details><summary>答案</summary>

```sql
-- 1.
CREATE VIEW v_user_stats AS
SELECT u.username,
       (SELECT COUNT(*) FROM orders o   WHERE o.user_id = u.id)  AS order_cnt,
       (SELECT COUNT(*) FROM reviews r  WHERE r.user_id = u.id)  AS review_cnt
FROM users u;
SELECT * FROM v_user_stats ORDER BY order_cnt DESC;
DROP VIEW v_user_stats;

-- 2.
ALTER TABLE products ADD COLUMN sales_cnt INTEGER NOT NULL DEFAULT 0;
CREATE TRIGGER trg_sales
AFTER INSERT ON order_items
FOR EACH ROW
BEGIN
    UPDATE products SET sales_cnt = sales_cnt + NEW.quantity WHERE id = NEW.product_id;
END;
INSERT INTO order_items (order_id, product_id, quantity, unit_price_cents)
VALUES (1, 2, 1, 89900);
SELECT name, sales_cnt FROM products WHERE id = 2;   -- 1
DROP TRIGGER trg_sales;
-- 生产推荐显式 UPDATE：逻辑在下单事务里可读可测；触发器隐式执行，
-- 取消订单时还要再写反向触发器，容易漏、难排查。
ALTER TABLE products DROP COLUMN sales_cnt;   -- SQLite 3.35+ 支持

-- 3. (PG)
-- ALTER TABLE products ADD COLUMN attrs JSONB;
-- UPDATE products SET attrs='{"switch":"topre","bluetooth":true}' WHERE id=1;
-- UPDATE products SET attrs='{"switch":"red","bluetooth":true}'  WHERE id=4;
-- CREATE INDEX idx_products_attrs ON products USING GIN(attrs);
-- EXPLAIN SELECT name FROM products WHERE attrs @> '{"bluetooth":true}';
-- → Bitmap Index Scan on idx_products_attrs

-- 4.
SELECT c.name AS category,
       json_group_array(p.name) AS products_json
FROM categories c
JOIN products p ON p.category_id = c.id
GROUP BY c.id, c.name;

-- 5. @> 是"包含"操作符，GIN 索引直接支持（索引里就能判定）；
--    ->>'x'='v' 要先对每行计算 JSON 提取（除非建表达式索引 (attrs->>'switch')），
--    无法利用通用 GIN 索引 → 逐行计算再比较 → 慢。
```
</details>

---

← 上一课：[14 · 数据建模与范式](14-数据建模与范式.md) | 下一课：[16 · Rust sqlx 入门](16-Rust-sqlx入门.md) · [返回目录](../README.md)
