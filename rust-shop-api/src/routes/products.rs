//! 商品路由：列表（动态筛选+分页）、详情（JOIN+聚合）、创建（RETURNING）

use axum::{extract::{Path, Query, State}, Json};
use sqlx::{QueryBuilder, Sqlite};

use crate::{
    error::{ApiResult, AppError},
    models::{CreateProductReq, Paged, ProductDetail, ProductListItem, ProductQuery},
    state::AppState,
};

/// GET /api/products?min=&max=&cat=&q=&page=&per_page=
///
/// 17 课 §4：QueryBuilder 动态 WHERE，主查询与 COUNT 复用同一组条件。
pub async fn list(
    State(st): State<AppState>,
    Query(p): Query<ProductQuery>,
) -> ApiResult<Paged<ProductListItem>> {
    let per_page = p.per_page.clamp(1, 100); // 防止 per_page=100000 拖库
    let page = p.page.max(1);
    let offset = (page - 1) * per_page;

    // 主查询
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        "SELECT p.id, p.name, c.name AS category, p.price_cents, p.stock
         FROM products p JOIN categories c ON c.id = p.category_id WHERE 1=1",
    );
    push_filters(&mut qb, &p);

    qb.push(" ORDER BY p.price_cents DESC, p.id LIMIT ");
    qb.push_bind(per_page);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let items: Vec<ProductListItem> = qb.build_query_as().fetch_all(&st.pool).await?;

    // 计数查询（同一组条件）
    let mut qc: QueryBuilder<Sqlite> = QueryBuilder::new(
        "SELECT COUNT(*) FROM products p WHERE 1=1",
    );
    push_filters(&mut qc, &p);
    let (total,): (i64,) = qc.build_query_as().fetch_one(&st.pool).await?;

    Ok(Json(Paged { items, total, page, per_page }))
}

/// WHERE 片段复用：主查询与计数查询不漂移（17 课建议）
/// 生命周期对齐：绑定的值必须与 QueryBuilder 的 'args 同寿，故共用 'a
fn push_filters<'a>(qb: &mut QueryBuilder<'a, Sqlite>, p: &'a ProductQuery) {
    if let Some(min) = p.min {
        qb.push(" AND p.price_cents >= ").push_bind(min);
    }
    if let Some(max) = p.max {
        qb.push(" AND p.price_cents <= ").push_bind(max);
    }
    if let Some(cat) = p.cat {
        qb.push(" AND p.category_id = ").push_bind(cat);
    }
    if let Some(q) = &p.q {
        // LIKE 的 % 放绑定参数里（16 课 §6）；参数由驱动安全转义
        qb.push(" AND p.name LIKE '%' || ").push_bind(q).push(" || '%'");
    }
}

/// GET /api/products/{id}
///
/// 06 课 LEFT JOIN + 05 课聚合：没评价的商品 review_cnt=0、avg=NULL
pub async fn get_one(
    State(st): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<ProductDetail> {
    let row: Option<ProductDetail> = sqlx::query_as(
        "SELECT p.id, p.name, c.name AS category, p.price_cents, p.stock,
                COUNT(r.id)            AS review_cnt,
                ROUND(AVG(r.rating),2) AS avg_rating
         FROM products p
         JOIN categories c ON c.id = p.category_id
         LEFT JOIN reviews r ON r.product_id = p.id
         WHERE p.id = ?1
         GROUP BY p.id, p.name, c.name, p.price_cents, p.stock",
    )
    .bind(id)
    .fetch_optional(&st.pool) // 16 课：可能不存在 → Option → 404
    .await?;

    match row {
        Some(d) => Ok(Json(d)),
        None => Err(AppError::NotFound),
    }
}

/// POST /api/products
///
/// 11 课 RETURNING：一条语句拿回自增 id 与默认值填充后的完整行。
/// UNIQUE(name) 冲突 → error.rs 映射为 409。
pub async fn create(
    State(st): State<AppState>,
    Json(req): Json<CreateProductReq>,
) -> ApiResult<crate::models::Product> {
    let row: crate::models::Product = sqlx::query_as(
        "INSERT INTO products (category_id, name, price_cents, stock)
         VALUES (?1, ?2, ?3, ?4)
         RETURNING id, category_id, name, price_cents, stock",
    )
    .bind(req.category_id)
    .bind(&req.name)
    .bind(req.price_cents)
    .bind(req.stock)
    .fetch_one(&st.pool)
    .await?;
    Ok(Json(row))
}

/// 供 orders 模块复用：按 id 批量取商品（避免 N+1，13 课）
/// 泛型 Executor：既能吃池（&Pool）也能吃事务（&mut Connection）——16 课 §9 的进阶版
pub(crate) async fn get_products_by_ids<'e, E>(
    executor: E,
    ids: &[i64],
) -> Result<Vec<crate::models::Product>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT id, category_id, name, price_cents, stock FROM products WHERE id IN ({placeholders})"
    );
    let mut q = sqlx::query_as::<_, crate::models::Product>(&sql);
    for id in ids {
        q = q.bind(id);
    }
    q.fetch_all(executor).await
}
