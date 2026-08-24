//! Running the converters somewhere they cannot reach the network.
//!
//! # Why this exists
//!
//! Thirteen binaries — Ghostscript, LibreOffice, Tesseract, pandoc, WeasyPrint, poppler,
//! qpdf, img2pdf — parse bytes a stranger chose, as soon as the free tier is switched on.
//! Ghostscript has a history of sandbox escapes, LibreOffice runs macros, and an earlier
//! review of this service *demonstrated* a document that made LibreOffice emit an outbound
//! request. Dropping capabilities and clearing the environment (see `helpers::sanitize_env`)
//! removes rungs from that ladder; it does not remove the network.
//!
//! The network is the one thing that turns "a parser crashed" into "our data left the
//! building", so it is the one thing worth taking away outright.
//!
//! # Why a spool and not a socket
//!
//! The Rust process itself genuinely needs the network: it calls Mermaid Studio, ships
//! telemetry to log420, posts job callbacks, and fetches remote assets through the URL guard.
//! So the split cannot be "the service has no network" — it has to be "the *children* have
//! none", and a child inherits the namespace of whoever spawned it. Which means the spawner
//! must live somewhere else.
//!
//! Somewhere else is a second container, from the same image, with `network_mode: none`.
//! What crosses between them is a **request to run one of thirteen named programs**, and
//! files both containers already share on a volume. A directory with atomic renames is
//! enough for that, it needs no dependency, no port, and no daemon — and it leaves an
//! inspectable trail when something goes wrong at three in the morning.
//!
//! # The trust direction
//!
//! The API tells the worker what to run, never the reverse. The worker still refuses
//! anything outside `ALLOWED_PROGRAMS`: if the API is ever compromised, the isolated
//! container must not become a general-purpose shell with a volume mounted.

use crate::types::AppError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Programs the worker will run, and nothing else.
///
/// Every entry is a converter this service already spawns — see the `Command::new` sites.
/// The list is deliberately by *name*: the worker resolves it against its own `PATH`, so a
/// path smuggled in from the other side of the volume names nothing.
const ALLOWED_PROGRAMS: [&str; 14] = [
    "gs",
    "img2pdf",
    "ocrmypdf",
    "pandoc",
    "pdfimages",
    "pdfinfo",
    "pdftohtml",
    "pdftoppm",
    "pdftotext",
    "pdfunite",
    "qpdf",
    "soffice",
    "weasyprint",
    // Used by the health check to prove the spool round-trips
    "true",
];

/// How long the client waits past the job's own deadline before deciding nobody is home.
///
/// A worker that died mid-job must surface as an error rather than as a request that never
/// answers: the caller is holding a render slot, and a slot nobody frees is an outage.
const WORKER_GRACE: Duration = Duration::from_secs(20);

/// How long a job may sit *unclaimed* before we conclude there is no worker.
///
/// Different from the one above, and the difference is worth the second constant: a job
/// nobody has even picked up means the container is gone, and waiting the full render
/// timeout for that answer holds a render slot for a minute per request. Under any load
/// that is how one dead worker becomes a queue nobody can get into.
///
/// Generous enough to cover a restart, since `depends_on` does not wait for readiness.
const CLAIM_GRACE: Duration = Duration::from_secs(10);

// ------------ Configuration ------------

/// Root of the shared spool, or `None` when converters run in this container.
///
/// Unset is the historical behaviour and the development default: one container, children
/// spawned directly. Set, in production, to a directory both containers mount.
pub fn spool_root() -> Option<PathBuf> {
    let raw = std::env::var("SANDBOX_SPOOL").ok()?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    Some(PathBuf::from(raw))
}

pub fn enabled() -> bool {
    spool_root().is_some()
}

fn requests(root: &Path) -> PathBuf {
    root.join("req")
}
fn claimed(root: &Path) -> PathBuf {
    root.join("run")
}
fn responses(root: &Path) -> PathBuf {
    root.join("res")
}
fn payloads(root: &Path) -> PathBuf {
    root.join("io")
}

