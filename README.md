# SQL 完整教程 —— 面向 Rust 后端开发

> 20 课 · 从零到生产级：SQL 语言核心 → 事务与性能 → 数据建模 → Rust (sqlx / Diesel / SeaORM / Axum) 全栈实战。
> 所有课程围绕一个统一的**电商示例数据库（shop）**展开，学完即可在真实后端项目中落地。

---

## 学习路线

```
阶段一：SQL 语言基础          阶段二：多表与进阶查询
┌──────────────────────┐    ┌──────────────────────┐
│ 00 环境搭建与示例库    │    │ 06 JOIN 多表查询      │
│ 01 SQL 与关系模型入门  │ ─▶ │ 07 子查询与 CTE       │
│ 02 SELECT 基础        │    │ 08 集合运算           │
│ 03 WHERE 过滤         │    │ 09 窗口函数           │
│ 04 函数与 CASE        │    └──────────────────────┘
│ 05 聚合与 GROUP BY    │
└──────────────────────┘
           │
           ▼
阶段三：写操作·事务·性能      阶段四：建模与数据库特性     阶段五：Rust 后端实战
┌──────────────────────┐    ┌──────────────────────┐   ┌──────────────────────┐
│ 10 DDL 建表与约束     │    │ 14 数据建模与范式     │   │ 16 Rust sqlx 入门    │
│ 11 DML 增删改         │ ─▶ │ 15 视图/序列/触发器/  │ ─▶│ 17 Rust sqlx 进阶    │
│ 12 事务与并发         │    │    JSON/全文搜索      │   │ 18 Diesel ORM        │
│ 13 索引与性能优化     │    └──────────────────────┘   │ 19 SeaORM            │
└──────────────────────┘                                │ 20 综合项目：        │
                                                        │    Axum REST API    │
                                                        └──────────────────────┘
```

## 目录

| # | 课程 | 主题 | 预计时长 |
|---|------|------|---------|
| 00 | [环境搭建与示例数据库](lessons/00-环境搭建与示例数据库.md) | SQLite / PostgreSQL 双环境、shop 库初始化 | 0.5h |
| 01 | [SQL 与关系模型入门](lessons/01-SQL与关系模型入门.md) | 表/行/列、主键、数据类型、SQL 分类 | 1h |
| 02 | [SELECT 基础](lessons/02-SELECT基础.md) | 投影、别名、DISTINCT、ORDER BY、LIMIT | 1.5h |
| 03 | [WHERE 过滤](lessons/03-WHERE过滤.md) | 比较与逻辑运算、IN/BETWEEN/LIKE、NULL 三值逻辑 | 1.5h |
| 04 | [函数与 CASE](lessons/04-函数与CASE.md) | 字符串/数值/日期函数、类型转换、CASE 表达式 | 2h |
| 05 | [聚合与 GROUP BY](lessons/05-聚合与GROUP-BY.md) | COUNT/SUM/AVG、分组、HAVING、常见统计套路 | 2h |
| 06 | [JOIN 多表查询](lessons/06-JOIN多表查询.md) | INNER/LEFT/RIGHT/FULL/CROSS/SELF JOIN | 2.5h |
| 07 | [子查询与 CTE](lessons/07-子查询与CTE.md) | 标量/列/表子查询、EXISTS、递归 CTE | 2.5h |
| 08 | [集合运算](lessons/08-集合运算.md) | UNION/INTERSECT/EXCEPT、比较规则 | 1h |
| 09 | [窗口函数](lessons/09-窗口函数.md) | OVER/PARTITION BY、排名、累计、移动窗口 | 2.5h |
| 10 | [DDL 建表与约束](lessons/10-DDL建表与约束.md) | CREATE TABLE、约束、ALTER、数据类型选择 | 2h |
| 11 | [DML 增删改](lessons/11-DML增删改.md) | INSERT/UPDATE/DELETE、UPSERT、RETURNING | 1.5h |
| 12 | [事务与并发](lessons/12-事务与并发.md) | ACID、隔离级别、锁、死锁、乐观并发 | 2h |
| 13 | [索引与性能优化](lessons/13-索引与性能优化.md) | B-tree 原理、EXPLAIN、慢查询优化清单 | 2.5h |
| 14 | [数据建模与范式](lessons/14-数据建模与范式.md) | ER 建模、1NF~3NF、反范式、典型设计模式 | 2h |
| 15 | [视图、序列、触发器、JSON](lessons/15-视图-序列-触发器-JSON.md) | VIEW、物化视图、TRIGGER、JSONB、全文搜索 | 2h |
| 16 | [Rust sqlx 入门](lessons/16-Rust-sqlx入门.md) | 连接池、query_as、参数绑定、迁移 | 2h |
| 17 | [Rust sqlx 进阶](lessons/17-Rust-sqlx进阶.md) | `query!` 编译期校验、事务、错误处理、连接池调优 | 2h |
| 18 | [Diesel ORM](lessons/18-Rust-Diesel.md) | Schema、CRUD、join、表达式树 | 2h |
| 19 | [SeaORM](lessons/19-Rust-SeaORM.md) | Entity、关系、ActiveModel、迁移 | 2h |
| 20 | [综合项目：Axum REST API](lessons/20-综合项目-Axum-REST-API.md) | 完整后端：路由/分层/事务/测试/迁移 | 4h |

