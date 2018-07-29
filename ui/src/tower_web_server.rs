// FIXME:
//
// evaluate.json no need content type
// off-thread Logging

use http::{header, Method};
use std::{
    convert::TryInto,
    io,
    net::SocketAddr,
    path::PathBuf,
    time::Duration,
};
use tokio::{
    prelude::{future::Either, Future},
};
use tower_web::{
    extract::http_date_time::HttpDateTime,
    middleware::{Identity, cors::{AllowedOrigins, CorsBuilder}, log::LogMiddleware},
    ServiceBuilder,
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

const ONE_DAY: Duration = Duration::from_secs(ONE_DAY_IN_SECONDS as u64);
const ONE_YEAR: Duration = Duration::from_secs(ONE_YEAR_IN_SECONDS as u64);
const ONE_HOUR: Duration = Duration::from_secs(ONE_HOUR_IN_SECONDS as u64);

#[derive(Debug)]
struct Index(PrecompressedAssets);

impl Index {
    fn new(base: PathBuf) -> Self {
        Index(PrecompressedAssets::new(base))
    }
}

#[derive(Debug)]
struct Assets(PrecompressedAssets);

impl Assets {
    fn new(mut base: PathBuf) -> Self {
        base.push("assets");
        Assets(PrecompressedAssets::new(base))
    }
}

#[derive(Debug)]
struct SandboxFixme;

#[derive(Debug, Default)]
struct Meta {
    cache: SandboxCache,
}

impl Meta {
    fn cached(&self, sandbox: Sandbox) -> CachedSandbox {
        CachedSandbox {
            sandbox,
            cache: &self.cache,
        }
    }
}

#[derive(Debug, Default)]
struct Gist {
    token: String,
}

impl Gist {
    fn new(token: String) -> Self {
        Self { token }
    }
}

use self::precompressed_assets::{PrecompressedAssets, FileResponse};

mod precompressed_assets {
    use http::Response;
    use std::{
        io,
        path::{Path, PathBuf},
    };
    use tokio::{
        fs::File,
        prelude::{future::Either, Future},
    };
    use tower_web::{
        codegen::bytes::BytesMut,
        extract::http_date_time::HttpDateTime,
        util::buf_stream::{Empty, empty},
    };
    use mime_guess;

    pub type FileResponse = Response<MaybeFile>;
    pub type MaybeFile = Either<File, Empty<io::Cursor<BytesMut>, io::Error>>;

    #[derive(Debug)]
    pub struct PrecompressedAssets {
        base: PathBuf,
    }

    impl PrecompressedAssets {
        pub fn new(base: PathBuf) -> Self {
            Self { base }
        }

        pub fn file<P>(
            &self,
            relative_path: P,
            if_modified_since: Option<HttpDateTime>,
        ) -> impl Future<Item = FileResponse, Error = io::Error> + Send
        where
            P: AsRef<Path>,
        {
            let relative_path = relative_path.as_ref();

            debug!("File is {}", relative_path.display());

            let requested_path = self.base.join(relative_path);

            let gz_path = {
                let mut current_ext = requested_path
                    .extension()
                    .unwrap_or_default()
                    .to_os_string();
                current_ext.push(".gz");
                requested_path.with_extension(current_ext)
            };

            debug!(
                "Looking for {} instead of {}",
                gz_path.display(),
                requested_path.display()
            );

            let ct = mime_guess::guess_mime_type(relative_path);

            File::open(gz_path)
                .map(|f| (f, true))
                .or_else(|_| File::open(requested_path).map(|f| (f, false)))
                .and_then(|(f, gzipped)| f.metadata().map(move |(f, md)| (f, md, gzipped)))
                .map(move |(f, md, gzipped)| {
                    let last_modified = md.modified().map(HttpDateTime::from);

                    let mut resp = Response::builder();

                    if let (Some(client), Ok(server)) = (&if_modified_since, &last_modified) {
                        debug!("Client has an if-modified-since date of {:?}", client);
                        debug!("Server has a last-modified date of      {:?}", server);

                        if client >= server {
                            debug!("File unchanged, returning 304");
                            return resp
                                .status(304)
                                .body(Either::B(empty()))
                                .expect("Did not create response");
                        }
                    }

                    resp.status(200).header("Content-Type", ct.to_string());

                    if gzipped {
                        debug!("Found the gzipped version of the asset");
                        resp.header("Content-Encoding", "gzip");
                    }

                    if let Ok(last_modified) = last_modified {
                        debug!("File had a modification time");
                        resp.header("Last-Modified", last_modified);
                    }

                    resp.body(Either::A(f)).expect("Did not create response")
                }).or_else(|e| {
                    debug!("AN ERROR {}", e);

                    // FIXME: Only for certain errors?

                    Ok(Response::builder()
                       .status(404)
                       .body(Either::B(empty()))
                       .expect("Did not create response"))
                }).map_err(|e| {
                    debug!("AN ERROR {}", e);
                    e
                })
        }
    }
}

impl_web! {
    impl Index {
        #[get("/")]
        fn index(
            &self,
            if_modified_since: Option<HttpDateTime>,
        ) -> impl Future<Item = FileResponse, Error = io::Error> + Send {
            self.0.file("index.html", if_modified_since)
        }

        #[get("/help")]
        fn help(
            &self,
            if_modified_since: Option<HttpDateTime>,
        ) -> impl Future<Item = FileResponse, Error = io::Error> + Send {
            self.index(if_modified_since)
        }
    }

    impl Assets {
        #[get("/assets/*asset")]
        fn asset(
            &self,
            asset: PathBuf,
            if_modified_since: Option<HttpDateTime>,
        ) -> impl Future<Item = FileResponse, Error = io::Error> + Send {
            self.0.file(asset, if_modified_since)
        }
    }

    impl SandboxFixme {
        #[post("/execute")]
        #[content_type("application/json")]
        fn execute(&self, body: ExecuteRequest) -> Result<ExecuteResponse> {
            Sandbox::new()?
                .execute(&body.try_into()?)
                .map(ExecuteResponse::from)
                .map_err(Error::Sandbox)
        }

        #[post("/compile")]
        #[content_type("application/json")]
        fn compile(&self, body: CompileRequest) -> Result<CompileResponse> {
            Sandbox::new()?
                .compile(&body.try_into()?)
                .map(CompileResponse::from)
                .map_err(Error::Sandbox)
        }

        #[post("/format")]
        #[content_type("application/json")]
        fn format(&self, body: FormatRequest) -> Result<FormatResponse> {
            Sandbox::new()?
                .format(&body.try_into()?)
                .map(FormatResponse::from)
                .map_err(Error::Sandbox)
        }

        #[post("/clippy")]
        #[content_type("application/json")]
        fn clippy(&self, body: ClippyRequest) -> Result<ClippyResponse> {
            Sandbox::new()?
                .clippy(&body.into())
                .map(ClippyResponse::from)
                .map_err(Error::Sandbox)
        }

        #[post("/miri")]
        #[content_type("application/json")]
        fn miri(&self, body: MiriRequest) -> Result<MiriResponse> {
            Sandbox::new()?
                .miri(&body.into())
                .map(MiriResponse::from)
                .map_err(Error::Sandbox)
        }

        // This is a backwards compatibilty shim. The Rust homepage and the
        // documentation use this to run code in place.
        #[post("/evaluate.json")]
        #[content_type("application/json")]
        fn evaluate(&self, body: EvaluateRequest) -> Result<EvaluateResponse> {
            Sandbox::new()?
                .execute(&body.try_into()?)
                .map(EvaluateResponse::from)
                .map_err(Error::Sandbox)
        }
    }

    impl Meta {
        #[get("/meta/crates")]
        #[content_type("application/json")]
        fn meta_crates(&self) -> Result<MetaCratesResponse> {
            self.cached(Sandbox::new()?)
                .crates()
                .map(MetaCratesResponse::from)
        }

        #[get("/meta/version/stable")]
        #[content_type("application/json")]
        fn meta_version_stable(&self) -> Result<MetaVersionResponse> {
            self.cached(Sandbox::new()?)
                .version_stable()
                .map(MetaVersionResponse::from)
        }

        #[get("/meta/version/beta")]
        #[content_type("application/json")]
        fn meta_version_beta(&self) -> Result<MetaVersionResponse> {
            self.cached(Sandbox::new()?)
                .version_beta()
                .map(MetaVersionResponse::from)
        }

        #[get("/meta/version/nightly")]
        #[content_type("application/json")]
        fn meta_version_nightly(&self) -> Result<MetaVersionResponse> {
            self.cached(Sandbox::new()?)
                .version_nightly()
                .map(MetaVersionResponse::from)
        }
    }

    impl Gist {
        #[post("/meta/gist")]
        #[content_type("application/json")]
        fn create(
            &self,
            body: MetaGistCreateRequest,
        ) -> impl Future<Item = MetaGistResponse, Error = Error> + Send {
            gist::create_future(self.token.clone(), body.code)
                .map(|gist| MetaGistResponse::from(gist))
                .map_err(|e| unimplemented!("FIXME {:?}", e))
        }

        #[get("/meta/gist/:id")]
        #[content_type("application/json")]
        fn show(&self, id: String) -> impl Future<Item = MetaGistResponse, Error = Error> + Send {
            gist::load_future(self.token.clone(), &id)
                .map(|gist| MetaGistResponse::from(gist))
                .map_err(|e| unimplemented!("FIXME {:?}", e))
        }
    }
}

fn maybe<M>(enabled: bool, f: impl FnOnce() -> M) -> Either<M, Identity> {
    if enabled {
        Either::A(f())
    } else {
        Either::B(Identity::new())
    }
}

use self::cache::Cache;

mod cache {
    use std::time::Duration;
    use tower_web::{self, routing::{IntoResource, RouteSet, Resource, RouteMatch}, util::BufStream};
    use http::{self, header::HeaderValue, status::StatusCode, HttpTryFrom};
    use tokio::prelude::Poll;
    use futures::{Future, Async};

    #[derive(Debug, Clone)]
    pub struct Cache<R> {
        inner: R,
        cache: HeaderValue,
    }

    impl<R> Cache<R> {
        pub fn new(time: Duration, inner: R) -> Self {
            let x = format!("public, max-age={}", time.as_secs());
            let cache = HeaderValue::try_from(x).expect("nah, dawg");
            Self { inner, cache }
        }
    }

    impl<R, S, RequestBody> IntoResource<S, RequestBody> for Cache<R>
    where
        R: IntoResource<S, RequestBody>,
        S: ::tower_web::response::Serializer,
        RequestBody: BufStream,
    {
        type Destination = R::Destination;
        type Resource = CacheResource<R::Resource>;

        fn routes(&self) -> RouteSet<Self::Destination> {
            self.inner.routes()
        }

        fn into_resource(self, serializer: S) -> Self::Resource {
            let Self { inner, cache } = self;
            let inner = inner.into_resource(serializer);
            CacheResource { inner, cache }
        }
    }

    #[derive(Debug, Clone)]
    pub struct CacheResource<R> {
        inner: R,
        cache: HeaderValue,
    }

    impl<R> Resource for CacheResource<R>
    where
        R: Resource,
    {
        type Destination = R::Destination;
        type RequestBody = R::RequestBody;
        type Buf = R::Buf;
        type Body = R::Body;
        type Future = CacheFuture<R::Future>;

        fn dispatch(
            &mut self,
            destination: Self::Destination,
            route_match: &RouteMatch,
            body: Self::RequestBody
        ) -> Self::Future {
            let inner = self.inner.dispatch(destination, route_match, body);
            let cache = self.cache.clone();

            CacheFuture { inner, cache: Some(cache) }
        }
    }

    pub struct CacheFuture<F> {
        inner: F,
        cache: Option<HeaderValue>,
    }

    impl<F, B> Future for CacheFuture<F>
    where
        F: Future<Item = http::Response<B>, Error = tower_web::Error>
    {
        type Item = http::Response<B>;
        type Error = tower_web::Error;

        fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
            let mut resp = try_ready!(self.inner.poll());
            if let Some(cache) = self.cache.take() {
                let status = resp.status();
                // TESTME
                if status == StatusCode::OK || status == StatusCode::NOT_MODIFIED {
                    resp.headers_mut().insert(http::header::CACHE_CONTROL, cache);
                }
            }
            Ok(Async::Ready(resp))
        }
    }
}

pub fn run(config: Config) {
    let addr = SocketAddr::new(config.address.parse().unwrap(), config.port).into();
    info!("[Tower-Web] Starting the server on http://{}", addr);

    let cors = maybe(config.cors_enabled, || {
        CorsBuilder::new()
            .allow_origins(AllowedOrigins::Any { allow_null: true })
            .allow_headers(vec![header::CONTENT_TYPE])
            .allow_methods(vec![Method::GET, Method::POST])
            .allow_credentials(false)
            .max_age(ONE_HOUR)
            .prefer_wildcard(true)
            .build()
    });

    let logging = LogMiddleware::new("access");

    ServiceBuilder::new()
        .resource((Cache::new(ONE_DAY, Index::new(config.root.clone())), ))
        .resource(Cache::new(ONE_YEAR, Assets::new(config.root)))
        .resource(SandboxFixme)
        .resource(Meta::default())
        .resource(Gist::new(config.gh_token))
        .middleware(cors)
        .middleware(logging)
        .run(&addr).unwrap();
}
