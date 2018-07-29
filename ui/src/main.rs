#![recursion_limit="128"]
#![feature(try_from)]

#[macro_use]
extern crate log;
extern crate env_logger;
extern crate dotenv;
extern crate iron;
extern crate mount;
extern crate router;
extern crate playground_middleware;
extern crate bodyparser;
extern crate serde;
extern crate serde_json;
extern crate mktemp;
#[macro_use]
extern crate quick_error;
extern crate corsware;
#[macro_use]
extern crate lazy_static;
extern crate petgraph;
extern crate regex;
extern crate rustc_demangle;
extern crate hubcaps;
extern crate tokio;
#[macro_use]
extern crate futures;
extern crate hyper;
extern crate hyper_tls;
extern crate openssl_probe;
#[macro_use]
extern crate tower_web;
extern crate http;
extern crate mime_guess;

#[macro_use]
extern crate serde_derive;

use std::{
    convert::TryFrom,
    env,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

use sandbox::Sandbox;

mod asm_cleanup;
mod gist;
mod iron_web_server;
mod sandbox;
mod tower_web_server;

const ONE_HOUR_IN_SECONDS: u32 = 60 * 60;
const ONE_DAY_IN_SECONDS: u64 = 60 * 60 * 24;
const ONE_YEAR_IN_SECONDS: u64 = 60 * 60 * 24 * 365;

const SANDBOX_CACHE_TIME_TO_LIVE_IN_SECONDS: u64 = ONE_HOUR_IN_SECONDS as u64;

pub struct Config {
    root: PathBuf,
    gh_token: String,
    address: String,
    port: u16 ,
    logfile: String ,
    cors_enabled: bool,
    tower_web: bool,
}

impl Config {
    const DEFAULT_ADDRESS: &'static str = "127.0.0.1";
    const DEFAULT_PORT: u16 = 5000;
    const DEFAULT_LOG_FILE: &'static str = "access-log.csv";

    fn from_env() -> Self {
        let root: PathBuf = env::var_os("PLAYGROUND_UI_ROOT").expect("Must specify PLAYGROUND_UI_ROOT").into();
        let gh_token = env::var("PLAYGROUND_GITHUB_TOKEN").expect("Must specify PLAYGROUND_GITHUB_TOKEN");

        let address = env::var("PLAYGROUND_UI_ADDRESS").unwrap_or_else(|_| Self::DEFAULT_ADDRESS.to_string());
        let port = env::var("PLAYGROUND_UI_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(Self::DEFAULT_PORT);
        let logfile = env::var("PLAYGROUND_LOG_FILE").unwrap_or_else(|_| Self::DEFAULT_LOG_FILE.to_string());
        let cors_enabled = env::var_os("PLAYGROUND_CORS_ENABLED").is_some();

        let tower_web = env::var_os("PLAYGROUND_TOWER_WEB").is_some();

        Self {
            root,
            gh_token,
            address,
            port,
            logfile,
            cors_enabled,
            tower_web,
        }
    }
}

fn main() {
    // Dotenv may be unable to load environment variables, but that's ok in production
    let _ = dotenv::dotenv();
    openssl_probe::init_ssl_cert_env_vars();
    env_logger::init();

    let config = Config::from_env();
    if config.tower_web {
        tower_web_server::run(config);
    } else {
        iron_web_server::run(config);
    }
}

#[derive(Debug, Clone)]
struct SandboxCacheInfo<T> {
    value: T,
    time: Instant,
}

/// Caches the success value of a single operation
#[derive(Debug)]
struct SandboxCacheOne<T>(Mutex<Option<SandboxCacheInfo<T>>>);

impl<T> Default for SandboxCacheOne<T> {
    fn default() -> Self { SandboxCacheOne(Mutex::default()) }
}

impl<T> SandboxCacheOne<T>
where
    T: Clone
{
    fn clone_or_populate<F>(&self, populator: F) -> Result<T>
    where
        F: FnOnce() -> sandbox::Result<T>
    {
        let mut cache = self.0.lock().map_err(|_| Error::CachePoisoned)?;

        match cache.clone() {
            Some(cached) => {
                if cached.time.elapsed() > Duration::from_secs(SANDBOX_CACHE_TIME_TO_LIVE_IN_SECONDS) {
                    SandboxCacheOne::populate(&mut *cache, populator)
                } else {
                    Ok(cached.value)
                }
            },
            None => {
                SandboxCacheOne::populate(&mut *cache, populator)
            }
        }
    }

    fn populate<F>(cache: &mut Option<SandboxCacheInfo<T>>, populator: F) -> Result<T>
    where
        F: FnOnce() -> sandbox::Result<T>
    {
        let value = populator().map_err(Error::Sandbox)?;
        *cache = Some(SandboxCacheInfo {
            value: value.clone(),
            time: Instant::now(),
        });
        Ok(value)
    }
}

/// Caches the successful results of all sandbox operations that make
/// sense to cache.
#[derive(Debug, Default)]
struct SandboxCache {
    crates: SandboxCacheOne<Vec<sandbox::CrateInformation>>,
    version_stable: SandboxCacheOne<sandbox::Version>,
    version_beta: SandboxCacheOne<sandbox::Version>,
    version_nightly: SandboxCacheOne<sandbox::Version>,
}

/// Provides a similar API to the Sandbox that caches the successful results.
struct CachedSandbox<'a> {
    sandbox: Sandbox,
    cache: &'a SandboxCache,
}

impl<'a> CachedSandbox<'a> {
    fn crates(&self) -> Result<Vec<sandbox::CrateInformation>> {
        self.cache.crates.clone_or_populate(|| self.sandbox.crates())
    }

    fn version_stable(&self) -> Result<sandbox::Version> {
        self.cache.version_stable.clone_or_populate(|| {
            self.sandbox.version(sandbox::Channel::Stable)
        })
    }

    fn version_beta(&self) -> Result<sandbox::Version> {
        self.cache.version_beta.clone_or_populate(|| {
            self.sandbox.version(sandbox::Channel::Beta)
        })
    }

    fn version_nightly(&self) -> Result<sandbox::Version> {
        self.cache.version_nightly.clone_or_populate(|| {
            self.sandbox.version(sandbox::Channel::Nightly)
        })
    }
}

quick_error! {
    #[derive(Debug)]
    pub enum Error {
        Sandbox(err: sandbox::Error) {
            description("sandbox operation failed")
            display("Sandbox operation failed: {}", err)
            cause(err)
            from()
        }
        Serialization(err: serde_json::Error) {
            description("unable to serialize response")
            display("Unable to serialize response: {}", err)
            cause(err)
            from()
        }
        Deserialization(err: bodyparser::BodyError) {
            description("unable to deserialize request")
            display("Unable to deserialize request: {}", err)
            cause(err)
            from()
        }
        InvalidTarget(value: String) {
            description("an invalid target was passed")
            display("The value {:?} is not a valid target", value)
        }
        InvalidAssemblyFlavor(value: String) {
            description("an invalid assembly flavor was passed")
            display("The value {:?} is not a valid assembly flavor", value)
        }
        InvalidDemangleAssembly(value: String) {
            description("an invalid demangling option was passed")
            display("The value {:?} is not a valid demangle option", value)
        }
        InvalidProcessAssembly(value: String) {
            description("an invalid assembly processing option was passed")
            display("The value {:?} is not a valid assembly processing option", value)
        }
        InvalidChannel(value: String) {
            description("an invalid channel was passed")
            display("The value {:?} is not a valid channel", value,)
        }
        InvalidMode(value: String) {
            description("an invalid mode was passed")
            display("The value {:?} is not a valid mode", value)
        }
        InvalidEdition(value: String) {
            description("an invalid edition was passed")
            display("The value {:?} is not a valid edition", value)
        }
        InvalidCrateType(value: String) {
            description("an invalid crate type was passed")
            display("The value {:?} is not a valid crate type", value)
        }
        RequestMissing {
            description("no request was provided")
            display("No request was provided")
        }
        CachePoisoned {
            description("the cache has been poisoned")
            display("The cache has been poisoned")
        }
    }
}

type Result<T> = ::std::result::Result<T, Error>;

#[derive(Debug, Clone, Deserialize, Extract)]
struct CompileRequest {
    target: String,
    #[serde(rename = "assemblyFlavor")]
    assembly_flavor: Option<String>,
    #[serde(rename = "demangleAssembly")]
    demangle_assembly: Option<String>,
    #[serde(rename = "processAssembly")]
    process_assembly: Option<String>,
    channel: String,
    mode: String,
    #[serde(default)]
    edition: String,
    #[serde(rename = "crateType")]
    crate_type: String,
    tests: bool,
    #[serde(default)]
    backtrace: bool,
    code: String,
}

#[derive(Debug, Clone, Serialize, Response)]
struct CompileResponse {
    success: bool,
    code: String,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Clone, Deserialize, Extract)]
