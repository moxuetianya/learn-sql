# 10 · DDL 建表与约束

> 本课目标：能独立设计并创建带完整约束的表，掌握 ALTER/DROP 与迁移的工程实践。建表质量决定后端代码的下限——约束越完整，Rust 侧防御代码越少。

---

## 1. CREATE TABLE 完整解剖

以 shop 库真实表为例（SQLite 版）：

```sql
CREATE TABLE orders (
    -- 列定义:  列名  类型  [约束...]
    id          INTEGER PRIMARY KEY AUTOINCREMENT,        -- 主键 + 自增
    user_id     INTEGER NOT NULL REFERENCES users(id),    -- 非空 + 外键
    status      TEXT NOT NULL DEFAULT 'pending'           -- 非空 + 默认值
                CHECK (status IN ('pending','paid','shipped','completed','cancelled')),
    total_cents INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))   -- 默认值可以用函数
);
```

约束一览：

| 约束 | 作用 | 违反时 |
|------|------|--------|
| `NOT NULL` | 禁止 NULL | 报错（而不是静默存 NULL）|
| `PRIMARY KEY` | 唯一 + 非空 | 报错 |
| `UNIQUE` | 唯一 | 报错（PG 允许多个 NULL）|
| `FOREIGN KEY` | 引用完整性 | 报错或按 ON DELETE 策略处理 |
| `CHECK` | 自定义断言 | 报错 |
| `DEFAULT` | 缺省值 | 静默填充 |

**核心思想：把业务不变量（invariant）下沉到数据库**。"rating 必须 1~5"、"email 不重复"、"订单必须属于真实用户"——这些在 Rust 里校验一次还不够，并发和多人开发都会绕过。数据库是最后一道防线。

## 2. 主键与自增的三种方言

```sql
-- SQLite
id INTEGER PRIMARY KEY AUTOINCREMENT     -- AUTOINCREMENT 防止 rowid 复用（更严格但略慢）

-- PostgreSQL（现代写法，推荐）
id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY

-- PostgreSQL（传统写法，存量代码常见）
id BIGSERIAL PRIMARY KEY                 -- 本质是 int8 + sequence + default nextval()

-- MySQL
id BIGINT AUTO_INCREMENT PRIMARY KEY
```

> 主键用 BIGINT 还是 INT？估算行数：INT 上限 21 亿（21亿行用户表？也许吧），BIGINT 上限 9.2×10¹⁸。Rust 侧两者都映射 `i64` 最省心——**新表一律 BIGINT**，免得将来迁移。

## 3. 外键与级联策略

```sql
CREATE TABLE order_items (
    order_id   INTEGER NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
    product_id INTEGER NOT NULL REFERENCES products(id),
    ...
);
```

`ON DELETE` / `ON UPDATE` 策略：

| 策略 | 行为 | 适用 |
|------|------|------|
| `NO ACTION`（默认） | 有子行就拒绝删父行 | 大多数场景（安全）|
| `RESTRICT` | 同上（检查时机略不同）| 同上 |
| `CASCADE` | 级联删除子行 | 强从属数据：订单明细随订单消失 |
| `SET NULL` | 子行外键置 NULL（列须可空）| "孤儿保留"：删用户保留其评论，author 置空 |
| `SET DEFAULT` | 置为默认值 | 少用 |

**级联是一把双刃刀**：`CASCADE` 链长了，一条 DELETE 悄悄删掉几百行（还绕过应用层缓存失效逻辑）。团队规范建议：

- 强从属（明细、快照）：CASCADE；
- 业务主体数据：NO ACTION，由**应用层事务**显式处理（12 课）；
- 绝不用触发器做隐式级联（15 课触发器节再谈）。

## 4. CHECK 约束：数据库层的类型化

```sql
CREATE TABLE reviews (
    rating SMALLINT NOT NULL CHECK (rating BETWEEN 1 AND 5),
    ...
);

-- 命名约束（推荐：报错信息里能看到名字）
CONSTRAINT chk_rating CHECK (rating BETWEEN 1 AND 5)

-- PG 15+ 支持 UNIQUE 约束里带条件
CREATE UNIQUE INDEX one_pending_order_per_user
    ON orders(user_id) WHERE status = 'pending';   -- 部分索引：每人最多一单待支付
```