**总计约 40 小时**。每课含：概念讲解 → 可运行示例 → 常见坑 → Rust 后端视角 → 练习题（附答案）。

### 进阶专题（阶段五毕业后续读）

| # | 课程 | 主题 | 预计时长 |
|---|------|------|---------|
| 21 | [sqlx 深度剖析](lessons/21-sqlx深度剖析.md) | 四层架构、连接池内幕、`query!` 宏编译期魔法、语句缓存与流式查询 | 2h |
| 22 | [Diesel 深度剖析](lessons/22-Diesel深度剖析.md) | `table!` 宏展开、`.eq()` 约束链、GROUP BY 类型安全、表达式树 → SQL | 2h |

> 深入章节回答"为什么"：池为什么这样配、宏到底生成了什么、类型错误从哪读起——适合准备生产部署或技术选型答辩时精读。

## 环境要求

| 工具 | 用途 | 检查命令 |
|------|------|---------|
| SQLite ≥ 3.35 | 默认练习环境（零配置） | `sqlite3 --version` |
| PostgreSQL ≥ 14 | 进阶/生产环境（可选） | `psql --version` |
| Rust ≥ 1.75 | 阶段五实战 | `cargo --version` |

## 快速开始（3 步）

```bash
# 1. 初始化练习数据库（SQLite，生成 shop.db）
./setup/init.sh

# 2. 进入练习环境
sqlite3 shop.db

# 3. 验证数据
SELECT COUNT(*) FROM products;   -- 应返回 24
```

> 想用 PostgreSQL？见 [00 课](lessons/00-环境搭建与示例数据库.md) 的 PG 初始化说明。

## 目录结构

```
learn-sql/
├── README.md                 # 本文件
├── setup/                    # 示例数据库初始化
│   ├── init.sh               # 一键初始化（SQLite）
│   ├── schema.sql            # SQLite 建表脚本
│   ├── seed.sql              # SQLite 种子数据
│   ├── schema.pg.sql         # PostgreSQL 建表脚本
│   └── seed.pg.sql           # PostgreSQL 种子数据
├── lessons/                  # 20 课教程文档
├── rust-sqlx-demo/           # 课程 16/17 配套可运行项目（sqlx + SQLite）
└── rust-shop-api/            # 课程 20 综合项目（Axum + sqlx REST API）
```

## 约定

- SQL 关键字大写（`SELECT`），表/列名小写蛇形（`order_items`），与 Rust 习惯一致。
- 金额一律用**整数分**存储（`price_cents`），避免浮点误差（[14 课](lessons/14-数据建模与范式.md) 讲原因）。
- 方言差异用标签标注：`-- PostgreSQL 专用` / `-- MySQL 专用` / `-- SQLite 专用`。
- 每课练习做完再看答案（`<details>` 折叠块）。

## Rust 后端 SQL 技术选型速查

| 方案 | 类型安全 | 异步 | 心智负担 | 适合场景 |
|------|---------|------|---------|---------|
| **sqlx** | 编译期 SQL 校验（`query!`） | ✅ tokio | 低 | 绝大多数后端（本教程主线） |
| **Diesel** | 最强（表达式树类型） | diesel-async | 高 | 复杂领域逻辑、纯同步代码 |
| **SeaORM** | 实体层（基于 sqlx） | ✅ | 中 | 喜欢 ActiveRecord 风格 |
| rbatis | 中 | ✅ | 低 | 从 Java/MyBatis 迁移 |

> 选型建议：**先用 sqlx 学透 SQL 本身**（本教程阶段一~四全是原生 SQL），再按需上 ORM。理解 SQL 的工程师用 ORM 才不会踩坑。