struct ExecuteRequest {
    channel: String,
    mode: String,
    #[serde(default)]
    edition: String,
    #[serde(rename = "crateType")]
    crate_type: String,
    tests: bool,
    #[serde(default)]
    backtrace: bool,
    code: String,
}

#[derive(Debug, Clone, Serialize, Response)]
struct ExecuteResponse {
    success: bool,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Clone, Deserialize, Extract)]
struct FormatRequest {
    code: String,
}

#[derive(Debug, Clone, Serialize, Response)]
struct FormatResponse {
    success: bool,
    code: String,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Clone, Deserialize, Extract)]
struct ClippyRequest {
    code: String,
}

#[derive(Debug, Clone, Serialize, Response)]
struct ClippyResponse {
    success: bool,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Clone, Deserialize, Extract)]
struct MiriRequest {
    code: String,
}

#[derive(Debug, Clone, Serialize, Response)]
struct MiriResponse {
    success: bool,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Clone, Serialize)]
struct CrateInformation {
    name: String,
    version: String,
    id: String,
}

#[derive(Debug, Clone, Serialize, Response)]
struct MetaCratesResponse {
    crates: Vec<CrateInformation>,
}

#[derive(Debug, Clone, Serialize, Response)]
struct MetaVersionResponse {
    version: String,
    hash: String,
    date: String,
}