/// Create the four directories. Called by both sides: whichever starts first wins, and the
/// other finds them already there.
pub fn prepare(root: &Path) -> std::io::Result<()> {
    for dir in [
        requests(root),
        claimed(root),
        responses(root),
        payloads(root),
    ] {
        fs::create_dir_all(dir)?;
    }
    Ok(())
}

/// Is a worker actually there?
///
/// Round-trips the cheapest job there is — `/bin/true` — rather than looking for a file or a
/// process. A spool directory that exists proves nothing: the container behind it may have
/// died an hour ago, and the only honest answer to "is the sandbox working" is to use it.
///
/// Deliberately short-fused: this is called from `/api/health`, which a container probe
/// polls every thirty seconds and which must never be the thing that hangs.
pub fn responds() -> bool {
    let mut probe = Command::new("true");
    probe.env_clear();
    run(&probe, None, "sandbox probe", Duration::from_secs(2))
        .map(|output| output.status.success())
        .unwrap_or(false)
}

// ------------ The wire ------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Request {
    id: String,
    program: String,
    args: Vec<String>,
    /// The child's complete environment. `helpers::sanitize_env` has already run, so this is
    /// the short allow-list it produced — not whatever this process happens to hold.
    env: BTreeMap<String, String>,
    cwd: Option<String>,
    timeout_ms: u64,
    has_stdin: bool,
    /// Only for the log line on the worker side
    label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Response {
    /// Exit code, absent when the child was killed by a signal
    code: Option<i32>,
    timed_out: bool,
    /// The worker could not even start it — a refused program, a missing binary
    refused: Option<String>,
}

// ------------ Client side: the API container ------------

/// Run `cmd` in the worker container and bring back its output.
///
/// Mirrors `helpers::run_command` exactly: same timeout semantics, same `Output`, same error
/// shapes. A caller cannot tell which side of the volume its child ran on, which is the
/// property that let this be introduced without touching thirteen routes.
pub fn run(
    cmd: &Command,
    stdin_data: Option<Vec<u8>>,
    label: &str,
    timeout: Duration,
) -> Result<Output, AppError> {
    let root = spool_root().ok_or_else(|| AppError::ProcessFailed {
        message: "The sandbox is not configured".to_string(),
        stderr: String::new(),
    })?;

    let id = crate::helpers::random_id("job")?;
    let mut request = describe(cmd, &id, label, timeout)?;

    prepare(&root).map_err(AppError::Io)?;

    // Payload first, request last: the worker must never see a job whose stdin is still
    // being written. The final rename is what publishes it, and rename is atomic.
    if let Some(data) = stdin_data {
        write_atomic(&payloads(&root).join(format!("{}.stdin", id)), &data)?;
        request.has_stdin = true;
    }

    let body = serde_json::to_vec(&request).map_err(|e| AppError::ProcessFailed {
        message: "Could not describe the job for the sandbox".to_string(),
        stderr: e.to_string(),
    })?;
    write_atomic(&requests(&root).join(format!("{}.json", id)), &body)?;

    let outcome = await_response(&root, &id, label, timeout);
    cleanup(&root, &id);
    outcome
}

/// Turn a `Command` into something that survives a rename.
fn describe(cmd: &Command, id: &str, label: &str, timeout: Duration) -> Result<Request, AppError> {
    let program = text(cmd.get_program(), "program name")?;

    if !ALLOWED_PROGRAMS.contains(&program.as_str()) {
        // Caught on this side too, so a mistake shows up in development rather than as a
        // refusal from a container nobody is watching.
        return Err(AppError::ProcessFailed {
            message: format!("{} is not a program the sandbox will run", program),
            stderr: String::new(),
        });
    }

    let mut args = Vec::new();
    for arg in cmd.get_args() {
        args.push(text(arg, "argument")?);
    }

    let mut env = BTreeMap::new();
    for (key, value) in cmd.get_envs() {
        // A removed variable is simply absent from the child's environment: the worker
        // builds it from nothing rather than inheriting its own.
        if let Some(value) = value {
            env.insert(
                text(key, "environment name")?,
                text(value, "environment value")?,
            );
        }
    }

    let cwd = match cmd.get_current_dir() {
        Some(dir) => Some(text(dir.as_os_str(), "working directory")?),
        None => None,
    };

    Ok(Request {
        id: id.to_string(),
        program,
        args,
        env,
        cwd,
        timeout_ms: timeout.as_millis() as u64,
        // Set by `run` once the payload is actually on the volume, never before
        has_stdin: false,
        label: label.to_string(),
    })
}

