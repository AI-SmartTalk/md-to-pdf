//! Accounts, sessions and self-served API keys.
//!
//! This service had no notion of a user: a token was minted by hand, mailed to whoever
//! asked, and pasted into a browser's local storage. That was defensible while it ran for
//! one internal caller. It stopped being defensible the day a pricing page went up, because
//! an account is not a feature here — it is the spine. Without it there is no funnel, no
//! quota anyone can read, no history, and above all **no way for the verdict to
//! accumulate**: a single verdict is a card, a history of verdicts is a quality record, and
//! a quality record needs somewhere to hang.
//!
//! # Why files and not a database
//!
//! The service holds no database on purpose — it is stateless, and it keeps only files with
//! a short life. An account store is the first durable state, and it is small: a few
//! hundred bytes per user. It therefore reuses exactly what `assets.rs` already does, and
//! adds no dependency to an image that is audited by hand.
//!
//! # Why the identity is behind a seam
//!
//! AI SmartTalk already has users and organisations. The right long-term answer is one
//! identity for the whole house, and this module is deliberately shaped so it can become a
//! thin adapter in front of that: everything above it speaks in terms of `Account` and
//! `Session`, never of passwords. Building it here first is a way to ship the funnel without
//! blocking on that integration — not a decision to keep two directories forever.

use crate::sign;
use crate::types::AppError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Iterations of PBKDF2-HMAC-SHA256.
///
/// OWASP's floor for this construction is 600 000, and the cost is paid once per sign-in on
/// a blocking thread. Raising it later is safe: the count is stored beside the hash, so old
/// records keep verifying with the number they were written with.
const PBKDF2_ITERATIONS: u32 = 600_000;

/// How long a session stays valid without being touched
const SESSION_DAYS: u64 = 30;

/// The same lifetime, for the cookie that carries the session.
///
/// Without it the browser mints a *session cookie* and throws it away when it closes, so a
/// thirty-day session on the server was worth one browsing session in practice: everybody
/// had to sign in again, every time, and nothing said why.
pub const SESSION_MAX_AGE_DAYS: i64 = SESSION_DAYS as i64;

/// The shortest password we will store. Length is the only property that reliably survives
/// contact with a real user; complexity rules mostly produce `P@ssw0rd!`.
const MIN_PASSWORD: usize = 10;

/// The longest one we will hash.
///
/// PBKDF2 hashes the password inside *every* one of its six hundred thousand iterations, so
/// the work is proportional to its length: a one-megabyte "password" asks this service for
/// six hundred gigabytes of SHA-256, on a thread it holds the whole time, from a request
/// that needs no account and no key. The bound is generous — a passphrase is a few dozen
/// characters, and nothing legitimate comes near it.
const MAX_PASSWORD: usize = 256;

// ------------ The record ------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    /// Lower-cased, trimmed. It is the login and the only identifier a human types.
    pub email: String,
    pub created_at: String,
    /// `free`, `pro`, `team`. The tier is stored, never inferred from the keys someone has.
    pub plan: String,
    #[serde(default)]
    pub name: Option<String>,
    /// PBKDF2-HMAC-SHA256, as `pbkdf2$<iterations>$<salt hex>$<hash hex>`.
    ///
    /// Skipped on the way out: this struct is what the API returns, and a password hash has
    /// no business travelling to a browser even once.
    #[serde(skip_serializing)]
    pub password: String,
}

/// One API key. The secret itself is never stored — only its hash, exactly like a password.
///
/// That means a key can be shown **once**, at creation, and never again. It is a small
/// inconvenience that removes a whole class of incident: a copy of the store is not a copy
/// of everyone's credentials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyRecord {
    pub id: String,
    pub name: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used: Option<String>,
    #[serde(skip_serializing)]
    pub secret_hash: String,
    /// First characters of the secret, so a human can tell two keys apart in a list
    pub hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub token_hash: String,
    pub account_id: String,
    pub created_at: String,
    pub expires_unix: u64,
}

// ------------ Storage ------------

fn accounts_root() -> PathBuf {
    Path::new("public").join("accounts")
}

fn account_dir(id: &str) -> PathBuf {
    accounts_root().join(id)
}

/// Where an e-mail is resolved to an account id.
///
/// A directory per e-mail rather than a single index file: two simultaneous sign-ups would
/// race on one file and one of them would be lost, whereas `create_dir` is atomic and tells
/// the loser it lost. That is also what makes "this address is already taken" reliable.
fn email_link(email: &str) -> PathBuf {
    accounts_root()
        .join("by-email")
        .join(sign::sha256_hex(email.as_bytes()))
}

