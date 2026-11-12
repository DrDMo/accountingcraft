//! Logins and encryption. The books file is SQLCipher-encrypted with one random 256-bit key.
//! `<books>.keys` (plain JSON beside it) holds that key wrapped once per user and once for the
//! recovery key: wrap = key XOR Argon2id(password, fresh salt)[..32], check = Argon2id(..)[32..].
//! Roles live inside the encrypted file (`users` table), so they can't be edited without a password.

use crate::books::Books;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub type Key = [u8; 32];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Role {
    Owner,
    Bookkeeper,
    Viewer,
}

impl Role {
    pub const ALL: [Role; 3] = [Role::Owner, Role::Bookkeeper, Role::Viewer];
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Bookkeeper => "bookkeeper",
            Role::Viewer => "viewer",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Role::Owner => "Owner",
            Role::Bookkeeper => "Bookkeeper",
            Role::Viewer => "View-only",
        }
    }
    pub fn parse(s: &str) -> Role {
        match s {
            "owner" => Role::Owner,
            "bookkeeper" => Role::Bookkeeper,
            _ => Role::Viewer, // unknown means least access
        }
    }
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

fn random<const N: usize>() -> Result<[u8; N], String> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|e| format!("no secure random numbers: {e}"))?;
    Ok(b)
}

fn derive(secret: &str, salt: &[u8]) -> Result<[u8; 64], String> {
    let mut out = [0u8; 64];
    // 64 MiB, 3 passes: slow enough to make guessing from a stolen .keys file expensive.
    let params = argon2::Params::new(64 * 1024, 3, 1, Some(64)).map_err(|e| e.to_string())?;
    argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
        .hash_password_into(secret.as_bytes(), salt, &mut out)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

fn wrap(key: &Key, secret: &str) -> Result<Value, String> {
    let salt = random::<16>()?;
    let d = derive(secret, &salt)?;
    let w: Vec<u8> = key.iter().zip(&d[..32]).map(|(a, b)| a ^ b).collect();
    Ok(json!({ "salt": hex(&salt), "wrap": hex(&w), "check": hex(&d[32..]) }))
}

/// Recovers the key from one wrapped entry, or None when the secret is wrong.
fn unwrap_entry(e: &Value, secret: &str) -> Option<Key> {
    let field = |k: &str| e.get(k).and_then(Value::as_str).and_then(unhex);
    let (salt, w, check) = (field("salt")?, field("wrap")?, field("check")?);
    let d = derive(secret, &salt).ok()?;
    if d[32..] != check[..] || w.len() != 32 {
        return None;
    }
    let mut k = [0u8; 32];
    for i in 0..32 {
        k[i] = w[i] ^ d[i];
    }
    Some(k)
}

pub fn keys_path(db: &str) -> PathBuf {
    PathBuf::from(format!("{db}.keys"))
}

/// True when the file on disk is an ordinary (unencrypted) SQLite file.
pub fn is_plain(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 16];
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut head)).is_ok_and(|_| &head == b"SQLite format 3\0")
}

fn read_keys(db: &str) -> Result<Value, String> {
    let p = keys_path(db);
    let s = std::fs::read_to_string(&p).map_err(|e| format!("can't read {}: {e}", p.display()))?;
    let v: Value = serde_json::from_str(&s).map_err(|e| format!("{} is damaged: {e}", p.display()))?;
    // Assigning into a wrong-shaped Value panics, so check the shape once here.
    if !v.is_object() || !v["users"].is_object() {
        return Err(format!("{} is damaged.", p.display()));
    }
    Ok(v)
}

/// Writes the keys file via a temp file and rename, so a crash never leaves it half written.
fn write_keys(db: &str, v: &Value) -> Result<(), String> {
    let p = keys_path(db);
    let tmp = PathBuf::from(format!("{db}.keys.tmp"));
    let text = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        f.write_all(text.as_bytes()).and_then(|_| f.sync_all()).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
}

