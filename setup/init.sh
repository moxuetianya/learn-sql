#!/usr/bin/env bash
# 一键初始化 SQLite 练习数据库（生成 ./shop.db）
set -euo pipefail
cd "$(dirname "$0")/.."

rm -f shop.db
sqlite3 shop.db < setup/schema.sql
sqlite3 shop.db < setup/seed.sql

echo "== 初始化完成 =="
sqlite3 shop.db "SELECT COUNT(*) || ' 个分类' FROM categories;
SELECT COUNT(*) || ' 个用户'   FROM users;
SELECT COUNT(*) || ' 个商品'   FROM products;
SELECT COUNT(*) || ' 个订单'   FROM orders;
SELECT COUNT(*) || ' 条明细'   FROM order_items;
SELECT COUNT(*) || ' 条评价'   FROM reviews;"
echo
echo "进入练习环境: sqlite3 shop.db"
echo "提示: 建议在 ~/.sqliterc 里加一行 PRAGMA foreign_keys=ON;"
