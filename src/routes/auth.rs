//! Signing up, signing in, and minting your own keys.
//!
//! Two credentials coexist on this service and they never mix: a **session cookie** says
//! "a human is looking at a page", an **`X-API-Key` header** says "a program is calling".
//! Keeping them apart is what lets the cookie be `SameSite=Lax` — enough to stop a
//! cross-site form from acting in someone's name — while the header stays usable from
//! anywhere, which is the whole point of an API.

use crate::accounts::{self, Account, ApiKeyRecord};
use crate::exec;
use crate::types::AppError;
use rocket::http::{Cookie, CookieJar, SameSite, Status};
use rocket::request::{FromRequest, Outcome, Request};
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};

/// Name of the session cookie. Prefixed so it cannot be confused with anything the
/// integrator console stores.
pub const SESSION_COOKIE: &str = "mdpdf_session";

// ------------ The guard ------------

/// A signed-in human. Fails with 401 rather than redirecting: these are API routes, and a
/// redirect to a login page is a page's job, not a JSON endpoint's.
pub struct Member(pub Account);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Member {
    type Error = AppError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match session_account(req.cookies()) {
            Some(account) => Outcome::Success(Member(account)),
            None => Outcome::Error((
                Status::Unauthorized,
                AppError::Unauthorized("Sign in to continue".to_string()),
            )),
        }
    }
}

/// Resolve the cookie to an account, for the guard and for the pages that render a header
/// differently when someone is signed in.
pub fn session_account(jar: &CookieJar<'_>) -> Option<Account> {
    let token = jar.get(SESSION_COOKIE)?.value();
    accounts::account_for_session(token)
}

fn set_session(jar: &CookieJar<'_>, token: String, secure: bool) {
    let cookie = Cookie::build((SESSION_COOKIE, token))
        // The browser must never hand this to a script: an XSS then costs a session
        // instead of costing everything.
        .http_only(true)
        // `Lax` lets someone follow a link into their account and still be signed in, while
        // refusing to travel on a cross-site POST — which is the attack this replaces a CSRF
        // token for.
        .same_site(SameSite::Lax)
        .secure(secure)
        .path("/")
        // Without an age this is a *session cookie*: the browser drops it on close, and the
        // thirty-day session the server just opened is worth one browsing session. Staying
        // signed in is the entire reason a session exists.
        .max_age(rocket::time::Duration::days(accounts::SESSION_MAX_AGE_DAYS))
        .build();

    jar.add(cookie);
}

/// Permission to *attempt* a sign-in or a sign-up.
///
/// A request guard rather than a check inside each handler, so it runs before the body is
/// even read: the point is to refuse cheaply, ahead of the six hundred thousand PBKDF2
/// rounds that follow. It answers 429, never 401 — "too fast" and "wrong password" are
/// different facts, and conflating them would tell an attacker their guess was plausible.
pub struct AuthAttempt;

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AuthAttempt {
    type Error = AppError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let address = req
            .client_ip()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        if crate::auth::within_auth_quota(&address) {
            Outcome::Success(AuthAttempt)
        } else {
            Outcome::Error((
                Status::TooManyRequests,
                AppError::TooManyRequests(
                    "Too many sign-in attempts: wait a minute and try again".to_string(),
                ),
            ))
        }
    }
}

// ------------ Sign-up and sign-in ------------

#[derive(Deserialize)]
pub struct Credentials {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Serialize)]
pub struct SessionResponse {
    pub account: Account,
}

#[post("/auth/signup", format = "json", data = "<body>")]
pub async fn signup(
    jar: &CookieJar<'_>,
    body: Json<Credentials>,
    secure: Https,
    _attempt: AuthAttempt,
) -> Result<(Status, Json<SessionResponse>), AppError> {
    let Credentials {
        email,
        password,
        name,
    } = body.into_inner();

    // Hashing a password is deliberately slow — six hundred thousand rounds — so it belongs
    // on a blocking thread. Not the *render* queue, though: see `exec::offload_cpu`.
    let account = exec::offload_cpu(move || accounts::create(&email, &password, name)).await?;
    let token = accounts::open_session(&account.id)?;
    set_session(jar, token, secure.0);

    Ok((Status::Created, Json(SessionResponse { account })))
}

#[post("/auth/login", format = "json", data = "<body>")]
pub async fn login(
    jar: &CookieJar<'_>,
    body: Json<Credentials>,
    secure: Https,
    _attempt: AuthAttempt,
) -> Result<Json<SessionResponse>, AppError> {
    let Credentials {
        email, password, ..
    } = body.into_inner();

    let account = exec::offload_cpu(move || accounts::authenticate(&email, &password)).await?;
    let token = accounts::open_session(&account.id)?;
    set_session(jar, token, secure.0);

    Ok(Json(SessionResponse { account }))
}

