use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::net::SocketAddr;
use std::time::Duration;
use uuid::Uuid;

// const DATABASE_URL: &str = "postgres://root:1234@localhost/rinhadb";
// const DATABASE_URL: &str = "/var/run/postgresql/.s.PGSQL.5432";

#[tokio::main]
async fn main() {
    let max_connections = std::env::var("MAX_CONNECTIONS")
        .unwrap_or("4".into())
        .parse::<u32>()
        .unwrap();
    let acquire_timeout = std::env::var("ACQUIRE_TIMEOUT")
        .unwrap_or("3".into())
        .parse::<u64>()
        .unwrap();
    println!("Max connections: {max_connections}\nAcquire Timeout: {acquire_timeout}");

	let database_url = &std::env::var("DB_URL").unwrap();
    println!("Database URL: {database_url}");

    // set up connection pool
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(acquire_timeout))
        .connect(database_url)
        .await
        .expect("can't connect to database");

    // build our application with some routes
    let app = Router::new()
        .route("/pessoas/:id", get(consultar_pessoa))
        .route("/pessoas", post(criar_pessoa).get(pesquisar_termo))
        .route("/contagem-pessoas", get(contagem_pessoas))
        .with_state(pool);

    let port = std::env::var("HTTP_PORT")
        .unwrap_or("8080".into())
        .parse::<u16>()
        .unwrap();
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    println!("Listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn consultar_pessoa(
    Path(id): Path<String>,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, StatusCode> {
    let query_result = sqlx::query_as!(
        PessoaDTO,
        r#"SELECT ID, APELIDO, NOME, NASCIMENTO, STACK
         FROM PESSOAS P
         WHERE P.ID = $1"#,
        id
    )
    .fetch_one(&pool)
    .await;
    match query_result {
        Ok(pessoa) => Ok(Json(pessoa)),
        Err(_) => Err(StatusCode::NOT_FOUND),
    }
}

#[axum::debug_handler]
async fn contagem_pessoas(State(pool): State<PgPool>) -> String {
    sqlx::query!("SELECT COUNT(*) FROM PESSOAS")
        .fetch_one(&pool)
        .await
        .unwrap()
        .count
        .unwrap()
        .to_string()
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
    let query_result = sqlx::query_as!(
        PessoaDTO,
        r#"SELECT ID, APELIDO, NOME, NASCIMENTO, STACK
         FROM PESSOAS P
         WHERE P.BUSCA_TRGM LIKE $1
         LIMIT 50"#,
        t.to_lowercase()
    )
    .fetch_all(&pool)
    .await;
    match query_result {
        Ok(pessoas) => Ok(Json(pessoas)),
        Err(_) => Ok(Json(vec![])),
    }
}
#[axum::debug_handler]
async fn criar_pessoa(
    State(pool): State<PgPool>,
    Json(req): Json<CriarPessoaDTO>,
) -> Result<impl IntoResponse, StatusCode> {
    let stack_str = match validate_person_and_return_stack(&req) {
        Ok(s) => s,
        Err(_) => return Err(StatusCode::UNPROCESSABLE_ENTITY),
    };

    let id = Uuid::new_v4();
    let query_result = sqlx::query!(
        r#"INSERT INTO pessoas (id, apelido, nome, nascimento, stack, stack_str)
        values ($1, $2, $3, $4, $5, $6)"#,
        id.to_string(),
        req.apelido,
        req.nome,
        req.nascimento,
        req.stack.as_deref(),
        stack_str,
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
pub struct PessoaDTO {
    pub id: String,
    pub apelido: String,
    pub nome: String,
    pub nascimento: String,
    pub stack: Option<Vec<String>>,
}

enum ValidationError {
    InvalidInput,
}

use ValidationError::*;

fn validate_person_and_return_stack(req: &CriarPessoaDTO) -> Result<String, ValidationError> {
    if req.apelido.len() > 32 || req.nome.len() > 100 || !is_data_nascimento_valida(&req.nascimento)
    {
        return Err(InvalidInput);
    }

    match &req.stack {
        Some(stacks) => {
            for s in stacks {
                if s.is_empty() || s.len() > 32 {
                    return Err(InvalidInput);
                }
            }
            Ok(stacks.join(" "))
        }
        None => Ok("".to_string()),
    }
}

fn is_data_nascimento_valida(date: &str) -> bool {
    if date.len() != 10 || &date[4..=4] != "-" || &date[7..=7] != "-" {
        return false;
    }

    let year = match date[..4].parse::<u16>() {
        Ok(year) => {
            if year == 0 {
                return false;
            }
            year
        }
        Err(_) => {
            return false;
        }
    };

    let month = match date[5..7].parse::<u8>() {
        Ok(month) => {
            if month == 0 || month > 12 {
                return false;
            }
            month
        }
        Err(_) => {
            return false;
        }
    };

    match date[8..].parse::<u8>() {
        Ok(day) => {
            if day == 0 || day > 31 {
                return false;
            }
            match month {
                2 => {
                    let is_leap_year = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
                    if (is_leap_year && day > 29) || (!is_leap_year && day > 28) {
                        return false;
                    }
                }
                4 | 6 | 9 | 11 => {
                    if day == 31 {
                        return false;
                    }
                }
                _ => (),
            }
        }
        Err(_) => {
            return false;
        }
    }

    true
}