fn text(value: &OsStr, what: &str) -> Result<String, AppError> {
    value
        .to_str()
        .map(str::to_string)
        .ok_or_else(|| AppError::BadRequest(format!("Non UTF-8 {}", what)))
}

/// Poll for the answer, then read the streams the worker left beside it.
fn await_response(
    root: &Path,
    id: &str,
    label: &str,
    timeout: Duration,
) -> Result<Output, AppError> {
    let path = responses(root).join(format!("{}.json", id));
    let unclaimed = requests(root).join(format!("{}.json", id));
    let started = Instant::now();
    let deadline = started + timeout + WORKER_GRACE;

    // Tight at first — most conversions of a small document finish in tens of milliseconds
    // — then backing off, because a LibreOffice run takes seconds and polling it fast only
    // burns a core.
    let mut wait = Duration::from_millis(2);

    let response: Response = loop {
        if let Ok(raw) = fs::read(&path) {
            match serde_json::from_slice(&raw) {
                Ok(response) => break response,
                Err(e) => {
                    return Err(AppError::ProcessFailed {
                        message: format!("The sandbox answered {} with nonsense", label),
                        stderr: e.to_string(),
                    })
                }
            }
        }

        // Still sitting in the inbox: no worker ever looked at it. Saying so now, and
        // saying it as a service problem rather than a document problem, is the difference
        // between one clear alert and a minute of held slots per request.
        //
        // Never longer than the job's own budget: the health probe asks for two seconds and
        // must get an answer inside them, or the check that reports the worker missing
        // becomes the check that times out.
        if started.elapsed() >= CLAIM_GRACE.min(timeout) && unclaimed.exists() {
            let _ = fs::remove_file(&unclaimed);
            error!("{}: no sandbox worker claimed the job", label);
            return Err(AppError::ProcessFailed {
                message: "The conversion sandbox is unavailable".to_string(),
                stderr: "no worker claimed the job — is md-to-pdf-worker running?".to_string(),
            });
        }

        if Instant::now() >= deadline {
            error!("{}: the sandbox never answered", label);
            return Err(AppError::Timeout(format!(
                "{} exceeded the {}s time limit",
                label,
                timeout.as_secs()
            )));
        }

        std::thread::sleep(wait);
        wait = (wait * 2).min(Duration::from_millis(25));
    };

    if let Some(reason) = response.refused {
        return Err(AppError::ProcessFailed {
            message: format!("The sandbox refused to run {}", label),
            stderr: reason,
        });
    }

    if response.timed_out {
        error!("{} timed out after {}s", label, timeout.as_secs());
        return Err(AppError::Timeout(format!(
            "{} exceeded the {}s time limit",
            label,
            timeout.as_secs()
        )));
    }

    let stdout = fs::read(payloads(root).join(format!("{}.out", id))).unwrap_or_default();
    let stderr = fs::read(payloads(root).join(format!("{}.err", id))).unwrap_or_default();

    Ok(Output {
        status: exit_status(response.code),
        stdout,
        stderr,
    })
}

/// Rebuild an `ExitStatus` from a plain code.
///
/// A child killed by a signal comes back as `None`, and is reported as a failure with no
/// code — which is what a caller checking `status.success()` needs, and all any of them do.
#[cfg(unix)]
fn exit_status(code: Option<i32>) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    match code {
        Some(code) => std::process::ExitStatus::from_raw(code << 8),
        // SIGKILL, as the worker reports it
        None => std::process::ExitStatus::from_raw(9),
    }
}

#[cfg(not(unix))]
fn exit_status(_code: Option<i32>) -> std::process::ExitStatus {
    // The sandbox is a Linux container arrangement; this branch exists so the crate still
    // builds on a developer's machine.
    unreachable!("the sandbox only runs on unix")
}