#[post("/auth/logout")]
pub fn logout(jar: &CookieJar<'_>) -> Status {
    if let Some(cookie) = jar.get(SESSION_COOKIE) {
        accounts::close_session(cookie.value());
    }
    jar.remove(Cookie::from(SESSION_COOKIE));
    Status::NoContent
}

#[get("/auth/me")]
pub fn me(member: Member) -> Json<SessionResponse> {
    Json(SessionResponse { account: member.0 })
}

// ------------ Keys ------------

#[derive(Deserialize)]
pub struct NewKey {
    pub name: String,
}

/// The secret travels **once**, here, and is never stored in the clear — so it can never be
/// shown again. Saying so in the response is part of the contract, not a courtesy.
#[derive(Serialize)]
pub struct NewKeyResponse {
    pub key: ApiKeyRecord,
    pub secret: String,
    pub notice: &'static str,
}

#[derive(Serialize)]
pub struct KeyList {
    pub keys: Vec<ApiKeyRecord>,
}

#[post("/keys", format = "json", data = "<body>")]
pub fn create_key(
    member: Member,
    body: Json<NewKey>,
) -> Result<(Status, Json<NewKeyResponse>), AppError> {
    let (key, secret) = accounts::create_key(&member.0.id, &body.name)?;

    Ok((
        Status::Created,
        Json(NewKeyResponse {
            key,
            secret,
            notice: "Copy this key now: it is stored hashed and cannot be shown again.",
        }),
    ))
}

#[get("/keys")]
pub fn list_keys(member: Member) -> Result<Json<KeyList>, AppError> {
    Ok(Json(KeyList {
        keys: accounts::list_keys(&member.0.id)?,
    }))
}

#[delete("/keys/<id>")]
pub fn revoke_key(member: Member, id: &str) -> Result<Status, AppError> {
    accounts::revoke_key(&member.0.id, id)?;
    Ok(Status::NoContent)
}

// ------------ Usage ------------

#[derive(Serialize)]
pub struct Usage {
    pub plan: String,
    pub keys: usize,
    /// What the plan allows, so a 429 is never the first time someone hears about a limit
    pub limits: Limits,
}

#[derive(Serialize)]
pub struct Limits {
    pub max_file_mb: u64,
    pub retention_hours: u64,
    pub requests_per_minute: String,
}

#[get("/usage")]
pub fn usage(member: Member) -> Result<Json<Usage>, AppError> {
    let config = crate::config::config();

    Ok(Json(Usage {
        plan: member.0.plan.clone(),
        keys: accounts::list_keys(&member.0.id)?.len(),
        limits: Limits {
            max_file_mb: config.asset_max_mb,
            // What *this member* gets, not what the anonymous tier gets: an account page
            // that quotes the visitor's limits back at a member is worse than none.
            retention_hours: config.asset_ttl_member.as_secs() / 3600,
            requests_per_minute: match std::env::var("API_QUOTA_PER_MINUTE").ok().as_deref() {
                None | Some("0") | Some("") => "unlimited".to_string(),
                Some(value) => value.to_string(),
            },
        },
    }))
}

// ------------ The quality record ------------

#[derive(Serialize)]
pub struct HistoryResponse {
    pub entries: Vec<crate::history::Entry>,
}

/// What this account did, newest first, with the verdict of each operation.
///
/// This is the endpoint the workspace is built on, and the one no competitor can offer:
/// their history is a list of files, ours is a list of what each operation cost.
#[get("/history")]
pub fn history(member: Member) -> Json<HistoryResponse> {
    Json(HistoryResponse {
        entries: crate::history::list(&member.0.id, 100),
    })
}

#[delete("/history")]
pub fn clear_history(member: Member) -> Status {
    crate::history::clear(&member.0.id);
    Status::NoContent
}

// ------------ Is this connection secure? ------------

/// Whether the session cookie may carry the `Secure` flag.
///
/// Behind the production proxy the service speaks plain HTTP, so the scheme it sees is
/// never the scheme the browser used: `X-Forwarded-Proto` is the only honest signal, and
/// `deploy/` sets it. In local development there is no proxy and no TLS, and a `Secure`
/// cookie would simply never come back — which is why this is detected rather than
/// hardcoded.
pub struct Https(pub bool);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Https {
    type Error = std::convert::Infallible;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let forwarded = req
            .headers()
            .get_one("X-Forwarded-Proto")
            .is_some_and(|proto| proto.eq_ignore_ascii_case("https"));

        Outcome::Success(Https(forwarded))
    }
}