pub fn init() {
    if let Err(e) = fs::create_dir_all(accounts_root().join("by-email")) {
        error!("Could not create {:?}: {}", accounts_root(), e);
        return;
    }
    for index in ["by-key", "by-owner"] {
        if let Err(e) = fs::create_dir_all(accounts_root().join(index)) {
            error!("Could not create the {} index: {}", index, e);
        }
    }
    if let Err(e) = fs::create_dir_all(sessions_root()) {
        error!("Could not create {:?}: {}", sessions_root(), e);
    }
}

fn sessions_root() -> PathBuf {
    Path::new("public").join("sessions")
}

/// Index from a key's hash to the account that owns it.
///
/// Without it, authenticating a request means walking every account and every key on disk —
/// once per call. That is invisible at ten accounts and ruinous at ten thousand, and the
/// day it starts hurting is the day nobody has time to fix it.
fn key_index(secret_hash: &str) -> PathBuf {
    accounts_root().join("by-key").join(secret_hash)
}

/// Index from the attribution name a request carries — `7a5f8d/production` — back to the
/// account.
///
/// It exists so the quality record can be written from `helpers::finish_tool`, which knows
/// who owns the work but not which account that is, without threading an account through
/// fourteen route signatures where a caller could then forge it. Hashed rather than stored
/// verbatim because the name becomes a file name.
fn owner_index(owner: &str) -> PathBuf {
    accounts_root()
        .join("by-owner")
        .join(sign::sha256_hex(owner.as_bytes()))
}

/// Name under which work done from a browser session is filed.
///
/// A member who never mints a key — which is most of them — still has to see their own
/// operations in their record. The session is the credential in that case, and this is the
/// name it attributes to, sitting beside `production` or `zapier` in the same list.
pub const WEB_KEY_NAME: &str = "web";

/// The attribution name a credential of this account produces. Kept in one place because the
/// guard and the index have to agree on it exactly.
///
/// Twelve hex characters, not six: this name is the *key* of the owner index, so two
/// accounts sharing a prefix would file their work in the same place, and one member would
/// read another's record. Six characters is 24 bits, which collides around four thousand
/// accounts — a number this service intends to pass. Twelve is short enough to stay readable
/// in a log line and long enough that the question never comes up.
pub fn owner_name(account_id: &str, key_name: &str) -> String {
    format!("{}/{}", account_id.get(4..16).unwrap_or("member"), key_name)
}

/// Point the owner index at an account, refusing to move one that already belongs elsewhere.
///
/// Attribution that silently rebinds is worse than attribution that fails: it does not lose
/// the work, it files it under the wrong person.
fn bind_owner(owner: &str, account_id: &str) -> Result<(), AppError> {
    let index = owner_index(owner);

    if let Ok(existing) = fs::read_to_string(&index) {
        if existing.trim() != account_id {
            return Err(AppError::Conflict(format!(
                "The attribution name \"{}\" is already taken",
                owner
            )));
        }
        return Ok(());
    }

    fs::create_dir_all(index.parent().unwrap_or(&accounts_root()))?;
    fs::write(&index, account_id)?;
    Ok(())
}

/// Which account, if any, a piece of work belongs to.
pub fn account_for_owner(owner: &str) -> Option<String> {
    if owner.is_empty() {
        return None;
    }
    let id = fs::read_to_string(owner_index(owner)).ok()?;
    let id = id.trim().to_string();
    account_dir(&id).join("account.json").exists().then_some(id)
}

// ------------ Passwords ------------

/// PBKDF2-HMAC-SHA256, RFC 8018.
///
/// Built on the HMAC already in `sign.rs` rather than pulled in as a crate: it is twenty
/// lines, it is a standard, and the alternative — hashing a password with a bare SHA-256 —
/// would be indefensible in a service that stores anything at all.
fn pbkdf2(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    // One block is enough: the output we want is exactly the width of the hash.
    let mut block = Vec::with_capacity(salt.len() + 4);
    block.extend_from_slice(salt);
    block.extend_from_slice(&1u32.to_be_bytes());

    let mut u = sign::hmac_sha256(password, &block);
    let mut out = u;

    for _ in 1..iterations {
        u = sign::hmac_sha256(password, &u);
        for (acc, byte) in out.iter_mut().zip(u.iter()) {
            *acc ^= byte;
        }
    }

    out
}

fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = random_bytes(16)?;
    let hash = pbkdf2(password.as_bytes(), &salt, PBKDF2_ITERATIONS);
    Ok(format!(
        "pbkdf2${}${}${}",
        PBKDF2_ITERATIONS,
        sign::hex(&salt),
        sign::hex(&hash)
    ))
}

/// Verify against the parameters the record was written with, not against today's.
pub fn verify_password(password: &str, stored: &str) -> bool {
    let mut parts = stored.split('$');
    let (Some("pbkdf2"), Some(iterations), Some(salt), Some(expected)) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };

    let (Ok(iterations), Some(salt)) = (iterations.parse::<u32>(), unhex(salt)) else {
        return false;
    };

    let actual = sign::hex(&pbkdf2(password.as_bytes(), &salt, iterations));
    sign::constant_time_eq(actual.as_bytes(), expected.as_bytes())
}

fn random_bytes(n: usize) -> Result<Vec<u8>, AppError> {
    use std::io::Read;
    let mut buf = vec![0u8; n];
    fs::File::open("/dev/urandom")?.read_exact(&mut buf)?;
    Ok(buf)
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

// ------------ Sign-up and sign-in ------------

/// Normalise an address the way a human will retype it tomorrow.
pub fn normalise_email(email: &str) -> String {
    email.trim().to_lowercase()
}

/// Deliberately permissive: an address either receives mail or it does not, and no regular
/// expression has ever settled that argument. This rejects what cannot possibly work.
pub fn plausible_email(email: &str) -> bool {
    let mut parts = email.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };

    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && email.len() <= 254
        && !email.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Refuse a password we should not even try to hash.
///
/// Called before `create` **and** before `authenticate`: the expensive part is the hashing,
/// and an attacker who cannot sign up can still send a megabyte to the sign-in form.
fn check_password_length(password: &str) -> Result<(), AppError> {
    let length = password.chars().count();

    if length < MIN_PASSWORD {
        return Err(AppError::BadRequest(format!(
            "The password must be at least {} characters",
            MIN_PASSWORD
        )));
    }
    if length > MAX_PASSWORD {
        return Err(AppError::BadRequest(format!(
            "The password must be at most {} characters",
            MAX_PASSWORD
        )));
    }

    Ok(())
}

pub fn create(email: &str, password: &str, name: Option<String>) -> Result<Account, AppError> {
    let email = normalise_email(email);

    if !plausible_email(&email) {
        return Err(AppError::BadRequest(
            "This does not look like an e-mail address".to_string(),
        ));
    }
    check_password_length(password)?;

    let id = crate::helpers::random_id("acc")?;
    let link = email_link(&email);

    // `create_dir` fails if the directory exists, and that failure is the whole locking
    // strategy: two sign-ups racing on one address cannot both win.
    fs::create_dir_all(link.parent().unwrap_or(&accounts_root()))?;
    if fs::create_dir(&link).is_err() {
        return Err(AppError::Conflict(
            "An account already exists for this address".to_string(),
        ));
    }

    let account = Account {
        id: id.clone(),
        email: email.clone(),
        created_at: crate::assets::rfc3339(crate::assets::now_unix()),
        plan: "free".to_string(),
        name,
        password: hash_password(password)?,
    };

    let dir = account_dir(&id);
    if let Err(e) = fs::create_dir_all(&dir).and_then(|_| write_account(&dir, &account)) {
        // The address reservation must not outlive a failed creation, or it locks the
        // address for ever against an account that does not exist.
        let _ = fs::remove_dir_all(&link);
        return Err(AppError::Io(e));
    }
    fs::write(link.join("id"), &id)?;

    // Bound at creation, not at first use: a member who works entirely from the browser and
    // never mints a key must still find their operations in their record, and the record is
    // written from a worker thread that only knows this name.
    bind_owner(&owner_name(&id, WEB_KEY_NAME), &id)?;

    Ok(account)
}

fn write_account(dir: &Path, account: &Account) -> std::io::Result<()> {
    // `password` is `skip_serializing`, so it has to be written beside the record rather
    // than inside it — which is also why nothing that serialises an `Account` can leak it.
    let mut value = serde_json::to_value(account).map_err(std::io::Error::other)?;
    value["password"] = serde_json::Value::String(account.password.clone());
    fs::write(
        dir.join("account.json"),
        serde_json::to_string(&value).map_err(std::io::Error::other)?,
    )
}

pub fn by_id(id: &str) -> Result<Account, AppError> {
    let id = validate_id(id, "acc")?;
    let raw = fs::read_to_string(account_dir(id).join("account.json"))
        .map_err(|_| AppError::NotFound("Account not found".to_string()))?;

    serde_json::from_str(&raw).map_err(|e| AppError::ProcessFailed {
        message: "Could not read the account".to_string(),
        stderr: e.to_string(),
    })
}

pub fn by_email(email: &str) -> Option<Account> {
    let id = fs::read_to_string(email_link(&normalise_email(email)).join("id")).ok()?;
    by_id(id.trim()).ok()
}

/// Sign in, in constant-ish time whether or not the address exists.
///
/// An unknown address must cost the same as a wrong password, or the response time tells an
/// attacker which of your users have accounts here.
pub fn authenticate(email: &str, password: &str) -> Result<Account, AppError> {
    const DUMMY: &str = "pbkdf2$600000$00000000000000000000000000000000$0000000000000000000000000000000000000000000000000000000000000000";

    let refusal = || AppError::Unauthorized("Wrong e-mail address or password".to_string());

    // Refused before hashing, and reported as a plain refusal: a distinct error here would
    // say "that length is not what this account uses", which is a hint we owe nobody.
    if check_password_length(password).is_err() {
        return Err(refusal());
    }

    match by_email(email) {
        Some(account) if verify_password(password, &account.password) => Ok(account),
        Some(_) => Err(refusal()),
        None => {
            let _ = verify_password(password, DUMMY);
            Err(refusal())
        }
    }
}

// ------------ Sessions ------------

/// Open a session and hand back the token that names it.
///
/// Only the token's hash is stored: a session file is then useless to whoever reads it,
/// which matters because these live on the same volume as everything else.
pub fn open_session(account_id: &str) -> Result<String, AppError> {
    let token = crate::helpers::random_id("ses")?;
    let session = Session {
        token_hash: sign::sha256_hex(token.as_bytes()),
        account_id: account_id.to_string(),
        created_at: crate::assets::rfc3339(crate::assets::now_unix()),
        expires_unix: crate::assets::now_unix() + SESSION_DAYS * 86_400,
    };

    fs::create_dir_all(sessions_root())?;
    fs::write(
        sessions_root().join(&session.token_hash),
        serde_json::to_string(&session).map_err(|e| AppError::ProcessFailed {
            message: "Could not write the session".to_string(),
            stderr: e.to_string(),
        })?,
    )?;

    Ok(token)
}

pub fn account_for_session(token: &str) -> Option<Account> {
    let path = sessions_root().join(sign::sha256_hex(token.as_bytes()));
    let session: Session = serde_json::from_str(&fs::read_to_string(&path).ok()?).ok()?;

    if session.expires_unix <= crate::assets::now_unix() {
        let _ = fs::remove_file(&path);
        return None;
    }

    by_id(&session.account_id).ok()
}

pub fn close_session(token: &str) {
    let _ = fs::remove_file(sessions_root().join(sign::sha256_hex(token.as_bytes())));
}

// ------------ API keys ------------

/// Mint a key. The secret is returned **once** and never stored in the clear.
pub fn create_key(account_id: &str, name: &str) -> Result<(ApiKeyRecord, String), AppError> {
    let name = clean_name(name)?;
    let dir = account_dir(validate_id(account_id, "acc")?).join("keys");
    fs::create_dir_all(&dir)?;

    if fs::read_dir(&dir)
        .map(|entries| entries.count())
        .unwrap_or(0)
        >= 20
    {
        return Err(AppError::BadRequest(
            "This account already has 20 keys — revoke one before creating another".to_string(),
        ));
    }

    // Names are what tells two keys apart in a log line, in a metric label and in the
    // quality record — so two keys sharing one is not a cosmetic problem: they share their
    // attribution entry, and revoking either would unindex the other.
    if list_keys(account_id)?
        .iter()
        .any(|existing| existing.name.eq_ignore_ascii_case(&name))
    {
        return Err(AppError::Conflict(format!(
            "This account already has a key named \"{}\"",
            name
        )));
    }

    let secret = crate::helpers::random_id("sk")?;
    let record = ApiKeyRecord {
        id: crate::helpers::random_id("key")?,
        name,
        created_at: crate::assets::rfc3339(crate::assets::now_unix()),
        last_used: None,
        secret_hash: sign::sha256_hex(secret.as_bytes()),
        hint: secret.chars().take(11).collect(),
    };

    let mut value = serde_json::to_value(&record).map_err(|e| AppError::ProcessFailed {
        message: "Could not write the key".to_string(),
        stderr: e.to_string(),
    })?;
    value["secret_hash"] = serde_json::Value::String(record.secret_hash.clone());
    fs::write(
        dir.join(format!("{}.json", record.id)),
        serde_json::to_string(&value).map_err(|e| AppError::ProcessFailed {
            message: "Could not write the key".to_string(),
            stderr: e.to_string(),
        })?,
    )?;

    // The index is written last: a crash between the two leaves a key that does not
    // authenticate, which is safe. The reverse order would leave one that authenticates
    // against an account record that was never written.
    let index = key_index(&record.secret_hash);
    fs::create_dir_all(index.parent().unwrap_or(&accounts_root()))?;
    fs::write(&index, account_id)?;

    bind_owner(&owner_name(account_id, &record.name), account_id)?;

    Ok((record, secret))
}

pub fn list_keys(account_id: &str) -> Result<Vec<ApiKeyRecord>, AppError> {
    let dir = account_dir(validate_id(account_id, "acc")?).join("keys");
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };

    let mut keys: Vec<ApiKeyRecord> = entries
        .flatten()
        .filter_map(|entry| fs::read_to_string(entry.path()).ok())
        .filter_map(|raw| serde_json::from_str(&raw).ok())
        .collect();

    keys.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(keys)
}