#[derive(Debug, Clone, Deserialize, Extract)]
struct MetaGistCreateRequest {
    code: String,
}

#[derive(Debug, Clone, Serialize, Response)]
struct MetaGistResponse {
    id: String,
    url: String,
    code: String,
}

#[derive(Debug, Clone, Deserialize, Extract)]
struct EvaluateRequest {
    version: String,
    optimize: String,
    code: String,
}

#[derive(Debug, Clone, Serialize, Response)]
struct EvaluateResponse {
    result: String,
    error: Option<String>,
}

impl TryFrom<CompileRequest> for sandbox::CompileRequest {
    type Error = Error;

    fn try_from(me: CompileRequest) -> Result<Self> {
        let target = parse_target(&me.target)?;
        let assembly_flavor = match me.assembly_flavor {
            Some(f) => Some(parse_assembly_flavor(&f)?),
            None => None,
        };

        let demangle = match me.demangle_assembly {
            Some(f) => Some(parse_demangle_assembly(&f)?),
            None => None,
        };

        let process_assembly = match me.process_assembly {
            Some(f) => Some(parse_process_assembly(&f)?),
            None => None,
        };

        let target = match (target, assembly_flavor, demangle, process_assembly) {
            (sandbox::CompileTarget::Assembly(_, _, _), Some(flavor), Some(demangle), Some(process)) =>
                sandbox::CompileTarget::Assembly(flavor, demangle, process),
            _ => target,
        };

        Ok(sandbox::CompileRequest {
            target,
            channel: parse_channel(&me.channel)?,
            mode: parse_mode(&me.mode)?,
            edition: parse_edition(&me.edition)?,
            crate_type: parse_crate_type(&me.crate_type)?,
            tests: me.tests,
            backtrace: me.backtrace,
            code: me.code,
        })
    }
}

