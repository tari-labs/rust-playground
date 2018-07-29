use bodyparser;
use corsware::{CorsMiddleware, AllowedOrigins, UniCase};
use iron::{
    self,
    headers::ContentType,
    method::Method::{Get, Post},
    modifiers::Header,
    prelude::*,
    status,
};
use mount::Mount;
use playground_middleware::{
    Staticfile, Cache, Prefix, ModifyWith, GuessContentType, FileLogger, StatisticLogger, Rewrite
};
use router::Router;
use serde::{Serialize, de::DeserializeOwned};
use serde_json;
use std::{
    any::Any,
    convert::TryInto,
    sync::Arc,
    time::Duration,
};

use ::{
    CachedSandbox,
    ClippyRequest,
    ClippyResponse,
    CompileRequest,
    CompileResponse,
    Config,
    Error,
    EvaluateRequest,
    EvaluateResponse,
    ExecuteRequest,
    ExecuteResponse,
    FormatRequest,
    FormatResponse,
    MetaCratesResponse,
    MetaGistCreateRequest,
    MetaGistResponse,
    MetaVersionResponse,
    MiriRequest,
    MiriResponse,
    ONE_DAY_IN_SECONDS,
    ONE_HOUR_IN_SECONDS,
    ONE_YEAR_IN_SECONDS,
    Result,
    Sandbox,
    SandboxCache,
    gist,
};

pub fn run(config: Config) {
    let files = Staticfile::new(&config.root).expect("Unable to open root directory");
    let mut files = Chain::new(files);
    let one_day = Duration::new(ONE_DAY_IN_SECONDS, 0);
    let one_year = Duration::new(ONE_YEAR_IN_SECONDS, 0);

    files.link_after(ModifyWith::new(Cache::new(one_day)));
    files.link_after(Prefix::new(&["assets"], Cache::new(one_year)));
    files.link_after(GuessContentType::new(ContentType::html().0));

    let mut gist_router = Router::new();
    gist_router.post("/", meta_gist_create, "gist_create");
    gist_router.get("/:id", meta_gist_get, "gist_get");

    let mut mount = Mount::new();
    mount.mount("/", files);
    mount.mount("/compile", compile);
    mount.mount("/execute", execute);
    mount.mount("/format", format);
    mount.mount("/clippy", clippy);
    mount.mount("/miri", miri);
    mount.mount("/meta/crates", meta_crates);
    mount.mount("/meta/version/stable", meta_version_stable);
    mount.mount("/meta/version/beta", meta_version_beta);
    mount.mount("/meta/version/nightly", meta_version_nightly);
    mount.mount("/meta/gist", gist_router);
    mount.mount("/evaluate.json", evaluate);

    let mut chain = Chain::new(mount);
    let file_logger = FileLogger::new(config.logfile).expect("Unable to create file logger");
    let logger = StatisticLogger::new(file_logger);
    let rewrite = Rewrite::new(vec![vec!["help".into()]], "/index.html".into());
    let gh_token = GhToken::new(config.gh_token);

    chain.link_around(logger);
    chain.link_before(rewrite);
    chain.link_before(gh_token);

    if config.cors_enabled {
        chain.link_around(CorsMiddleware {
            // A null origin occurs when you make a request from a
            // page hosted on a filesystem, such as when you read the
            // Rust book locally
            allowed_origins: AllowedOrigins::Any { allow_null: true },
            allowed_headers: vec![UniCase("Content-Type".to_owned())],
            allowed_methods: vec![Get, Post],
            exposed_headers: vec![],
            allow_credentials: false,
            max_age_seconds: ONE_HOUR_IN_SECONDS,
            prefer_wildcard: true,
        });
    }

    info!("Starting the server on http://{}:{}", config.address, config.port);
    Iron::new(chain).http((&*config.address, config.port)).expect("Unable to start server");
}

#[derive(Debug, Clone)]
struct GhToken(Arc<String>);

impl GhToken {
    fn new(token: String) -> Self {
        GhToken(Arc::new(token))
    }
}

impl iron::BeforeMiddleware for GhToken {
    fn before(&self, req: &mut Request) -> IronResult<()> {
        req.extensions.insert::<Self>(self.clone());
        Ok(())
    }
}

impl iron::typemap::Key for GhToken {
    type Value = Self;
}


fn compile(req: &mut Request) -> IronResult<Response> {
    with_sandbox(req, |sandbox, req: CompileRequest| {
        let req = try!(req.try_into());
        sandbox
            .compile(&req)
            .map(CompileResponse::from)
            .map_err(Error::Sandbox)
    })
}

fn execute(req: &mut Request) -> IronResult<Response> {
    with_sandbox(req, |sandbox, req: ExecuteRequest| {
        let req = try!(req.try_into());
        sandbox
            .execute(&req)
            .map(ExecuteResponse::from)
            .map_err(Error::Sandbox)
    })
}

fn format(req: &mut Request) -> IronResult<Response> {
    with_sandbox(req, |sandbox, req: FormatRequest| {
        let req = try!(req.try_into());
        sandbox
            .format(&req)
            .map(FormatResponse::from)
            .map_err(Error::Sandbox)
    })
}

fn clippy(req: &mut Request) -> IronResult<Response> {
    with_sandbox(req, |sandbox, req: ClippyRequest| {
        sandbox
            .clippy(&req.into())
            .map(ClippyResponse::from)
            .map_err(Error::Sandbox)
    })
}

