//! Example of application using <https://github.com/launchbadge/sqlx>
//!
//! Run with
//!
//! ```not_rust
//! cargo run -p example-sqlx-postgres
//! ```
//!
//! Test with curl:
//!
//! ```not_rust
//! curl 127.0.0.1:3000
//! curl -X POST 127.0.0.1:3000
//! ```

use axum::{
    extract::{Path, State, Query},
    http::StatusCode,
    Json,
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use sqlx::postgres::{PgPool, PgPoolOptions};
use tokio_postgres::Row;

use serde::{Deserialize, Serialize};

use std::net::SocketAddr;
use std::time::Duration;

use uuid::Uuid;

const DATABASE_URL: &str = "postgres://root:1234@localhost/rinhadb";

#[tokio::main]
async fn main() {

    // set up connection pool
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(5))
        .connect(DATABASE_URL)
        .await
        .expect("can't connect to database");

    // build our application with some routes
    let app = Router::new()
        .route("/pessoas/:id", get(consultar_pessoa))
        .route("/pessoas",
            post(criar_pessoa)
            .get(pesquisar_termo)
        )
        .route("/contagem-pessoas", get(contagem_pessoas))
        .with_state(pool);

    let port = std::env::var("HTTP_PORT").unwrap_or("8080".into()).parse::<u16>().unwrap();
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    println!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn consultar_pessoa(
    Path(id): Path<String>,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, StatusCode> {
    let query_result = sqlx::query_as!(Pessoa,
        r#"SELECT ID, APELIDO, NOME, NASCIMENTO, STACK
         FROM PESSOAS P
         WHERE P.ID = $1"#,
        id)
        .fetch_one(&pool)
        .await;
    match query_result {
        Ok(pessoa) => Ok(Json(pessoa.to_pessoa_dto())),
        Err(_) => Err(StatusCode::NOT_FOUND),
    }
}

#[axum::debug_handler]
async fn contagem_pessoas(
    State(pool): State<PgPool>,
) -> String {
    sqlx::query!("SELECT COUNT(*) FROM PESSOAS")
        .fetch_one(&pool)
        .await
        .unwrap()
        .count.unwrap().to_string()
}

#[derive(Deserialize)]
struct TermoPesquisa {
    t: String,
}

async fn pesquisar_termo(
    Query(termo): Query<TermoPesquisa>,
    State(pool): State<PgPool>,
) -> Result<Json<Vec<PessoaDTO>>, StatusCode> {
    if termo.t.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut t = String::with_capacity(termo.t.len() + 2);
    t.push('%');
    t.push_str(&termo.t);
    t.push('%');
    let query_result = sqlx::query_as!(Pessoa,
        r#"SELECT ID, APELIDO, NOME, NASCIMENTO, STACK
         FROM PESSOAS P
         WHERE P.BUSCA_TRGM LIKE $1
         LIMIT 50"#,
        t.to_lowercase())
        .fetch_all(&pool)
        .await;
    match query_result {
        Ok(pessoas) => {
            let pessoas: Vec<PessoaDTO> = pessoas.into_iter().map(|p| p.to_pessoa_dto()).collect();
            Ok(Json(pessoas))
        },
        Err(e) => {
            dbg!(e);
            panic!("Failed to search by term: {}", t);
        },
    }
}

#[axum::debug_handler]
async fn criar_pessoa(
    State(pool): State<PgPool>,
    Json(req): Json<CriarPessoaDTO>,
) -> Result<impl IntoResponse, StatusCode> {
    let id = Uuid::new_v4();
    let stack = "todo".to_string();
    let query_result = sqlx::query!(
        r#"INSERT INTO pessoas (id, apelido, nome, nascimento, stack)
        values ($1, $2, $3, $4, $5)"#,
        id.to_string(),
        req.apelido,
        req.nome,
        req.nascimento,
        stack,
    )
        .execute(&pool)
        .await;
    match query_result {
        Ok(_) => Ok((
            StatusCode::CREATED,
            [("location", format!("/pessoas/{}", id))],
        )),
        Err(_) => Err(StatusCode::UNPROCESSABLE_ENTITY),
    }
}

#[derive(Debug, Deserialize)]
pub struct CriarPessoaDTO {
    pub apelido: String,
    pub nome: String,
    pub nascimento: String,
    pub stack: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Pessoa {
    pub id: String,
    pub apelido: String,
    pub nome: String,
    pub nascimento: String,
    pub stack: Option<String>,
}

impl Pessoa {
    fn to_pessoa_dto(self) -> PessoaDTO {
        let stack = match &self.stack {
            Some(v) => Some(v.split_ascii_whitespace().map(|s| s.to_string()).collect()),
            None => None,
        };
        PessoaDTO { id: self.id, apelido: self.apelido, nome: self.nome, nascimento: self.nascimento, stack: stack }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PessoaDTO {
    pub id: String,
    pub apelido: String,
    pub nome: String,
    pub nascimento: String,
    pub stack: Option<Vec<String>>,
}

impl PessoaDTO {
    pub fn from(row: &Row) -> PessoaDTO {
        // COLUMNS: ID, APELIDO, NOME, NASCIMENTO, STACK
        let stack: Option<String> = row.get(4);
        let stack = match stack {
            None => None,
            Some(s) => Some(s.split(' ').map(|s| s.to_string()).collect()),
        };
        PessoaDTO {
            id: row.get(0),
            apelido: row.get(1),
            nome: row.get(2),
            nascimento: row.get(3),
            stack,
        }
    }
}
