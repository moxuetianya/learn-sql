# 11 · DML 增删改

> 本课目标：熟练 INSERT/UPDATE/DELETE，掌握 UPSERT（ON CONFLICT）与 RETURNING——这两个特性直接决定 Rust 后端代码的形态。写操作 = 数据在变，练习后记得 `./setup/init.sh` 重置。

---

## 1. INSERT：单行、多行、指定列

```sql
-- 完整列序插入（依赖表定义列序，不推荐：加列就崩）
INSERT INTO categories VALUES (7, '显示器灯');

-- 指定列插入（推荐：结构变了也稳）
INSERT INTO categories (name) VALUES ('摄像头');

-- 多行批量（生产标配，一条语句比循环 N 条快一个数量级）
INSERT INTO categories (name) VALUES
  ('麦克风'),
  ('音箱'),
  ('扩展坞');

-- 插入查询结果（INSERT INTO ... SELECT：数据搬运/归档）
INSERT INTO categories (name)
SELECT '外设-' || name FROM categories WHERE id <= 3;   -- 示例：造几条派生数据
```

> 后端铁律：**批量插入永远用多值 INSERT / COPY（PG），不要循环单条**。1 万行：循环约 10s+（每条一个网络往返+事务），多值 INSERT 约 100ms，PG `COPY` 约 30ms。

## 2. UPDATE：必须带 WHERE（讲三遍）

```sql
-- 全表改（不带 WHERE = 全部行！生产事故第一名）
UPDATE products SET stock = stock + 10;

-- 条件更新
UPDATE products SET stock = stock - 1 WHERE id = 1 AND stock > 0;
-- ↑ 经典"防止超卖"：受影响行数 = 0 说明库存不足（Rust 侧看 rows_affected）

-- 多列
UPDATE orders SET status = 'shipped', updated_at = datetime('now')   -- shop 库暂无该列，示意
WHERE status = 'paid' AND id = 7;

-- 带子查询的更新（PG 支持 JOIN 版 UPDATE，SQLite 用子查询）
UPDATE products
SET price_cents = price_cents * 0.9
WHERE category_id = (SELECT id FROM categories WHERE name = '书籍');
```

**执行 UPDATE 前，先把 WHERE 原样放进 SELECT 数一遍行数**——团队血泪规范：

```sql
SELECT COUNT(*) FROM orders WHERE status = 'paid';   -- 先看影响范围
UPDATE orders SET status = 'shipped' WHERE status = 'paid';  -- 再动手
```

## 3. DELETE 与软删除

```sql
DELETE FROM categories WHERE name LIKE '外设-%';   -- 删刚才造的测试数据
DELETE FROM order_items WHERE order_id = 16;       -- 删明细

-- 无 WHERE 的 DELETE = 清空全表（等价 TRUNCATE 但慢、可回滚、触发器照常跑）
```

**生产实践：重要数据用软删除**（加 `deleted_at` 列，查询时 `WHERE deleted_at IS NULL`）。原因：

1. 误删可恢复、审计可追溯；
2. 用户"注销"常有法定数据保留期（GDPR/电商法）；
3. 外键引用不断裂。

硬删除只用于：临时表、日志归档、用户行使"被遗忘权"且法务放行。

## 4. UPSERT：存在则更新，不存在则插入

后端最高频写操作之一（缓存回写、计数器、幂等写入）：

```sql
-- SQLite 语法（PG 完全兼容这个形式）
INSERT INTO products (category_id, name, price_cents, stock)
VALUES (1, 'Keychron Q1 Pro', 129900, 25)
ON CONFLICT (name) DO UPDATE            -- 冲突键：name（需有 UNIQUE 约束）
SET price_cents = excluded.price_cents, -- excluded = 本次试图插入的那行
    stock = excluded.stock;

-- 冲突时什么都不做（幂等插入：重复点击"领券"不报错不重复发）
INSERT INTO coupons (code, user_id, amount_cents, expires_at)
VALUES ('WELCOME10', 1, 1000, '2026-12-31')
ON CONFLICT (code) DO NOTHING;

-- 计数器模式（upsert + 自增）
INSERT INTO product_views (product_id, cnt) VALUES (1, 1)
ON CONFLICT (product_id) DO UPDATE SET cnt = product_views.cnt + 1;

-- MySQL 方言:
--   INSERT ... ON DUPLICATE KEY UPDATE cnt = cnt + 1
```

> `excluded` 是 PG/SQLite 标准的"拟插入行"引用；`ON CONFLICT` 的目标列**必须有唯一约束/索引**，否则报错。

## 5. RETURNING：写完立刻拿回结果

DML 语句传统上只返回"影响行数"；自增 id 要再查一次（或用 `last_insert_rowid()`）。`RETURNING` 让 INSERT/UPDATE/DELETE 直接返回行——**Rust 后端写代码的形态由此改变**（SQLite ≥3.35 / PG ≥9.1，MySQL/MariaDB 新版才跟进）：

```sql
-- 插入并拿回自增 id + 默认值填充后的完整行
INSERT INTO users (username, email) VALUES ('irene', 'irene@example.com')
RETURNING id, created_at;

-- 更新并返回新值（不用再 SELECT 一次）
UPDATE products SET stock = stock - 1 WHERE id = 1 AND stock > 0
RETURNING id, stock;                      -- 库存不足时返回 0 行

-- 删除前留档
DELETE FROM categories WHERE name = '摄像头'
RETURNING *;                              -- 被删的行完整返回（写审计日志）
```

## 6. 幂等性：后端接口的防重复提交