impl From<sandbox::CompileResponse> for CompileResponse {
    fn from(me: sandbox::CompileResponse) -> Self {
        CompileResponse {
            success: me.success,
            code: me.code,
            stdout: me.stdout,
            stderr: me.stderr,
        }
    }
}

impl TryFrom<ExecuteRequest> for sandbox::ExecuteRequest {
    type Error = Error;

    fn try_from(me: ExecuteRequest) -> Result<Self> {
        Ok(sandbox::ExecuteRequest {
            channel: try!(parse_channel(&me.channel)),
            mode: try!(parse_mode(&me.mode)),
            edition: parse_edition(&me.edition)?,
            crate_type: try!(parse_crate_type(&me.crate_type)),
            tests: me.tests,
            backtrace: me.backtrace,
            code: me.code,
        })
    }
}

impl From<sandbox::ExecuteResponse> for ExecuteResponse {
    fn from(me: sandbox::ExecuteResponse) -> Self {
        ExecuteResponse {
            success: me.success,
            stdout: me.stdout,
            stderr: me.stderr,
        }
    }
}

impl TryFrom<FormatRequest> for sandbox::FormatRequest {
    type Error = Error;

    fn try_from(me: FormatRequest) -> Result<Self> {
        Ok(sandbox::FormatRequest {
            code: me.code,
        })
    }
}

impl From<sandbox::FormatResponse> for FormatResponse {
    fn from(me: sandbox::FormatResponse) -> Self {
        FormatResponse {
            success: me.success,
            code: me.code,
            stdout: me.stdout,
            stderr: me.stderr,
        }
    }
}

impl From<ClippyRequest> for sandbox::ClippyRequest {
    fn from(me: ClippyRequest) -> Self {
        sandbox::ClippyRequest {
            code: me.code,
        }
    }
}

impl From<sandbox::ClippyResponse> for ClippyResponse {
    fn from(me: sandbox::ClippyResponse) -> Self {
        ClippyResponse {
            success: me.success,
            stdout: me.stdout,
            stderr: me.stderr,
        }
    }
}

impl From<MiriRequest> for sandbox::MiriRequest {
    fn from(me: MiriRequest) -> Self {
        sandbox::MiriRequest {
            code: me.code,
        }
    }
}

impl From<sandbox::MiriResponse> for MiriResponse {
    fn from(me: sandbox::MiriResponse) -> Self {
        MiriResponse {
            success: me.success,
            stdout: me.stdout,
            stderr: me.stderr,
        }
    }
}

impl From<Vec<sandbox::CrateInformation>> for MetaCratesResponse {
    fn from(me: Vec<sandbox::CrateInformation>) -> Self {
        let crates = me.into_iter()
            .map(|cv| CrateInformation { name: cv.name, version: cv.version, id: cv.id })
            .collect();

        MetaCratesResponse {
            crates,
        }
    }
}

impl From<sandbox::Version> for MetaVersionResponse {
    fn from(me: sandbox::Version) -> Self {
        MetaVersionResponse {
            version: me.release,
            hash: me.commit_hash,
            date: me.commit_date,
        }
    }
}

impl From<gist::Gist> for MetaGistResponse {
    fn from(me: gist::Gist) -> Self {
        MetaGistResponse {
            id: me.id,
            url: me.url,
            code: me.code,
        }
    }
}

impl TryFrom<EvaluateRequest> for sandbox::ExecuteRequest {
    type Error = Error;

    fn try_from(me: EvaluateRequest) -> Result<Self> {
        Ok(sandbox::ExecuteRequest {
            channel: parse_channel(&me.version)?,
            mode: if me.optimize != "0" { sandbox::Mode::Release } else { sandbox::Mode::Debug },
            edition: None, // FIXME: What should this be?
            crate_type: sandbox::CrateType::Binary,
            tests: false,
            backtrace: false,
            code: me.code,
        })
    }
}