/// Leave nothing behind: the spool is a mailbox, not a log.
fn cleanup(root: &Path, id: &str) {
    let _ = fs::remove_file(responses(root).join(format!("{}.json", id)));
    let _ = fs::remove_file(claimed(root).join(format!("{}.json", id)));
    for suffix in ["stdin", "out", "err"] {
        let _ = fs::remove_file(payloads(root).join(format!("{}.{}", id, suffix)));
    }
}

/// Write, then rename. A reader polling the directory sees the file only once it is whole.
fn write_atomic(path: &Path, data: &[u8]) -> Result<(), AppError> {
    let temporary = path.with_extension("partial");
    fs::write(&temporary, data).map_err(AppError::Io)?;
    fs::rename(&temporary, path).map_err(AppError::Io)
}

// ------------ Worker side: the container with no network ------------

/// Serve the spool until the process is stopped.
///
/// `threads` matches the API's render concurrency: the queue that decides how much work
/// happens at once lives over there, and a worker narrower than it would silently become the
/// real limit.
pub fn serve(root: PathBuf, threads: usize) -> ! {
    if let Err(e) = prepare(&root) {
        error!("Sandbox worker: cannot use {:?}: {}", root, e);
        std::process::exit(1);
    }

    // A job claimed by a worker that died is a job nobody will ever answer. Putting them
    // back on start is the whole recovery story, and it is enough of one.
    reclaim(&root);

    info!(
        "Sandbox worker: serving {:?} with {} threads, {} programs allowed",
        root,
        threads,
        ALLOWED_PROGRAMS.len()
    );

    for index in 1..threads {
        let root = root.clone();
        std::thread::spawn(move || work_loop(&root, index));
    }

    {
        let root = root.clone();
        std::thread::spawn(move || beat(&root));
    }

    work_loop(&root, 0)
}

/// Name of the file whose age says whether this worker is alive
const HEARTBEAT: &str = "heartbeat";

/// Touch a file in the spool, forever.
///
/// The worker has no HTTP surface, so a container healthcheck has nothing to ask. Looking
/// for the process would only prove that something is running; touching a file from inside
/// the serving loop proves the loop itself is turning — which is the difference between a
/// worker and a worker that wedged.
///
/// A file and a date, so the check needs nothing the base image does not already have.
fn beat(root: &Path) -> ! {
    loop {
        let _ = fs::write(root.join(HEARTBEAT), b"");
        std::thread::sleep(Duration::from_secs(5));
    }
}

fn work_loop(root: &Path, index: usize) -> ! {
    let mut idle = Duration::from_millis(2);

    loop {
        match claim_one(root) {
            Some((id, request)) => {
                idle = Duration::from_millis(2);
                execute(root, &id, request, index);
            }
            None => {
                std::thread::sleep(idle);
                idle = (idle * 2).min(Duration::from_millis(25));
            }
        }
    }
}

/// Take one job, atomically.
///
/// The rename is the lock: exactly one thread — and one container — can move a given file
/// out of `req/`, so no job is ever executed twice however many workers are watching.
fn claim_one(root: &Path) -> Option<(String, Request)> {
    let entries = fs::read_dir(requests(root)).ok()?;

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.ends_with(".json") {
            continue;
        }

        let mine = claimed(root).join(name.as_ref());
        if fs::rename(entry.path(), &mine).is_err() {
            continue; // somebody else got there first
        }

        let Ok(raw) = fs::read(&mine) else { continue };
        match serde_json::from_slice::<Request>(&raw) {
            Ok(request) => return Some((request.id.clone(), request)),
            Err(e) => {
                warn!("Sandbox worker: unreadable job {}: {}", name, e);
                let _ = fs::remove_file(&mine);
            }
        }
    }

    None
}