CHECK 里可以引用同行多列：`CHECK (discount_cents <= price_cents)`。跨行/跨表约束数据库不管——那是应用层事务的活（12 课）。

## 5. ALTER TABLE：改表的生存指南

```sql
-- 加列（最安全，所有库都支持）
ALTER TABLE products ADD COLUMN description TEXT;
ALTER TABLE products ADD COLUMN weight_grams INTEGER NOT NULL DEFAULT 0;   -- 必须有默认值（老行填什么）

-- PG: 加约束
ALTER TABLE products ADD CONSTRAINT chk_stock CHECK (stock >= 0);

-- PG: 改列类型
ALTER TABLE products ALTER COLUMN name TYPE TEXT;
ALTER TABLE products ALTER COLUMN price_cents TYPE BIGINT;

-- 改列名
ALTER TABLE products RENAME COLUMN description TO descr;

-- SQLite 只支持 加列/改名/改表名，改类型要走"影子表"流程：
-- 1 CREATE new 2 拷数据 3 DROP old 4 RENAME —— 这正是现代迁移工具（含 sqlx migrate）的做法
```

> ⚠️ **大表改列的锁风险**：PG 的 `ALTER COLUMN TYPE` 在 12+ 前多数情况要重写全表并持锁；加列带默认值在 PG 11+ 是元数据操作（瞬间完成）。**改表前先估行数、看版本、在预发演练**——线上 DDL 是后端大事故高发区。

## 6. DROP / TRUNCATE

```sql
DROP TABLE IF EXISTS tmp_users;         -- 连表带数据带索引一起没（DDL，不可回滚）
TRUNCATE TABLE tmp_users;               -- 清空数据保留结构（快，绕过逐行删除；PG 不触发行级触发器）
DELETE FROM tmp_users;                  -- DML，逐行删，可回滚，触发触发器（11 课）
```

## 7. 工程实践：迁移（Migration）工作流

**生产环境禁止手敲 DDL**，一律走版本化迁移文件（16 课 sqlx-cli 实操）：

```
migrations/
├── 0001_create_shop.sql       -- up: 建表
├── 0002_add_product_desc.sql  -- up: 加列
└── 0003_...sql
```

铁律：

1. 迁移**只增不改**：已合入 main 的迁移文件不许修改，写新迁移纠正；
2. **可重放**：`migrate up` 幂等（工具用 `_sqlx_migrations` 表记录已执行版本）；
3. **可回滚**：配套 down 脚本（sqlx 社区惯例是不写 down，靠备份/前滚——团队约定好即可）；
4. 每次迁移后跑一遍种子数据脚本，保证 CI 环境可复现。

## 8. 实战：从需求到建表

需求：优惠券表——每人可领多张，每张有唯一码、面值（分）、有效期、状态（未用/已用/过期），领取时记录来源。

```sql
CREATE TABLE coupons (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    code         TEXT NOT NULL UNIQUE,                    -- 唯一码
    user_id      INTEGER NOT NULL REFERENCES users(id),   -- 领取人
    amount_cents INTEGER NOT NULL CHECK (amount_cents > 0),
    status       TEXT NOT NULL DEFAULT 'unused'
                 CHECK (status IN ('unused','used','expired')),
    source       TEXT NOT NULL DEFAULT 'campaign',
    expires_at   TEXT NOT NULL,
    created_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_coupons_user_status ON coupons(user_id, status);   -- 复合索引（13 课）
```

设计 checklist（每张新表过一遍）：

- [ ] 主键（代理键 BIGINT/INTEGER）？
- [ ] 外键 + 明确的 ON DELETE 策略？
- [ ] 枚举列用 CHECK 白名单？
- [ ] 金额/数量有正负性约束？
- [ ] NOT NULL 想清楚每一列（默认加上，真需要可空再放开）？
- [ ] created_at/updated_at 审计列？（updated_at 需触发器或应用层维护）
- [ ] 查询热路径的索引建了？（下一课）

## 9. 常见坑

| 坑 | 说明 |
|---|---|
| SQLite 外键不生效 | 忘了 `PRAGMA foreign_keys=ON`（00 课已配置）|
| MySQL 5.x 忽略 CHECK | 8.0.16 才真正执行；用 ENUM 或触发器过渡 |
| 加 NOT NULL 列没默认值 | 老表有数据时失败；先加可空列→回填→改非空 |
| 级联链设太长 | 删一行引发雪崩；审慎使用 CASCADE |
| 手改生产表 | 无记录、不可回滚、多环境漂移；一律走迁移工具 |
| 表名/列名用关键字 | `order` `user` `group` 是保留字，起名 `orders` `users` 或加引号 |

