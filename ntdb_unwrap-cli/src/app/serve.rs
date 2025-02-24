use crate::Result;
use axum::http::StatusCode;
use clap::ArgMatches;
use serde::{Deserialize, Serialize};
use snafu::Snafu;
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use utoipa::{IntoParams, OpenApi, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_scalar::{Scalar, Servable};

#[derive(Debug, Clone)]
struct AppState {
    bootstrap: Arc<Mutex<super::common::Bootstrap>>,
}
pub struct Serve {
    listen: SocketAddr,
    state: AppState,
}
pub fn serve(matches: ArgMatches) -> Result<Serve> {
    let bootstrap = super::common::bootstrap(&matches)?;
    let listen = matches.get_one::<SocketAddr>("listen").unwrap().to_owned();
    Ok(Serve {
        state: AppState {
            bootstrap: Arc::new(Mutex::new(bootstrap)),
        },
        listen,
    })
}
impl super::App for Serve {
    fn run(self: Box<Self>) -> Result<()> {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(self.async_run())
    }
}
#[derive(OpenApi)]
#[openapi()]
struct ApiDoc;
impl Serve {
    async fn async_run(self) -> Result<()> {
        let (router, api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
            .nest("/api/", handlers::router(self.state))
            .split_for_parts();

        let router = router.merge(Scalar::with_url("/api/docs", api));

        let listener = tokio::net::TcpListener::bind(&self.listen).await?;
        let serve = axum::serve(listener, router.into_make_service());
        println!("server started at: {}", &self.listen);
        println!("check the api doc at: http://{}/api/docs", &self.listen);
        let quit = tokio::signal::ctrl_c();
        tokio::select! {
            _ = serve => {},
            _ = quit => {},
        }
        Ok(())
    }
}

#[derive(Debug, ToSchema, Serialize)]
struct PagedList<T> {
    pub limit: u64,
    pub offset: u64,
    #[schema(value_type=[Object])]
    pub items: Vec<T>,
}
#[derive(Debug, Snafu)]
enum ApiError {
    #[snafu(context(false))]
    Internal { source: crate::Error },
    #[allow(dead_code)]
    ClientError { code: StatusCode, detail: String },
}
impl From<ntdb_unwrap::Error> for ApiError {
    fn from(value: ntdb_unwrap::Error) -> Self {
        Self::from(crate::Error::from(value))
    }
}
impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        #[derive(Serialize)]
        struct ErrorBody {
            #[serde(skip_serializing_if = "Option::is_none")]
            message: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            reason: Option<String>,
        }
        match self {
            Self::Internal { source } => {
                let mut r = axum::response::Json(ErrorBody {
                    message: Some("Internal Server Error".to_string()),
                    reason: Some(format!("{:?}", source)),
                })
                .into_response();
                *r.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                r
            }
            Self::ClientError { code, detail } => {
                let mut r = axum::response::Json(ErrorBody {
                    message: Some(
                        code.canonical_reason()
                            .unwrap_or("Unknown Status Code")
                            .to_string(),
                    ),
                    reason: Some(detail),
                })
                .into_response();
                *r.status_mut() = code;
                r
            }
        }
    }
}
#[derive(Debug, IntoParams, Deserialize)]
struct ListQuery {
    offset: Option<u64>,
    limit: Option<u64>,
}
mod handlers {
    use crate::SqliteSnafu;
    use axum::{
        Json,
        extract::{Query, State},
    };
    use ntdb_unwrap::db::model::{self, Model};
    use rusqlite::params;
    use snafu::ResultExt;
    use utoipa_axum::{router::OpenApiRouter, routes};

    use super::*;
    type Result<T> = std::result::Result<T, ApiError>;

    pub fn router(state: AppState) -> OpenApiRouter {
        OpenApiRouter::new()
            .routes(routes!(get_group_msg_table))
            .with_state(state)
    }
    #[utoipa::path(
        get,
        path = "/nt_msg/group_msg_table", 
        params(
            ListQuery,
        ),
        responses(
            (status=200,description="group_msg_table",body = PagedList<PagedList<Object>>),
        )
    )]
    pub async fn get_group_msg_table(
        State(s): State<AppState>,
        q: Query<ListQuery>,
    ) -> Result<Json<PagedList<model::GroupMsgTable>>> {
        let b = s.bootstrap.lock().unwrap();
        let conn = &b.conn;
        let prepare_stmt = "SELECT * FROM group_msg_table ORDER BY `40050` DESC LIMIT ? OFFSET ?;";
        let mut stmt = conn.prepare(prepare_stmt).context(SqliteSnafu {
            op: format!("prepare stmt: {}", &prepare_stmt),
        })?;
        let (limit, offset) = (q.limit.unwrap_or(10), q.offset.unwrap_or(0));
        let params = params![limit, offset];
        let mut rows = stmt.query(params).with_context(|_| SqliteSnafu {
            op: format!(
                "query stmt {} with {:?}",
                &prepare_stmt,
                params.iter().map(|x| x.to_sql()).collect::<Vec<_>>()
            ),
        })?;
        let items = model::GroupMsgTable::parse_rows(&mut rows)?;
        Ok(Json(PagedList {
            limit,
            offset,
            items,
        }))
    }
}
