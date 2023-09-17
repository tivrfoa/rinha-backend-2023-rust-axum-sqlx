use axum::{
    extract::{Path, State, Query},
    http::StatusCode,
    Json,
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::net::SocketAddr;
use std::time::Duration;
use uuid::Uuid;

const DATABASE_URL: &str = "postgres://root:1234@localhost/rinhadb";

#[tokio::main]
async fn main() {
    let max_connections = std::env::var("MAX_CONNECTIONS").unwrap_or("4".into()).parse::<u32>().unwrap();
    let acquire_timeout = std::env::var("ACQUIRE_TIMEOUT").unwrap_or("3".into()).parse::<u64>().unwrap();

    // set up connection pool
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(acquire_timeout))
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
            eprintln!("{}", e);
            // what to do here ...? return empty result for now
            Ok(Json(vec![]))
        },
    }
}

#[axum::debug_handler]
async fn criar_pessoa(
    State(pool): State<PgPool>,
    Json(req): Json<CriarPessoaDTO>,
) -> Result<impl IntoResponse, StatusCode> {

	if req.apelido.len() > 32 || req.nome.len() > 100 {
		return Err(StatusCode::UNPROCESSABLE_ENTITY);
	}
	let mut stack = String::with_capacity(100);
    let stack: String = match req.stack {
		Some(stacks) => {
			if stacks[0].is_empty() || stacks[1].len() > 32 {
				return Err(StatusCode::UNPROCESSABLE_ENTITY);
			}
			stack.push_str(&stacks[0]);
			for s in stacks.into_iter().skip(1) {
				if s.is_empty() || s.len() > 32 {
					return Err(StatusCode::UNPROCESSABLE_ENTITY);
				}
				stack.push(' ');
				stack.push_str(&s);
			}
			stack
		}
		None => "".to_string(),
	};

    let id = Uuid::new_v4();
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