pub fn revoke_key(account_id: &str, key_id: &str) -> Result<(), AppError> {
    let path = account_dir(validate_id(account_id, "acc")?)
        .join("keys")
        .join(format!("{}.json", validate_id(key_id, "key")?));

    // The index goes first: a revoked key must stop authenticating even if the record
    // itself survives a failed unlink.
    if let Ok(raw) = fs::read_to_string(&path) {
        if let Ok(record) = serde_json::from_str::<ApiKeyRecord>(&raw) {
            let _ = fs::remove_file(key_index(&record.secret_hash));
            let _ = fs::remove_file(owner_index(&owner_name(account_id, &record.name)));
        }
    }

    fs::remove_file(&path).map_err(|_| AppError::NotFound("Key not found".to_string()))
}

/// Resolve a presented secret to the account that owns it, in one read.
///
/// The secret is looked up by its hash, so the index leaks nothing to whoever reads the
/// directory: knowing that a hash exists does not produce the key it came from.
pub fn account_for_key(secret: &str) -> Option<(Account, ApiKeyRecord)> {
    let hash = sign::sha256_hex(secret.as_bytes());
    let account_id = fs::read_to_string(key_index(&hash)).ok()?;
    let account = by_id(account_id.trim()).ok()?;

    let record = list_keys(&account.id)
        .ok()?
        .into_iter()
        .find(|key| sign::constant_time_eq(key.secret_hash.as_bytes(), hash.as_bytes()))?;

    touch_key(&account.id, &record);

    Some((account, record))
}