fn miri(req: &mut Request) -> IronResult<Response> {
    with_sandbox(req, |sandbox, req: MiriRequest| {
        sandbox
            .miri(&req.into())
            .map(MiriResponse::from)
            .map_err(Error::Sandbox)
    })
}

fn meta_crates(_req: &mut Request) -> IronResult<Response> {
    with_sandbox_no_request(|sandbox| {
        cached(sandbox)
            .crates()
            .map(MetaCratesResponse::from)
    })
}

fn meta_version_stable(_req: &mut Request) -> IronResult<Response> {
    with_sandbox_no_request(|sandbox| {
        cached(sandbox)
            .version_stable()
            .map(MetaVersionResponse::from)
    })
}

fn meta_version_beta(_req: &mut Request) -> IronResult<Response> {
    with_sandbox_no_request(|sandbox| {
        cached(sandbox)
            .version_beta()
            .map(MetaVersionResponse::from)
    })
}

fn meta_version_nightly(_req: &mut Request) -> IronResult<Response> {
    with_sandbox_no_request(|sandbox| {
        cached(sandbox)
            .version_nightly()
            .map(MetaVersionResponse::from)
    })
}

fn meta_gist_create(req: &mut Request) -> IronResult<Response> {
    let token = req.extensions.get::<GhToken>().unwrap().0.as_ref().clone();
    serialize_to_response(deserialize_from_request(req, |r: MetaGistCreateRequest| {
        let gist = gist::create(token, r.code);
        Ok(MetaGistResponse::from(gist))
    }))
}

/// A convenience constructor
fn cached(sandbox: Sandbox) -> CachedSandbox<'static> {
    lazy_static! {
        static ref SANDBOX_CACHE: SandboxCache = Default::default();
    }

    CachedSandbox {
        sandbox,
        cache: &SANDBOX_CACHE,
    }
}

fn meta_gist_get(req: &mut Request) -> IronResult<Response> {
    match req.extensions.get::<Router>().unwrap().find("id") {
        Some(id) => {
            let token = req.extensions.get::<GhToken>().unwrap().0.as_ref().clone();
            let gist = gist::load(token, id);
            serialize_to_response(Ok(MetaGistResponse::from(gist)))
        }
        None => {
            Ok(Response::with(status::UnprocessableEntity))
        }
    }
}

// This is a backwards compatibilty shim. The Rust homepage and the
// documentation use this to run code in place.
fn evaluate(req: &mut Request) -> IronResult<Response> {
    with_sandbox(req, |sandbox, req: EvaluateRequest| {
        let req = req.try_into()?;
        sandbox
            .execute(&req)
            .map(EvaluateResponse::from)
            .map_err(Error::Sandbox)
    })
}

fn with_sandbox<Req, Resp, F>(req: &mut Request, f: F) -> IronResult<Response>
where
    F: FnOnce(Sandbox, Req) -> Result<Resp>,
    Req: DeserializeOwned + Clone + Any + 'static,
    Resp: Serialize,
{
    serialize_to_response(run_handler(req, f))
}

fn with_sandbox_no_request<Resp, F>(f: F) -> IronResult<Response>
where
    F: FnOnce(Sandbox) -> Result<Resp>,
    Resp: Serialize,
{
    serialize_to_response(run_handler_no_request(f))
}

fn run_handler<Req, Resp, F>(req: &mut Request, f: F) -> Result<Resp>
where
    F: FnOnce(Sandbox, Req) -> Result<Resp>,
    Req: DeserializeOwned + Clone + Any + 'static,
{
    deserialize_from_request(req, |req| {
        let sandbox = Sandbox::new()?;
        f(sandbox, req)
    })
}

fn deserialize_from_request<Req, Resp, F>(req: &mut Request, f: F) -> Result<Resp>
where
    F: FnOnce(Req) -> Result<Resp>,
    Req: DeserializeOwned + Clone + Any + 'static,
{
    let body = req.get::<bodyparser::Struct<Req>>()
        .map_err(Error::Deserialization)?;

    let req = body.ok_or(Error::RequestMissing)?;

    let resp = f(req)?;

    Ok(resp)
}

fn run_handler_no_request<Resp, F>(f: F) -> Result<Resp>
where
    F: FnOnce(Sandbox) -> Result<Resp>,
{
    let sandbox = Sandbox::new()?;
    let resp = f(sandbox)?;
    Ok(resp)
}

fn serialize_to_response<Resp>(response: Result<Resp>) -> IronResult<Response>
where
    Resp: Serialize,
{
    let response = response.and_then(|resp| {
        let resp = serde_json::ser::to_string(&resp)?;
        Ok(resp)
    });

    match response {
        Ok(body) => Ok(Response::with((status::Ok, Header(ContentType::json()), body))),
        Err(err) => {
            let err = ErrorJson { error: err.to_string() };
            match serde_json::ser::to_string(&err) {
                Ok(error_str) => Ok(Response::with((status::InternalServerError, Header(ContentType::json()), error_str))),
                Err(_) => Ok(Response::with((status::InternalServerError, Header(ContentType::json()), FATAL_ERROR_JSON))),
            }
        },
    }
}

#[derive(Debug, Clone, Serialize)]
struct ErrorJson {
    error: String,
}

const FATAL_ERROR_JSON: &str =
    r#"{"error": "Multiple cascading errors occurred, abandon all hope"}"#;
