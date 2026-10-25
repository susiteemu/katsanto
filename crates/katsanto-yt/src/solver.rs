//! Signature and `n` parameter solving with yt-dlp's ejs solver.
//!
//! The solver is JavaScript; it runs in an embedded QuickJS runtime on a dedicated
//! thread (QuickJS runtimes cannot move between threads). It parses YouTube's
//! `player.js` and executes the extracted functions, so the runtime gets a memory
//! limit and an execution deadline.

use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use rquickjs::{Context, Ctx, Function, Runtime, Value};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

use crate::source::{YtError, YtResult};

const LIB_JS: &str = include_str!("../assets/ejs/yt.solver.lib.js");
const CORE_JS: &str = include_str!("../assets/ejs/yt.solver.core.js");

/// astring builds its output with `+=`, which is quadratic in QuickJS (no rope
/// strings): generating the preprocessed player took >2 minutes. Its stream
/// output mode avoids that, so collect chunks in an array instead.
const GENERATE_PATCH: &str = r#"
(() => {
  const generate = lib.astring.generate;
  lib.astring.generate = (node, options) => {
    const parts = [];
    generate(node, Object.assign({}, options, { output: { write: (s) => { parts.push(s); } } }));
    return parts.join("");
  };
})();
"#;

const MEMORY_LIMIT: usize = 512 * 1024 * 1024;
const MAX_STACK: usize = 4 * 1024 * 1024;
const CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Solver source code: the vendored ejs bundle, or newer files from `<data_dir>/ejs/`.
#[derive(Clone)]
pub struct SolverScripts {
    lib: Arc<str>,
    core: Arc<str>,
}

impl SolverScripts {
    pub fn vendored() -> Self {
        Self {
            lib: LIB_JS.into(),
            core: CORE_JS.into(),
        }
    }

    /// Use `yt.solver.lib.js` / `yt.solver.core.js` from `dir` if both exist.
    pub fn from_dir_or_vendored(dir: &Path) -> Self {
        let read = |name| std::fs::read_to_string(dir.join(name)).ok();
        match (read("yt.solver.lib.js"), read("yt.solver.core.js")) {
            (Some(lib), Some(core)) => {
                tracing::info!(dir = %dir.display(), "using ejs solver override");
                Self {
                    lib: lib.into(),
                    core: core.into(),
                }
            }
            _ => Self::vendored(),
        }
    }
}