/// Note that this key was used today, at most once a day.
///
/// The workspace shows "last used" beside every key, and it is the one signal a member
/// leans on to decide which one is safe to revoke — so leaving it permanently empty does
/// not merely omit information, it says "never used" about the key running production.
///
/// A write per request would be a write per request. A write per key per day gives the list
/// exactly the resolution it displays, and costs nothing the rest of the time.
fn touch_key(account_id: &str, record: &ApiKeyRecord) {
    let today = rfc3339_day(crate::assets::now_unix());
    if record
        .last_used
        .as_deref()
        .is_some_and(|seen| seen == today)
    {
        return;
    }

    let path = account_dir(account_id)
        .join("keys")
        .join(format!("{}.json", record.id));
    let Ok(raw) = fs::read_to_string(&path) else {
        return;
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return;
    };

    value["last_used"] = serde_json::Value::String(today);
    // Bookkeeping: a failure here must never refuse a request that authenticated correctly.
    if let Ok(json) = serde_json::to_string(&value) {
        let _ = fs::write(&path, json);
    }
}

/// `YYYY-MM-DD` — the resolution the key list actually shows
fn rfc3339_day(unix: u64) -> String {
    crate::assets::rfc3339(unix).chars().take(10).collect()
}

// ------------ Shared validation ------------

