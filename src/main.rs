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

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

const DATABASE_URL: &str = "postgres://root:1234@localhost/rinhadb";

struct AppState {
    pool: PgPool,
    cache: Mutex<Cache>,
    http_client: reqwest::Client,
}

#[derive(Debug)]
struct CacheTermoBusca {
	termo: String,
	id: String,
}

struct Cache {
    pessoa_map: HashMap<String, PessoaDTO>,
    apelidos: HashSet<String>,
	termos_busca: Vec<CacheTermoBusca>,
}

impl Cache {
    fn get_pessoa(&self, id: &str) -> Option<PessoaDTO> {
        self.pessoa_map.get(id).map(|p| p.clone())
    }

    fn insert(&mut self, pessoa: PessoaDTO) -> bool {
		if let Some(ref stack) = pessoa.stack {
			let stack_str = stack.join(" ").to_lowercase();
			self.termos_busca.push(CacheTermoBusca {
				termo: stack_str,
				id: pessoa.id.clone(),
			});
		}
        self.apelidos.insert(pessoa.apelido.clone());
        self.pessoa_map.insert(pessoa.id.clone(), pessoa);
        false
    }

    fn apelido_exists(&self, apelido: &str) -> bool {
        self.apelidos.contains(apelido)
    }

	fn pesquisar_termo(&self, termo: &str) -> Vec<PessoaDTO> {
		// println!("pesquisando {termo}");
		// dbg!(&self.termos_busca);
		let mut pessoas: Vec<PessoaDTO> = Vec::with_capacity(50);
		for tb in &self.termos_busca {
			if tb.termo.find(termo).is_some() {
				pessoas.push(self.pessoa_map.get(&tb.id).unwrap().clone());
				if pessoas.len() == 50 {
					break;
				}
			}
		}
		pessoas
	}
}

static mut PORT: u16 = 8080;
static mut BROTHER_URL: Option<String> = None; // xD

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

    // set up connection pool
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(acquire_timeout))
        .connect(DATABASE_URL)
        .await
        .expect("can't connect to database");

    let cache: Mutex<Cache> = Mutex::new(Cache {
        pessoa_map: HashMap::new(),
        apelidos: HashSet::new(),
		termos_busca: Vec::with_capacity(46576),
    });

    let app_state = Arc::new(AppState {
        pool,
        cache,
        http_client: reqwest::Client::new(),
    });

    // build our application with some routes
    let app = Router::new()
        .route("/pessoas/:id", get(consultar_pessoa))
        .route("/pessoas", post(criar_pessoa).get(pesquisar_termo))
        .route("/contagem-pessoas", get(contagem_pessoas))
        .route("/pessoas/cache", post(cache_pessoa))
        .with_state(app_state);

    if let Ok(brother_port) = std::env::var("BROTHER_PORT") {
        unsafe {
            BROTHER_URL = Some(format!("http://localhost:{brother_port}/pessoas/cache"));
        }
    }

    if let Ok(port) = std::env::var("HTTP_PORT") {
        unsafe {
            PORT = port.parse().unwrap();
        }
    }
    let addr = SocketAddr::from(([127, 0, 0, 1], unsafe { PORT }));
    println!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn consultar_pessoa(
    Path(id): Path<String>,
    State(shared_state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, StatusCode> {
    if let Some(pessoa) = shared_state.cache.lock().unwrap().get_pessoa(&id) {
        // println!("Found pessoa in cache");
        return Ok(Json(pessoa));
    }
    let query_result = sqlx::query_as!(
        PessoaDTO,
        r#"SELECT ID, APELIDO, NOME, NASCIMENTO, STACK
         FROM PESSOAS P
         WHERE P.ID = $1"#,
        id
    )
    .fetch_one(&shared_state.pool)
    .await;
    match query_result {
        Ok(pessoa) => Ok(Json(pessoa)),
        Err(_) => Err(StatusCode::NOT_FOUND),
    }
}

#[axum::debug_handler]
async fn contagem_pessoas(State(shared_state): State<Arc<AppState>>) -> String {
    // dbg!(&shared_state.cache.lock().unwrap().pessoa_map);
    sqlx::query!("SELECT COUNT(*) FROM PESSOAS")
        .fetch_one(&shared_state.pool)
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
    State(shared_state): State<Arc<AppState>>,
) -> Result<Json<Vec<PessoaDTO>>, StatusCode> {
    if termo.t.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
	Ok(Json(shared_state.cache.lock().unwrap().pesquisar_termo(&termo.t.to_lowercase())))
}

async fn criar_pessoa(
    State(shared_state): State<Arc<AppState>>,
    Json(req): Json<CriarPessoaDTO>,
) -> Result<impl IntoResponse, StatusCode> {
    if shared_state
        .cache
        .lock()
        .unwrap()
        .apelido_exists(&req.apelido)
    {
        // println!("apelido already exists!");
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }

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
    .execute(&shared_state.pool)
    .await;
    match query_result {
        Ok(_) => {
			let pessoa = PessoaDTO::from_CriarPessoaDTO(id.to_string(), &req);
            replicate_cache(&shared_state.http_client, &pessoa).await;
			shared_state.cache.lock().unwrap().insert(pessoa.clone());
            Ok((
                StatusCode::CREATED,
                [("location", format!("/pessoas/{}", id))],
            ))
        }
        Err(e) => {
			println!("ERROR criar_pessoa: {:?}", e);
			Err(StatusCode::UNPROCESSABLE_ENTITY)
		}
    }
}

#[axum::debug_handler]
async fn cache_pessoa(
    State(shared_state): State<Arc<AppState>>,
    Json(req): Json<PessoaDTO>,
) -> Result<impl IntoResponse, StatusCode> {
    shared_state.cache.lock().unwrap().insert(req);
    Ok(())
}

async fn replicate_cache(http_client: &reqwest::Client, pessoa: &PessoaDTO) {
	unsafe {
		if let Some(url) = &BROTHER_URL {
			if let Err(e) = http_client
				.post(url)
				.json(pessoa)
				.send()
				.await
			{
				println!("Error replicating cache: {:?}", e);
			}
		}
	}
}

#[derive(Debug, Deserialize)]
pub struct CriarPessoaDTO {
    pub apelido: String,
    pub nome: String,
    pub nascimento: String,
    pub stack: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PessoaDTO {
    pub id: String,
    pub apelido: String,
    pub nome: String,
    pub nascimento: String,
    pub stack: Option<Vec<String>>,
}

impl PessoaDTO {
    #[allow(non_snake_case)]
    fn from_CriarPessoaDTO(id: String, pessoa: &CriarPessoaDTO) -> Self {
        Self {
            id,
            apelido: pessoa.apelido.clone(),
            nome: pessoa.nome.clone(),
            nascimento: pessoa.nascimento.clone(),
            stack: pessoa.stack.clone(),
        }
    }
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
    if date.len() != 10 || &date[4..=4] != "-" || &date[7..=7] != "-" || &date[5..=6] > "12" {
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