/// Matches SQLite's NOCASE (ASCII only), so the keys file and the users table agree on who is who.
fn norm(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

/// A recovery key: 128 random bits shown as eight groups of four hex digits.
fn new_recovery() -> Result<String, String> {
    let h = hex(&random::<16>()?).to_uppercase();
    Ok(h.as_bytes().chunks(4).map(|c| String::from_utf8_lossy(c).to_string()).collect::<Vec<_>>().join("-"))
}

fn norm_recovery(code: &str) -> String {
    code.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_uppercase()
}

pub fn check_password(pw: &str) -> Result<(), String> {
    if pw.chars().count() < 8 {
        return Err("Passwords need at least 8 characters.".into());
    }
    Ok(())
}

/// The signed-in person after a successful unlock.
pub struct Session {
    pub books: Books,
    pub key: Key,
    pub user: String,
    pub role: Role,
}

fn session(db: &str, key: Key, user: &str) -> Result<Session, String> {
    let books = Books::open_key(db, Some(&key)).map_err(|e| format!("couldn't open the books: {e}"))?;
    let role = books.user_role(user).map_err(|e| e.to_string())?.ok_or("That user no longer has access.")?;
    if role == Role::Viewer {
        books.db.execute_batch("PRAGMA query_only = ON;").map_err(|e| e.to_string())?;
    }
    Ok(Session { books, key, user: user.trim().to_string(), role })
}

/// Signs in with a username and password.
pub fn login(db: &str, user: &str, pw: &str) -> Result<Session, String> {
    let keys = read_keys(db)?;
    let key = keys["users"].get(norm(user)).and_then(|e| unwrap_entry(e, pw)).ok_or("Wrong username or password.")?;
    session(db, key, user)
}

/// Uses the recovery key to set a new password for `user` (who must be an owner), then signs in.
pub fn recover(db: &str, code: &str, user: &str, new_pw: &str) -> Result<Session, String> {
    check_password(new_pw)?;
    let mut keys = read_keys(db)?;
    let key = unwrap_entry(&keys["recovery"], &norm_recovery(code)).ok_or("That recovery key doesn't match these books.")?;
    let s = session(db, key, user).map_err(|_| "No owner by that name in these books.".to_string())?;
    if s.role != Role::Owner {
        return Err("The recovery key can only reset an owner's password.".into());
    }
    keys["users"][norm(user)] = wrap(&key, new_pw)?;
    write_keys(db, &keys)?;
    Ok(s)
}

/// First sign-in for a books file: creates the owner, encrypts the file (an existing plain file is
/// converted in place) and returns the session plus the recovery key to show once.
pub fn setup(db: &str, owner: &str, pw: &str) -> Result<(Session, String), String> {
    if owner.trim().is_empty() {
        return Err("Enter a username.".into());
    }
    check_password(pw)?;
    if keys_path(db).exists() {
        return Err("These books already have logins.".into());
    }
    let key = random::<32>()?;
    let recovery = new_recovery()?;
    let keys = json!({
        "version": 1,
        "users": { norm(owner): wrap(&key, pw)? },
        "recovery": wrap(&key, &norm_recovery(&recovery))?,
    });
    let path = Path::new(db);
    if path.exists() && std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) > 0 {
        if !is_plain(path) {
            return Err("This file is encrypted but its .keys file is missing. Put the .keys file back beside it.".into());
        }
        let old = encrypt_in_place(db, &key)?;
        // The plain copy goes only once the keys are safely on disk; otherwise put it back.
        if let Err(e) = write_keys(db, &keys) {
            let _ = std::fs::remove_file(db);
            let _ = std::fs::rename(&old, db);
            return Err(format!("couldn't save the keys, so nothing was changed: {e}"));
        }
        let _ = std::fs::remove_file(&old);
    } else {
        write_keys(db, &keys)?;
    }
    let books = Books::open_key(db, Some(&key)).map_err(|e| e.to_string())?;
    books.set_user(owner.trim(), Role::Owner).map_err(|e| e.to_string())?;
    drop(books);
    Ok((session(db, key, owner)?, recovery))
}

/// Rewrites a plain books file as an encrypted one and returns where the plain copy was moved,
/// for the caller to delete once the keys are saved.
fn encrypt_in_place(db: &str, key: &Key) -> Result<String, String> {
    let tmp = format!("{db}.encrypting");
    let _ = std::fs::remove_file(&tmp);
    if Path::new(&format!("{db}.unencrypted")).exists() {
        return Err(format!("{db}.unencrypted is left from an earlier attempt. Move it somewhere safe first."));
    }
    {
        let plain = rusqlite::Connection::open(db).map_err(|e| e.to_string())?;
        plain
            .execute("ATTACH DATABASE ?1 AS enc KEY ?2", [tmp.as_str(), &format!("x'{}'", hex(key))])
            .map_err(|e| format!("couldn't encrypt: {e}"))?;
        plain.query_row("SELECT sqlcipher_export('enc')", [], |_| Ok(())).map_err(|e| format!("couldn't encrypt: {e}"))?;
        plain.execute_batch("DETACH DATABASE enc;").map_err(|e| e.to_string())?;
    }
    Books::open_key(&tmp, Some(key)).and_then(|b| b.accounts()).map_err(|e| format!("encrypted copy didn't check out: {e}"))?;
    // Keep the plain file under another name until the swap is done.
    let old = format!("{db}.unencrypted");
    std::fs::rename(db, &old).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&tmp, db) {
        let _ = std::fs::rename(&old, db);
        return Err(e.to_string());
    }
    Ok(old)
}