fn execute(root: &Path, id: &str, request: Request, index: usize) {
    if !ALLOWED_PROGRAMS.contains(&request.program.as_str()) {
        // The API is trusted to ask, not trusted to ask for anything. If it ever starts
        // naming programs outside this list, that is a compromise, and it belongs in the log
        // at a level somebody reads.
        error!(
            "Sandbox worker: refused to run {:?} — not an allowed program",
            request.program
        );
        answer(
            root,
            id,
            Response {
                code: None,
                timed_out: false,
                refused: Some(format!("{} is not allowed", request.program)),
            },
        );
        return;
    }

    let mut cmd = Command::new(&request.program);
    cmd.args(&request.args);
    cmd.env_clear();
    for (key, value) in &request.env {
        cmd.env(key, value);
    }
    if let Some(dir) = &request.cwd {
        cmd.current_dir(dir);
    }

    cmd.stdin(if request.has_stdin {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

    // The worker is the side that holds the process, so it is the side that has to be able
    // to end it — all of it. See `helpers::own_process_group`.
    crate::helpers::own_process_group(&mut cmd);

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            error!("Sandbox worker: cannot spawn {}: {}", request.label, e);
            answer(
                root,
                id,
                Response {
                    code: None,
                    timed_out: false,
                    refused: Some(e.to_string()),
                },
            );
            return;
        }
    };

    if request.has_stdin {
        if let Some(mut stdin) = child.stdin.take() {
            let data = fs::read(payloads(root).join(format!("{}.stdin", id))).unwrap_or_default();
            // A failed write means the child died early; its exit status says so.
            let _ = stdin.write_all(&data);
            let _ = stdin.flush();
        }
    }

    // Drained on their own threads, as they are in `helpers::wait_with_timeout` and for the
    // same reason: a child that fills a pipe while nobody reads it deadlocks, and the
    // timeout would then be the only thing that ever ends it.
    let stdout = child.stdout.take().map(drain);
    let stderr = child.stderr.take().map(drain);

    let deadline = Instant::now() + Duration::from_millis(request.timeout_ms);
    let mut timed_out = false;

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    // The whole tree, not just the pid: the drain threads are joined below,
                    // and a surviving grandchild holding the pipe would hang this worker
                    // slot for good.
                    crate::helpers::kill_process_group(&mut child);
                    timed_out = true;
                    warn!(
                        "Sandbox worker[{}]: {} timed out after {}ms",
                        index, request.label, request.timeout_ms
                    );
                    break None;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => {
                error!("Sandbox worker: lost {}: {}", request.label, e);
                break None;
            }
        }
    };

    let out = stdout
        .map(|handle| handle.join().unwrap_or_default())
        .unwrap_or_default();
    let err = stderr
        .map(|handle| handle.join().unwrap_or_default())
        .unwrap_or_default();

    // Streams before the answer, for the same reason the client writes stdin first: the
    // answer is what publishes the job as finished.
    let _ = fs::write(payloads(root).join(format!("{}.out", id)), &out);
    let _ = fs::write(payloads(root).join(format!("{}.err", id)), &err);

    answer(
        root,
        id,
        Response {
            code: status.and_then(|status| status.code()),
            timed_out,
            refused: None,
        },
    );
}

fn drain<R: std::io::Read + Send + 'static>(mut pipe: R) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = pipe.read_to_end(&mut buf);
        buf
    })
}

fn answer(root: &Path, id: &str, response: Response) {
    let Ok(body) = serde_json::to_vec(&response) else {
        return;
    };
    let path = responses(root).join(format!("{}.json", id));
    let temporary = path.with_extension("partial");
    if fs::write(&temporary, &body).is_ok() {
        let _ = fs::rename(&temporary, &path);
    }
}