## 10. Rust 视角

- 迁移工具链：`sqlx-cli`（`cargo install sqlx-cli`）`sqlx migrate add/run`，Rust 内 `sqlx::migrate!()` 宏嵌入二进制启动时自动迁移（16 课实战）；
- 类型映射基线：`BIGINT/INTEGER → i64`、`TEXT → String`、`TIMESTAMPTZ → DateTime<Utc>`、`NUMERIC → rust_decimal::Decimal`（别用 f64！）；
- CHECK/UNIQUE 冲突在 sqlx 里表现为 `sqlx::Error::Database`，错误码区分：PG `23505=unique_violation`、`23503=foreign_key_violation`、`23514=check_violation`（17 课统一处理）。

---

## 练习

1. 创建上面的 coupons 表，插入三张优惠券（给 alice 一张满 5000 分、bob 两张），分别测试：重复 code、amount=-1、非法 status、不存在的 user_id——各触发什么约束？
2. 给 coupons 表补一列 `used_at`（可空 TEXT），再补 `CHECK (status <> 'used' OR used_at IS NOT NULL)`（用了的券必须有使用时间）。SQLite 支持向已有表 ADD CONSTRAINT 吗？怎么办？
3. 写出 PG 版 coupons 建表（IDENTITY 主键 + TIMESTAMPTZ）。
4. 思考：订单表删除用户时订单怎么办？为 users→orders 设计外键策略并说明理由。

<details><summary>答案</summary>

```sql
-- 1.
INSERT INTO coupons (code, user_id, amount_cents, expires_at) VALUES
  ('A001', 1, 5000, '2026-12-31 23:59:59');
-- 重复 code → UNIQUE constraint failed
INSERT INTO coupons (code, user_id, amount_cents, expires_at) VALUES
  ('A001', 2, 3000, '2026-12-31 23:59:59');
-- amount=-1 → CHECK constraint failed
INSERT INTO coupons (code, user_id, amount_cents, expires_at) VALUES
  ('A002', 2, -1, '2026-12-31 23:59:59');
-- 非法 status → CHECK constraint failed
INSERT INTO coupons (code, user_id, amount_cents, status, expires_at) VALUES
  ('A003', 2, 3000, 'stolen', '2026-12-31 23:59:59');
-- user_id=999 → FOREIGN KEY constraint failed
INSERT INTO coupons (code, user_id, amount_cents, expires_at) VALUES
  ('A004', 999, 3000, '2026-12-31 23:59:59');

-- 2. SQLite 不支持 ADD CONSTRAINT。流程：
ALTER TABLE coupons ADD COLUMN used_at TEXT;         -- ① 加列
-- ② SQLite 需要重建表才能加约束（或等迁移工具/新版本支持 DROP CONSTRAINT 再处理）；
--    PG 直接: ALTER TABLE coupons ADD CONSTRAINT chk_used
--             CHECK (status <> 'used' OR used_at IS NOT NULL);

-- 3. (PG)
CREATE TABLE coupons (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    code         TEXT NOT NULL UNIQUE,
    user_id      BIGINT NOT NULL REFERENCES users(id),
    amount_cents BIGINT NOT NULL CHECK (amount_cents > 0),
    status       TEXT NOT NULL DEFAULT 'unused'
                 CHECK (status IN ('unused','used','expired')),
    source       TEXT NOT NULL DEFAULT 'campaign',
    expires_at   TIMESTAMPTZ NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_coupons_user_status ON coupons(user_id, status);

-- 4. 建议 NO ACTION（默认）：订单含金额、审计、售后信息，删用户必须先人工处理其订单
--    （或采用软删除 users.deleted_at）。绝不能 CASCADE——删个用户把两年交易记录带走了。
DROP TABLE coupons;   -- 练习完清理，或 ./setup/init.sh 重置
```
</details>

---

← 上一课：[09 · 窗口函数](09-窗口函数.md) | 下一课：[11 · DML 增删改](11-DML增删改.md) · [返回目录](../README.md)