/// Adds a user or resets their password (owner only, enforced by the caller).
pub fn set_login(books: &Books, key: &Key, db: &str, user: &str, pw: &str, role: Role) -> Result<(), String> {
    if user.trim().is_empty() {
        return Err("Enter a username.".into());
    }
    check_password(pw)?;
    let mut keys = read_keys(db)?;
    books.set_user(user.trim(), role).map_err(|e| e.to_string())?;
    keys["users"][norm(user)] = wrap(key, pw)?;
    write_keys(db, &keys)
}

pub fn change_password(key: &Key, db: &str, me: &str, pw: &str) -> Result<(), String> {
    check_password(pw)?;
    let mut keys = read_keys(db)?;
    keys["users"][norm(me)] = wrap(key, pw)?;
    write_keys(db, &keys)
}

pub fn remove_login(books: &Books, db: &str, me: &str, user: &str) -> Result<(), String> {
    if norm(user) == norm(me) {
        return Err("You can't remove yourself.".into());
    }
    books.remove_user(user).map_err(|e| e.to_string())?; // refuses to remove the last owner
    let mut keys = read_keys(db)?;
    if let Some(m) = keys["users"].as_object_mut() {
        m.remove(&norm(user));
    }
    write_keys(db, &keys)
}

/// Replaces the recovery key; the old one stops working.
pub fn new_recovery_key(key: &Key, db: &str) -> Result<String, String> {
    let code = new_recovery()?;
    let mut keys = read_keys(db)?;
    keys["recovery"] = wrap(key, &norm_recovery(&code))?;
    write_keys(db, &keys)?;
    Ok(code)
}

/// Plain (unencrypted) backups left over from before logins were turned on.
pub fn plain_backups(db: &str) -> Vec<PathBuf> {
    let dir = Path::new(db).parent().unwrap_or(Path::new(".")).join("backups");
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "db") && is_plain(p)).collect())
        .unwrap_or_default();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logins_and_encryption() {
        let dir = std::env::temp_dir().join(format!("ac-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("b.db").display().to_string();
        // An existing plain file with data in it.
        Books::open(&db).unwrap().add_account("1090", "Rainy Day Fund", "asset", "").unwrap();
        assert!(is_plain(Path::new(&db)));

        let (s, code) = setup(&db, "Alex", "correct horse").unwrap();
        assert_eq!(s.role, Role::Owner);
        drop(s);
        assert!(!is_plain(Path::new(&db)), "file is encrypted");
        assert!(!Path::new(&format!("{db}.unencrypted")).exists(), "plain copy removed");
        assert!(Books::open(&db).is_err(), "no key, no books");
        assert!(login(&db, "alex", "wrong password").is_err());
        let s = login(&db, " ALEX ", "correct horse").unwrap();
        assert!(s.books.accounts().unwrap().iter().any(|a| a.name == "Rainy Day Fund"), "data survived encryption");

        // Backups stay encrypted and open with the same key.
        let bak = dir.join("bak.db");
        s.books.backup_to(&bak).unwrap();
        assert!(!is_plain(&bak));
        assert!(Books::open_key(bak.to_str().unwrap(), Some(&s.key)).unwrap().accounts().unwrap().len() > 5);

        // A view-only user can read but the file refuses changes.
        set_login(&s.books, &s.key, &db, "Pat", "pat password", Role::Viewer).unwrap();
        let v = login(&db, "pat", "pat password").unwrap();
        assert_eq!(v.role, Role::Viewer);
        assert!(v.books.accounts().is_ok());
        assert!(v.books.add_account("1091", "Nope", "asset", "").is_err());
        drop(v);
        remove_login(&s.books, &db, "alex", "Pat").unwrap();
        assert!(login(&db, "pat", "pat password").is_err());
        assert!(remove_login(&s.books, &db, "alex", "alex").is_err());
        drop(s);

        // Recovery key resets the owner's password.
        assert!(recover(&db, "0000-0000", "alex", "new password").is_err());
        let s = recover(&db, &code.to_lowercase().replace('-', " "), "alex", "new password").unwrap();
        drop(s);
        assert!(login(&db, "alex", "correct horse").is_err());
        assert!(login(&db, "alex", "new password").is_ok());
        assert!(check_password("short").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