fn validate_id<'a>(id: &'a str, prefix: &str) -> Result<&'a str, AppError> {
    let expected = prefix.len() + 1 + 32;
    let valid = id.len() == expected
        && id.starts_with(prefix)
        && id.as_bytes().get(prefix.len()) == Some(&b'_')
        && id[prefix.len() + 1..]
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase());

    if !valid {
        return Err(AppError::BadRequest(format!("Invalid {} id", prefix)));
    }
    Ok(id)
}

/// A key name is shown in a list, written to a log and used as a metric label
fn clean_name(name: &str) -> Result<String, AppError> {
    let cleaned: String = name
        .trim()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == ' ')
        .take(48)
        .collect();

    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() {
        return Err(AppError::BadRequest(
            "The key needs a name — \"production\", \"zapier\", anything you will recognise"
                .to_string(),
        ));
    }
    // `web` is what a browser session is filed under. A key wearing the same name would
    // share its attribution entry, and revoking the key would silently stop recording
    // everything the member does on the site.
    if cleaned.eq_ignore_ascii_case(WEB_KEY_NAME) {
        return Err(AppError::BadRequest(format!(
            "\"{}\" is reserved for your browser session — pick another name",
            WEB_KEY_NAME
        )));
    }
    Ok(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6070's PBKDF2-HMAC-SHA1 vectors do not apply here; this is the SHA-256 variant,
    /// checked against a published vector so a rewrite of the XOR loop cannot pass unseen.
    #[test]
    fn matches_a_published_pbkdf2_vector() {
        let out = pbkdf2(b"password", b"salt", 1);
        assert_eq!(
            sign::hex(&out),
            "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
        );

        let out = pbkdf2(b"password", b"salt", 2);
        assert_eq!(
            sign::hex(&out),
            "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43"
        );
    }

    #[test]
    fn a_password_verifies_against_its_own_parameters() {
        let stored = hash_password("un mot de passe assez long").unwrap();
        assert!(stored.starts_with("pbkdf2$600000$"));
        assert!(verify_password("un mot de passe assez long", &stored));
        assert!(!verify_password("un autre mot de passe", &stored));
    }

    /// Raising the iteration count must not lock existing users out
    #[test]
    fn an_old_record_keeps_verifying_with_its_own_iteration_count() {
        let salt = b"0123456789abcdef";
        let old = format!(
            "pbkdf2$1000${}${}",
            sign::hex(salt),
            sign::hex(&pbkdf2(b"secret assez long", salt, 1000))
        );
        assert!(verify_password("secret assez long", &old));
    }

    #[test]
    fn refuses_what_cannot_be_a_password_hash() {
        for bad in [
            "",
            "plain",
            "pbkdf2$abc$de$f",
            "sha256$1$2$3",
            "pbkdf2$1000$zz$00",
        ] {
            assert!(!verify_password("secret assez long", bad), "{}", bad);
        }
    }

    #[test]
    fn normalises_the_address_the_way_a_human_retypes_it() {
        assert_eq!(
            normalise_email("  Jean.Dupont@Example.COM "),
            "jean.dupont@example.com"
        );
    }

    #[test]
    fn rejects_only_what_cannot_possibly_work() {
        for good in ["a@b.co", "jean.dupont+pdf@example.com", "x@sous.domaine.fr"] {
            assert!(plausible_email(good), "{} should be accepted", good);
        }
        for bad in [
            "",
            "sans-arobase",
            "@example.com",
            "a@b",
            "a@.com",
            "a@b.",
            "a b@c.com",
            "a@b.com\n",
        ] {
            assert!(!plausible_email(bad), "{:?} should be refused", bad);
        }
    }

    #[test]
    fn a_key_name_survives_being_a_metric_label() {
        assert_eq!(clean_name("  production ").unwrap(), "production");
        assert_eq!(clean_name("prod\"}\n=x").unwrap(), "prodx");
        assert!(clean_name("   ").is_err());
        assert!(clean_name("\"\n").is_err());
        assert_eq!(clean_name(&"a".repeat(100)).unwrap().len(), 48);
    }

    #[test]
    fn ids_are_accepted_only_in_the_shape_we_mint() {
        let good = format!("acc_{}", "a".repeat(32));
        assert!(validate_id(&good, "acc").is_ok());

        for bad in [
            "acc_short".to_string(),
            format!("acc_{}", "A".repeat(32)),
            format!("key_{}", "a".repeat(32)),
            format!("acc_{}", "../".repeat(11)),
        ] {
            assert!(validate_id(&bad, "acc").is_err(), "{}", bad);
        }
    }
}
