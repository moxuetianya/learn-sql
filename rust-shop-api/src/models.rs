//! DTO：请求/响应结构（serde）+ 行映射（sqlx::FromRow）
//! 命名约定（20 课 §3 分层）：
//!   XxxRow  = 数据库行（内部）
//!   XxxReq  = 请求体（反序列化）
//!   XxxResp = 响应体（序列化）

use serde::{Deserialize, Serialize};

// ==================== 商品 ====================

/// 商品行（对应 16 课 FromRow struct）
#[derive(Debug, sqlx::FromRow, Serialize)]
pub struct Product {
    pub id: i64,
    pub category_id: i64,
    pub name: String,
    pub price_cents: i64,
    pub stock: i64,
}

/// GET /api/products 列表项 = 商品 + 分类名（JOIN 投影，06 课）
#[derive(Debug, sqlx::FromRow, Serialize)]
pub struct ProductListItem {
    pub id: i64,
    pub name: String,
    pub category: String,
    pub price_cents: i64,
    pub stock: i64,
}

/// 详情 = 商品 + 统计（LEFT JOIN 聚合，05/06 课）
#[derive(Debug, sqlx::FromRow, Serialize)]
pub struct ProductDetail {
    pub id: i64,
    pub name: String,
    pub category: String,
    pub price_cents: i64,
    pub stock: i64,
    pub review_cnt: i64,
    pub avg_rating: Option<f64>,
}

/// 列表查询参数（Query 提取，全部可选 + 默认值）
#[derive(Debug, Deserialize)]
pub struct ProductQuery {
    pub min: Option<i64>,
    pub max: Option<i64>,
    pub cat: Option<i64>,
    pub q: Option<String>,
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_per_page")]
    pub per_page: u32,
}
fn default_page() -> u32 { 1 }
fn default_per_page() -> u32 { 10 }

/// 分页响应外壳
#[derive(Serialize)]
pub struct Paged<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub page: u32,
    pub per_page: u32,
}

/// POST /api/products 请求体
#[derive(Debug, Deserialize)]
pub struct CreateProductReq {
    pub category_id: i64,
    pub name: String,
    pub price_cents: i64,
    #[serde(default)]
    pub stock: i64,
}

// ==================== 订单 ====================

#[derive(Debug, Deserialize)]
pub struct CreateOrderReq {
    pub user_id: i64,
    pub items: Vec<OrderItemReq>,
}

#[derive(Debug, Deserialize)]
pub struct OrderItemReq {
    pub product_id: i64,
    pub quantity: i64,
}

#[derive(Serialize)]
pub struct CreateOrderResp {
    pub order_id: i64,
    pub total_cents: i64,
}

/// 订单详情（含明细数组：一条 SQL + json 聚合，15/16 课）
/// items 由 handler 从 TEXT 解析为 JSON（见 routes/orders.rs）
#[derive(Debug, Serialize)]
pub struct OrderDetail {
    pub id: i64,
    pub username: String,
    pub status: String,
    pub total_cents: i64,
    pub created_at: String,
    pub items: serde_json::Value,
}

// ==================== 统计 ====================

/// 月度营收 + 环比（窗口函数 LAG，09 课）
#[derive(Debug, sqlx::FromRow, Serialize)]
pub struct MonthlyRevenue {
    pub month: String,
    pub revenue: i64,
    pub prev_revenue: Option<i64>,
    pub diff: Option<i64>,
}
