-- ============================================================
-- shop 示例数据库 · SQLite 版
-- 用法: sqlite3 shop.db < setup/schema.sql
-- 注意: SQLite 默认不强制外键，每次连接后需执行 PRAGMA foreign_keys=ON
-- ============================================================
PRAGMA foreign_keys = ON;

-- 分类表
DROP TABLE IF EXISTS reviews;
DROP TABLE IF EXISTS order_items;
DROP TABLE IF EXISTS orders;
DROP TABLE IF EXISTS products;
DROP TABLE IF EXISTS users;
DROP TABLE IF EXISTS categories;

CREATE TABLE categories (
    id   INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE
);

-- 用户表
CREATE TABLE users (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    username   TEXT NOT NULL UNIQUE,
    email      TEXT NOT NULL UNIQUE,
    role       TEXT NOT NULL DEFAULT 'customer'
               CHECK (role IN ('customer', 'admin')),
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 商品表
CREATE TABLE products (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    category_id  INTEGER NOT NULL REFERENCES categories(id),
    name         TEXT NOT NULL UNIQUE,
    price_cents  INTEGER NOT NULL CHECK (price_cents >= 0),
    stock        INTEGER NOT NULL DEFAULT 0 CHECK (stock >= 0),
    created_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 订单表
CREATE TABLE orders (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id     INTEGER NOT NULL REFERENCES users(id),
    status      TEXT NOT NULL DEFAULT 'pending'
                CHECK (status IN ('pending', 'paid', 'shipped', 'completed', 'cancelled')),
    total_cents INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 订单明细表（联合主键：一个订单中同一商品只占一行）
CREATE TABLE order_items (
    order_id          INTEGER NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
    product_id        INTEGER NOT NULL REFERENCES products(id),
    quantity          INTEGER NOT NULL CHECK (quantity > 0),
    unit_price_cents  INTEGER NOT NULL CHECK (unit_price_cents >= 0),
    PRIMARY KEY (order_id, product_id)
);

-- 商品评价表
CREATE TABLE reviews (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id    INTEGER NOT NULL REFERENCES users(id),
    product_id INTEGER NOT NULL REFERENCES products(id),
    rating     INTEGER NOT NULL CHECK (rating BETWEEN 1 AND 5),
    comment    TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (user_id, product_id)          -- 每人每商品只能评一次
);

-- 索引（13 课详细讲解）
CREATE INDEX idx_products_category ON products(category_id);
CREATE INDEX idx_orders_user       ON orders(user_id);
CREATE INDEX idx_orders_status     ON orders(status);
CREATE INDEX idx_order_items_product ON order_items(product_id);
CREATE INDEX idx_reviews_product   ON reviews(product_id);
-- ============================================================
-- shop 示例数据库 · SQLite 种子数据
-- 用法: sqlite3 shop.db < setup/seed.sql
-- ============================================================
PRAGMA foreign_keys = ON;

INSERT INTO categories (id, name) VALUES
  (1, '键盘'),
  (2, '显示器'),
  (3, '鼠标'),
  (4, '耳机'),
  (5, '书籍'),
  (6, '椅子');

INSERT INTO users (id, username, email, role, created_at) VALUES
  (1, 'alice',   'alice@example.com',   'customer', '2025-01-05 09:00:00'),
  (2, 'bob',     'bob@example.com',     'customer', '2025-01-12 10:30:00'),
  (3, 'charlie', 'charlie@example.com', 'customer', '2025-02-03 14:20:00'),
  (4, 'diana',   'diana@example.com',   'admin',    '2025-02-10 08:00:00'),
  (5, 'eve',     'eve@example.com',     'customer', '2025-03-01 19:45:00'),
  (6, 'frank',   'frank@example.com',   'customer', '2025-03-15 11:10:00'),
  (7, 'grace',   'grace@example.com',   'customer', '2025-04-02 16:30:00'),
  (8, 'henry',   'henry@example.com',   'customer', '2025-04-20 12:00:00');

INSERT INTO products (id, category_id, name, price_cents, stock, created_at) VALUES
  ( 1, 1, 'HHKB Professional 2',      189900, 12, '2025-01-15 08:00:00'),
  ( 2, 1, 'Leopold FC750R',           89900,  30, '2025-01-15 08:05:00'),
  ( 3, 1, 'Keychron Q1 Pro',         129900,  25, '2025-02-01 09:00:00'),
  ( 4, 1, 'Nuphy Air75',              99900,  40, '2025-02-05 10:00:00'),
  ( 5, 2, 'Dell U2723QE',            459900,   8, '2025-01-20 09:00:00'),
  ( 6, 2, 'LG 27GP850',              329900,  15, '2025-01-22 09:30:00'),
  ( 7, 2, 'AOC 24B2XH',               79900,  50, '2025-01-25 10:00:00'),
  ( 8, 2, 'BenQ PD2705U',            399900,  10, '2025-02-10 11:00:00'),
  ( 9, 3, 'Logitech MX Master 3S',    79900,  45, '2025-01-18 08:30:00'),
  (10, 3, 'Logitech G Pro X',         59900,  60, '2025-01-18 08:35:00'),
  (11, 3, 'Razer Viper V2 Pro',      139900,  20, '2025-02-12 12:00:00'),
  (12, 3, 'Apple Magic Mouse',        74900,  35, '2025-02-15 13:00:00'),
  (13, 4, 'Sony WH-1000XM5',         289900,  18, '2025-01-28 09:15:00'),
  (14, 4, 'Bose QC45',               229900,  22, '2025-01-30 09:45:00'),
  (15, 4, 'Sennheiser HD 660S',      299900,   6, '2025-02-18 14:00:00'),
  (16, 4, 'AirPods Pro 2',           189900,  50, '2025-02-20 15:00:00'),
  (17, 5, '《Rust 程序设计语言》',      11900, 100, '2025-01-10 08:00:00'),
  (18, 5, '《SQL 必知必会》',            6900, 120, '2025-01-10 08:10:00'),
  (19, 5, '《数据结构与算法》',          9900,  80, '2025-01-11 09:00:00'),
  (20, 5, '《Designing Data-Intensive Applications》', 15900, 40, '2025-02-01 10:00:00'),
  (21, 6, 'Herman Miller Aeron',    1099900,   3, '2025-01-08 07:00:00'),
  (22, 6, 'Steelcase Leap V2',       699900,   5, '2025-01-09 07:30:00'),
  (23, 6, 'Ergonor 保友金豪 b',       329900,  10, '2025-01-14 08:00:00'),
  (24, 6, '西昊 M57',                 109900,  20, '2025-02-25 16:00:00');

INSERT INTO orders (id, user_id, status, total_cents, created_at) VALUES
  ( 1, 1, 'completed', 269800, '2025-02-01 10:00:00'),
  ( 2, 1, 'completed',  11900, '2025-03-05 15:30:00'),
  ( 3, 2, 'completed', 389800, '2025-02-14 20:00:00'),
  ( 4, 2, 'cancelled',  79900, '2025-03-01 09:00:00'),
  ( 5, 3, 'completed', 289900, '2025-02-20 12:00:00'),
  ( 6, 3, 'shipped',   159800, '2025-04-01 18:00:00'),
  ( 7, 1, 'paid',      129900, '2025-04-10 21:00:00'),
  ( 8, 4, 'completed', 1099900,'2025-03-12 14:00:00'),
  ( 9, 5, 'completed',  98800, '2025-03-18 11:00:00'),
  (10, 5, 'pending',    74900, '2025-04-15 10:00:00'),
  (11, 6, 'completed', 519800, '2025-03-22 17:00:00'),
  (12, 6, 'completed',  31900, '2025-04-02 13:00:00'),
  (13, 7, 'shipped',   229900, '2025-04-05 16:00:00'),
  (14, 7, 'paid',       15900, '2025-04-18 09:30:00'),
  (15, 8, 'completed', 369800, '2025-03-28 20:30:00'),
  (16, 8, 'cancelled', 299900, '2025-04-08 22:00:00'),
  (17, 2, 'completed', 459900, '2025-04-12 19:00:00'),
  (18, 3, 'completed',  16900, '2025-04-20 10:00:00'),
  (19, 5, 'completed', 329900, '2025-04-22 15:00:00'),
  (20, 7, 'completed',  59900, '2025-04-25 08:00:00');

INSERT INTO order_items (order_id, product_id, quantity, unit_price_cents) VALUES
  -- 订单1: alice 买了键盘+鼠标
  ( 1,  1, 1, 189900), ( 1,  9, 1,  79900),
  -- 订单2: alice 买书
  ( 2, 17, 1,  11900),
  -- 订单3: bob 买了键盘+显示器
  ( 3,  2, 1,  89900), ( 3,  6, 1, 329900) ,
  -- 订单4: bob 买鼠标（后取消）
  ( 4, 10, 1,  59900), ( 4, 12, 1,  74900) ,
  -- 订单5: charlie 买耳机
  ( 5, 13, 1, 289900),
  -- 订单6: charlie 买书 x2
  ( 6, 20, 10,  15900),
  -- 订单7: alice 买键盘
  ( 7,  3, 1, 129900),
  -- 订单8: diana(admin) 买人体工学椅
  ( 8, 21, 1, 1099900),
  -- 订单9: eve 买鼠标+书
  ( 9, 10, 1,  59900), ( 9, 18, 1,   6900) ,
  -- 订单10: eve 买苹果鼠标（待支付）
  (10, 12, 1,  74900),
  -- 订单11: frank 买显示器+键盘
  (11,  5, 1, 459900), (11,  4, 1,  99900) ,
  -- 订单12: frank 买书
  (12, 19, 2,   9900), (12, 18, 1,   6900) ,
  -- 订单13: grace 买耳机
  (13, 14, 1, 229900),
  -- 订单14: grace 买 DDIA
  (14, 20, 1,  15900),
  -- 订单15: henry 买显示器+键盘
  (15,  7, 1,  79900), (15,  2, 1,  89900) ,
  -- 订单16: henry 买耳机（后取消）
  (16, 15, 1, 299900),
  -- 订单17: bob 买显示器
  (17,  5, 1, 459900),
  -- 订单18: charlie 买书
  (18, 17, 1,  11900), (18, 18, 1,   6900) ,
  -- 订单19: eve 买人体工学椅
  (19, 23, 1, 329900),
  -- 订单20: grace 买鼠标
  (20, 10, 1,  59900);

INSERT INTO reviews (user_id, product_id, rating, comment, created_at) VALUES
  (1,  1, 5, '打字手感一流，静电容名不虚传', '2025-02-03 10:00:00'),
  (1,  9, 5, '人体工学满分', '2025-02-03 10:05:00'),
  (1, 17, 4, 'Rust 入门首选', '2025-03-06 09:00:00'),
  (2,  2, 4, 'PBT 键帽很舒服', '2025-02-15 21:00:00'),
  (2,  6, 5, '打游戏很爽，色彩好', '2025-02-15 21:10:00'),
  (2, 10, 3, '还行，无线偶尔延迟', '2025-03-02 10:00:00'),
  (2,  5, 5, '4K 办公神器', '2025-04-13 20:00:00'),
  (3, 13, 5, '降噪无敌', '2025-02-21 12:00:00'),
  (3, 20, 5, '后端工程师必读', '2025-04-02 18:30:00'),
  (3, 17, 5, '写得很清楚', '2025-04-21 10:00:00'),
  (4, 21, 5, '腰不酸了', '2025-03-13 15:00:00'),
  (5, 10, 4, '轻巧好用', '2025-03-19 12:00:00'),
  (5, 18, 3, '有点薄', '2025-03-19 12:05:00'),
  (5, 23, 4, '性价比高', '2025-04-23 16:00:00'),
  (6,  5, 5, 'Type-C 扩展方便', '2025-03-23 18:00:00'),
  (6,  4, 4, '矮轴适合携带', '2025-03-23 18:05:00'),
  (6, 19, 4, '例子丰富', '2025-04-03 14:00:00'),
  (7, 14, 4, '佩戴舒适', '2025-04-06 17:00:00'),
  (7, 20, 5, '神书', '2025-04-19 10:00:00'),
  (8,  7, 3, '性价比入门款', '2025-03-29 21:00:00'),
  (8,  2, 4, '中规中矩', '2025-03-29 21:05:00');