/// Solved challenges: challenge → solution.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Solutions {
    pub n: HashMap<String, String>,
    pub sig: HashMap<String, String>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Input<'a> {
    Player {
        player: &'a str,
        output_preprocessed: bool,
        requests: [Request<'a>; 2],
    },
    Preprocessed {
        preprocessed_player: &'a str,
        requests: [Request<'a>; 2],
    },
}

#[derive(Serialize)]
struct Request<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    challenges: &'a [String],
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Output {
    Result {
        responses: Vec<Response>,
        preprocessed_player: Option<String>,
    },
    Error {
        error: String,
    },
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Response {
    Result { data: HashMap<String, String> },
    Error { error: String },
}

struct Job {
    input: String,
    reply: oneshot::Sender<YtResult<String>>,
}

/// Handle to the solver thread. Cheap to clone.
#[derive(Clone)]
pub struct Solver {
    tx: mpsc::Sender<Job>,
}

impl Solver {
    pub fn spawn(scripts: SolverScripts) -> YtResult<Self> {
        let (tx, mut rx) = mpsc::channel::<Job>(64);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        thread::Builder::new()
            .name("ejs-solver".into())
            .spawn(move || {
                let engine = match Engine::new(&scripts) {
                    Ok(e) => {
                        let _ = ready_tx.send(Ok(()));
                        e
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                while let Some(job) = rx.blocking_recv() {
                    let _ = job.reply.send(engine.call(&job.input));
                }
            })
            .map_err(|e| YtError::Other(format!("spawn solver thread: {e}")))?;
        ready_rx
            .recv()
            .map_err(|_| YtError::Other("solver thread died".into()))??;
        Ok(Self { tx })
    }

    async fn run(&self, input: String) -> YtResult<Output> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Job { input, reply })
            .await
            .map_err(|_| YtError::Other("solver thread stopped".into()))?;
        let out = rx
            .await
            .map_err(|_| YtError::Other("solver thread stopped".into()))??;
        serde_json::from_str(&out).map_err(|e| YtError::Other(format!("solver output: {e}")))
    }

    /// Preprocess a raw `player.js` (slow, do once per player version) and solve
    /// the given challenges. Returns the preprocessed player for later calls.
    pub async fn solve_player(
        &self,
        player_js: &str,
        n: &[String],
        sig: &[String],
    ) -> YtResult<(Solutions, String)> {
        let input = Input::Player {
            player: player_js,
            output_preprocessed: true,
            requests: requests(n, sig),
        };
        match self.run(to_json(&input)).await? {
            Output::Result {
                responses,
                preprocessed_player,
            } => {
                let pre = preprocessed_player.ok_or_else(|| {
                    YtError::Other("solver returned no preprocessed player".into())
                })?;
                Ok((solutions(responses)?, pre))
            }
            Output::Error { error } => Err(YtError::Other(format!("solver: {error}"))),
        }
    }

    pub async fn solve(
        &self,
        preprocessed: &str,
        n: &[String],
        sig: &[String],
    ) -> YtResult<Solutions> {
        let input = Input::Preprocessed {
            preprocessed_player: preprocessed,
            requests: requests(n, sig),
        };
        match self.run(to_json(&input)).await? {
            Output::Result { responses, .. } => solutions(responses),
            Output::Error { error } => Err(YtError::Other(format!("solver: {error}"))),
        }
    }
}

fn requests<'a>(n: &'a [String], sig: &'a [String]) -> [Request<'a>; 2] {
    [
        Request {
            kind: "n",
            challenges: n,
        },
        Request {
            kind: "sig",
            challenges: sig,
        },
    ]
}

fn to_json(input: &Input<'_>) -> String {
    serde_json::to_string(input).expect("serializable")
}

fn solutions(responses: Vec<Response>) -> YtResult<Solutions> {
    let mut it = responses.into_iter();
    let mut take = |kind: &str| match it.next() {
        Some(Response::Result { data }) => Ok(data),
        Some(Response::Error { error }) => Err(YtError::Other(format!("solver ({kind}): {error}"))),
        None => Err(YtError::Other(format!(
            "solver returned no {kind} response"
        ))),
    };
    Ok(Solutions {
        n: take("n")?,
        sig: take("sig")?,
    })
}

struct Engine {
    _rt: Runtime,
    ctx: Context,
    deadline: Arc<AtomicU64>,
    started: Instant,
}

impl Engine {
    fn new(scripts: &SolverScripts) -> YtResult<Self> {
        let js_err = |e: rquickjs::Error| YtError::Other(format!("solver init: {e}"));
        let rt = Runtime::new().map_err(js_err)?;
        rt.set_memory_limit(MEMORY_LIMIT);
        rt.set_max_stack_size(MAX_STACK);

        // Interrupt long-running calls: the handler is polled by QuickJS while executing.
        let started = Instant::now();
        let deadline = Arc::new(AtomicU64::new(u64::MAX));
        let d = deadline.clone();
        rt.set_interrupt_handler(Some(Box::new(move || {
            started.elapsed().as_millis() as u64 > d.load(Ordering::Relaxed)
        })));

        let ctx = Context::full(&rt).map_err(js_err)?;
        ctx.with(|ctx| -> YtResult<()> {
            let glue = format!(
                "{}\n{GENERATE_PATCH}\nObject.assign(globalThis, lib);\n{}\n",
                scripts.lib, scripts.core
            );
            ctx.eval::<(), _>(glue).map_err(|e| js_error(&ctx, e))
        })?;
        Ok(Self {
            _rt: rt,
            ctx,
            deadline,
            started,
        })
    }

    fn call(&self, input: &str) -> YtResult<String> {
        let limit = self.started.elapsed() + CALL_TIMEOUT;
        self.deadline
            .store(limit.as_millis() as u64, Ordering::Relaxed);
        let res = self.ctx.with(|ctx| -> YtResult<String> {
            let jsc: Function = ctx.globals().get("jsc").map_err(|e| js_error(&ctx, e))?;
            let arg = ctx.json_parse(input).map_err(|e| js_error(&ctx, e))?;
            let out: Value = jsc.call((arg,)).map_err(|e| js_error(&ctx, e))?;
            let json = ctx
                .json_stringify(out)
                .map_err(|e| js_error(&ctx, e))?
                .ok_or_else(|| YtError::Other("solver returned undefined".into()))?;
            json.to_string().map_err(|e| js_error(&ctx, e))
        });
        self.deadline.store(u64::MAX, Ordering::Relaxed);
        res
    }
}

fn js_error(ctx: &Ctx<'_>, e: rquickjs::Error) -> YtError {
    if let rquickjs::Error::Exception = e {
        let exc = ctx.catch();
        let msg = exc
            .as_exception()
            .map(|x| x.message().unwrap_or_default())
            .unwrap_or_else(|| format!("{exc:?}"));
        return YtError::Other(format!("solver exception: {msg}"));
    }
    YtError::Other(format!("solver: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn loads_vendored_solver() {
        let solver = Solver::spawn(SolverScripts::vendored()).unwrap();
        // Not a real player: the solver must report extraction failure, not crash.
        let res = solver
            .solve_player("var x = 1;", &["abc".into()], &[])
            .await;
        assert!(res.is_err(), "{res:?}");
    }
}