impl From<sandbox::ExecuteResponse> for EvaluateResponse {
    fn from(me: sandbox::ExecuteResponse) -> Self {
        // The old playground didn't use Cargo, so it never had the
        // Cargo output ("Compiling playground...") which is printed
        // to stderr. Since this endpoint is used to inline results on
        // the page, don't include the stderr unless an error
        // occurred.
        if me.success {
            EvaluateResponse {
                result: me.stdout,
                error: None,
            }
        } else {
            // When an error occurs, *some* consumers check for an
            // `error` key, others assume that the error is crammed in
            // the `result` field and then they string search for
            // `error:` or `warning:`. Ew. We can put it in both.
            let result = me.stderr + &me.stdout;
            EvaluateResponse {
                result: result.clone(),
                error: Some(result),
            }
        }
    }
}

fn parse_target(s: &str) -> Result<sandbox::CompileTarget> {
    Ok(match s {
        "asm" => sandbox::CompileTarget::Assembly(sandbox::AssemblyFlavor::Att,
                                                  sandbox::DemangleAssembly::Demangle,
                                                  sandbox::ProcessAssembly::Filter),
        "llvm-ir" => sandbox::CompileTarget::LlvmIr,
        "mir" => sandbox::CompileTarget::Mir,
        "wasm" => sandbox::CompileTarget::Wasm,
        _ => return Err(Error::InvalidTarget(s.into()))
    })
}

fn parse_assembly_flavor(s: &str) -> Result<sandbox::AssemblyFlavor> {
    Ok(match s {
        "att" => sandbox::AssemblyFlavor::Att,
        "intel" => sandbox::AssemblyFlavor::Intel,
        _ => return Err(Error::InvalidAssemblyFlavor(s.into()))
    })
}

fn parse_demangle_assembly(s: &str) -> Result<sandbox::DemangleAssembly> {
    Ok(match s {
        "demangle" => sandbox::DemangleAssembly::Demangle,
        "mangle" => sandbox::DemangleAssembly::Mangle,
        _ => return Err(Error::InvalidDemangleAssembly(s.into()))
    })
}

fn parse_process_assembly(s: &str) -> Result<sandbox::ProcessAssembly> {
    Ok(match s {
        "filter" => sandbox::ProcessAssembly::Filter,
        "raw" => sandbox::ProcessAssembly::Raw,
        _ => return Err(Error::InvalidProcessAssembly(s.into()))
    })
}

fn parse_channel(s: &str) -> Result<sandbox::Channel> {
    Ok(match s {
        "stable" => sandbox::Channel::Stable,
        "beta" => sandbox::Channel::Beta,
        "nightly" => sandbox::Channel::Nightly,
        _ => return Err(Error::InvalidChannel(s.into()))
    })
}

fn parse_mode(s: &str) -> Result<sandbox::Mode> {
    Ok(match s {
        "debug" => sandbox::Mode::Debug,
        "release" => sandbox::Mode::Release,
        _ => return Err(Error::InvalidMode(s.into()))
    })
}

fn parse_edition(s: &str) -> Result<Option<sandbox::Edition>> {
    Ok(match s {
        "" => None,
        "2015" => Some(sandbox::Edition::Rust2015),
        "2018" => Some(sandbox::Edition::Rust2018),
        _ => return Err(Error::InvalidEdition(s.into()))
    })
}

fn parse_crate_type(s: &str) -> Result<sandbox::CrateType> {
    use sandbox::{CrateType::*, LibraryType::*};
    Ok(match s {
        "bin" => Binary,
        "lib" => Library(Lib),
        "dylib" => Library(Dylib),
        "rlib" => Library(Rlib),
        "staticlib" => Library(Staticlib),
        "cdylib" => Library(Cdylib),
        "proc-macro" => Library(ProcMacro),
        _ => return Err(Error::InvalidCrateType(s.into()))
    })
}