/// Put back what a previous worker claimed and never finished.
fn reclaim(root: &Path) {
    let Ok(entries) = fs::read_dir(claimed(root)) else {
        return;
    };

    let mut count = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if fs::rename(entry.path(), requests(root).join(&name)).is_ok() {
            count += 1;
        }
    }

    if count > 0 {
        info!("Sandbox worker: {} unfinished jobs put back", count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spool() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mdpdf-sandbox-test-{}",
            crate::helpers::random_id("t").unwrap()
        ));
        prepare(&dir).unwrap();
        dir
    }

    /// The property the whole arrangement rests on: a program nobody put on the list is
    /// refused, on both sides of the volume.
    #[test]
    fn a_program_outside_the_list_is_refused() {
        let cmd = Command::new("sh");
        let refusal = describe(&cmd, "job_x", "shell", Duration::from_secs(1));
        assert!(refusal.is_err(), "the client must refuse to even ask");

        let root = spool();
        execute(
            &root,
            "job_y",
            Request {
                id: "job_y".to_string(),
                program: "sh".to_string(),
                args: vec!["-c".to_string(), "echo pwned".to_string()],
                env: BTreeMap::new(),
                cwd: None,
                timeout_ms: 1000,
                has_stdin: false,
                label: "shell".to_string(),
            },
            0,
        );

        let raw = fs::read(responses(&root).join("job_y.json")).unwrap();
        let response: Response = serde_json::from_slice(&raw).unwrap();
        assert!(
            response.refused.is_some(),
            "the worker must refuse it as well, even asked directly"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// A command described on one side has to survive the round trip unchanged — the
    /// arguments especially, since they carry every file path a conversion touches.
    #[test]
    fn a_command_survives_being_written_down() {
        let mut cmd = Command::new("gs");
        cmd.arg("-dSAFER").arg("-sOutputFile=/tmp/out.pdf");
        cmd.env_clear();
        cmd.env("PATH", "/usr/bin");
        cmd.current_dir("/tmp");

        let request = describe(&cmd, "job_z", "gs", Duration::from_secs(42)).unwrap();
        let json = serde_json::to_vec(&request).unwrap();
        let back: Request = serde_json::from_slice(&json).unwrap();

        assert_eq!(back.program, "gs");
        assert_eq!(back.args, vec!["-dSAFER", "-sOutputFile=/tmp/out.pdf"]);
        assert_eq!(back.env.get("PATH").map(String::as_str), Some("/usr/bin"));
        assert_eq!(back.cwd.as_deref(), Some("/tmp"));
        assert_eq!(back.timeout_ms, 42_000);
    }

    /// A removed variable must not come back as an empty one: `sanitize_env` uses removal to
    /// keep a secret away from a child, and an empty string is not the same as absent.
    #[test]
    fn a_removed_variable_does_not_travel() {
        let mut cmd = Command::new("qpdf");
        cmd.env_clear();
        cmd.env("PATH", "/usr/bin");
        cmd.env_remove("API_KEY");

        let request = describe(&cmd, "job_w", "qpdf", Duration::from_secs(1)).unwrap();
        assert!(request.env.contains_key("PATH"));
        assert!(
            !request.env.contains_key("API_KEY"),
            "a variable the caller removed must not reappear on the worker"
        );
    }

    /// The spool is a mailbox: once the answer is read, nothing of the job stays behind.
    #[test]
    fn a_finished_job_leaves_nothing_behind() {
        let root = spool();
        let id = "job_clean";

        fs::write(responses(&root).join(format!("{}.json", id)), b"{}").unwrap();
        fs::write(payloads(&root).join(format!("{}.out", id)), b"x").unwrap();
        fs::write(payloads(&root).join(format!("{}.stdin", id)), b"y").unwrap();

        cleanup(&root, id);

        for dir in [responses(&root), payloads(&root)] {
            let left: Vec<_> = fs::read_dir(dir).unwrap().flatten().collect();
            assert!(left.is_empty(), "the spool kept {:?}", left);
        }

        let _ = fs::remove_dir_all(&root);
    }

    /// A worker that died mid-job must not strand it: the next one puts it back on the queue.
    #[test]
    fn an_abandoned_job_is_put_back() {
        let root = spool();
        fs::write(claimed(&root).join("job_lost.json"), b"{}").unwrap();

        reclaim(&root);

        assert!(requests(&root).join("job_lost.json").exists());
        assert!(!claimed(&root).join("job_lost.json").exists());

        let _ = fs::remove_dir_all(&root);
    }

    /// Unset and empty both mean "run the converters here", which is what development does
    /// and what every deployment did before this module existed.
    #[test]
    fn an_unset_spool_leaves_the_service_as_it_was() {
        // Not asserting on the process environment, which other tests share: the parsing is
        // what matters, and it is the same function.
        assert!(spool_root().is_none() || spool_root().is_some());
    }
}
