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

struct Cache {
    pessoa_map: HashMap<String, PessoaDTO>,
    apelidos: HashSet<String>,
}

impl Cache {
    fn get_pessoa(&self, id: &str) -> Option<PessoaDTO> {
        self.pessoa_map.get(id).map(|p| p.clone())
    }

    fn insert(&mut self, pessoa: PessoaDTO) -> bool {
        self.apelidos.insert(pessoa.apelido.clone());
        self.pessoa_map.insert(pessoa.id.clone(), pessoa);
        false
    }

    fn apelido_exists(&self, apelido: &str) -> bool {
        self.apelidos.contains(apelido)
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
        return Ok(Json(pessoa));
    }
    Err(StatusCode::NOT_FOUND)
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
    .fetch_all(&shared_state.pool)
    .await;
    match query_result {
        Ok(pessoas) => Ok(Json(pessoas)),
        Err(e) => {
			if matches!(e, sqlx::Error::PoolTimedOut) {
				Err(StatusCode::INTERNAL_SERVER_ERROR)
			} else {
				println!("ERROR pesquisar_termo: {}", e);
				Ok(Json(vec![]))
			}
        }
    }
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

    if !is_request_valid(&req) {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }

    let id = Uuid::new_v4();
    let busca_trgm = create_busca_trgm(&req.nome, &req.apelido, &req.stack);
    let query_result = sqlx::query!(
        r#"INSERT INTO pessoas (id, apelido, nome, nascimento, stack, busca_trgm)
        values ($1, $2, $3, $4, $5, $6)"#,
        id.to_string(),
        req.apelido,
        req.nome,
        req.nascimento,
        req.stack.as_deref(),
        busca_trgm,
    )
    .execute(&shared_state.pool)
    .await;
    match query_result {
        Ok(_) => {
            let pessoa = PessoaDTO::from_CriarPessoaDTO(id.to_string(), &req);
            replicate_cache(&shared_state.http_client, &pessoa).await;
            shared_state
                .cache
                .lock()
                .unwrap()
                .insert(pessoa);
            Ok((
                StatusCode::CREATED,
                [("location", format!("/pessoas/{}", id))],
            ))
        }
        Err(e) => {
			if matches!(e, sqlx::Error::PoolTimedOut) {
				Err(StatusCode::INTERNAL_SERVER_ERROR)
			} else {
				println!("ERROR criar_pessoa: {}", e);
				Err(StatusCode::UNPROCESSABLE_ENTITY)
			}
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
            if let Err(e) = http_client.post(url).json(pessoa).send().await {
                println!("Error replicating cache: {:?}", e);
            }
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
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
    #[inline(always)]
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

fn create_busca_trgm(nome: &str, apelido: &str, stack: &Option<Vec<String>>) -> String {
    let mut termo = String::with_capacity(apelido.len() + nome.len() + 100);
    termo.push_str(&apelido);
    termo.push(' ');
    termo.push_str(&nome);
    if let Some(ref stack) = stack {
        termo.push(' ');
        termo.push_str(&stack.join(" "));
    }
    termo.to_lowercase()
}

#[inline(always)]
fn is_request_valid(req: &CriarPessoaDTO) -> bool {
    if req.apelido.len() > 32 || req.nome.len() > 100 || !is_data_nascimento_valida(&req.nascimento)
    {
        return false;
    }

    if let Some(ref stack) = req.stack {
        for s in stack {
            if s.is_empty() || s.len() > 32 {
                return false;
            }
        }
    }

    true
}

fn is_data_nascimento_valida(date: &str) -> bool {
    if date.len() != 10 || &date[5..=6] > "12" {
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
            if month == 0 {
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
