use axum::Router;

mod routes;

pub fn routes() -> Router {
    Router::new()
        .merge(routes::login::routes())
        .merge(routes::course_list::routes())
        .merge(routes::course::routes())
        .merge(routes::resource::routes())
        .merge(routes::proxy::routes())
}