网络重试、用户双击、MQ 至少一次投递……同一个写请求会来不止一次。SQL 层的幂等武器：

```sql
-- 方案1：唯一约束 + ON CONFLICT DO NOTHING（天然幂等键：业务单号）
INSERT INTO payments (order_id, amount_cents, idempotency_key)
VALUES (7, 129900, 'req_abc123')
ON CONFLICT (idempotency_key) DO NOTHING
RETURNING id;
-- 返回 0 行 = 重复请求 → 查已有记录返回相同结果，副作用不重复发生

-- 方案2：条件更新当状态机（乐观锁，12 课细讲）
UPDATE orders SET status = 'paid' WHERE id = 7 AND status = 'pending';
-- 重复调用第二次 affected_rows = 0，接口可据此返回"已处理"
```

## 7. 常见坑

| 坑 | 说明 |
|---|---|
| UPDATE/DELETE 忘 WHERE | 全表遭殃。改前 SELECT COUNT(*)，生产开事务先看再提交 |
| 循环单条 INSERT | 慢 100 倍；用多值插入/COPY/`QueryBuilder::push_values` |
| INSERT 依赖隐式列序 | 加列即崩；永远写明列清单 |
| last_insert_rowid 拿 id 有竞态 | 连接复用/并发下不可靠；用 RETURNING |
| ON CONFLICT 键无唯一约束 | 语法报错；先建唯一索引 |
| DELETE 硬删有外键引用的行 | 先删子行或级联/软删（10 课策略） |
| 把业务规则写在多条 DML 却不包事务 | 中间失败留下脏数据 → 12 课 |

## 8. Rust 视角（sqlx 预告）

```rust
// INSERT + RETURNING 一步到位
let (id, created_at): (i64, String) = sqlx::query_as(
    "INSERT INTO users (username, email) VALUES (?1, ?2) RETURNING id, created_at"
)
.bind(&req.username).bind(&req.email)
.fetch_one(&pool).await?;                      // 拿不回行就是错误

// 受影响行数（防超卖/状态机）
let res = sqlx::query("UPDATE products SET stock = stock - ?1 WHERE id = ?2 AND stock >= ?1")
    .bind(qty).bind(pid)
    .execute(&pool).await?;
if res.rows_affected() == 0 {
    return Err(AppError::OutOfStock);          // 0 行 = 库存不足
}

// 批量插入
let mut qb = sqlx::QueryBuilder::new("INSERT INTO order_items (order_id, product_id, quantity, unit_price_cents) ");
qb.push_values(items, |mut b, item| {
    b.push_bind(order_id).push_bind(item.pid).push_bind(item.qty).push_bind(item.price);
});
qb.build().execute(&pool).await?;
```

注意 PG 占位符是 `$1`，SQLite 兼容 `?` 与 `$1` 两种——写跨方言 SQL 用 `$1` 风格最统一。

---

## 练习

（以下写操作会改库，做完 `./setup/init.sh` 重置）

1. 批量插入 3 个新分类（'麦克风','音箱','扩展坞'），一条 SQL。
2. 给 id=11 的商品（Razer Viper V2 Pro）打 9 折，并**同一条语句**返回折后价格。
3. 模拟下单：给 alice（id=1）插一笔订单（status=paid），再用 `INSERT INTO...SELECT` 从购物车式数据构造——把 id=17、18 两本书各 1 本写进 order_items（单价取当前商品价），并让订单 total_cents 等于明细合计（用 UPDATE + 子查询算总额）。
4. 幂等练习：建一张 `coupon_claims(user_id, coupon_code, claimed_at, PRIMARY KEY(user_id, coupon_code))`，同一用户同一券重复领取用 ON CONFLICT DO NOTHING 挡住；测试插两次。
5. 删除上题的表和第 1 题的分类，全部用 RETURNING * 观察被删内容。

<details><summary>答案</summary>

```sql
-- 1.
INSERT INTO categories (name) VALUES ('麦克风'), ('音箱'), ('扩展坞');

-- 2.
UPDATE products SET price_cents = price_cents * 0.9
WHERE id = 11
RETURNING name, price_cents;

-- 3.
INSERT INTO orders (user_id, status) VALUES (1, 'paid')
RETURNING id;                        -- 假设返回 21
INSERT INTO order_items (order_id, product_id, quantity, unit_price_cents)
SELECT 21, id, 1, price_cents FROM products WHERE id IN (17, 18);
UPDATE orders
SET total_cents = (SELECT SUM(quantity * unit_price_cents)
                   FROM order_items WHERE order_id = 21)
WHERE id = 21
RETURNING total_cents;               -- 11900 + 6900 = 18800

-- 4.
CREATE TABLE coupon_claims (
    user_id     INTEGER NOT NULL REFERENCES users(id),
    coupon_code TEXT NOT NULL,
    claimed_at  TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (user_id, coupon_code)
);
INSERT INTO coupon_claims (user_id, coupon_code) VALUES (1, 'WELCOME10');
INSERT INTO coupon_claims (user_id, coupon_code) VALUES (1, 'WELCOME10')
ON CONFLICT (user_id, coupon_code) DO NOTHING;    -- 第二次静默跳过

-- 5.
DELETE FROM coupon_claims RETURNING *;
DELETE FROM categories WHERE name IN ('麦克风','音箱','扩展坞') RETURNING *;
```
</details>

---

← 上一课：[10 · DDL 建表与约束](10-DDL建表与约束.md) | 下一课：[12 · 事务与并发](12-事务与并发.md) · [返回目录](../README.md)
