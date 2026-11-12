//! Change history: who changed what and when, plus undo for deleted transactions.

use crate::books::{fmt_cents, nice_dates, Books};
use base64::Engine as _;
use rusqlite::types::{Value as Sql, ValueRef};
use rusqlite::{OptionalExtension, Result};
use serde_json::{json, Map, Value};

pub struct Change {
    pub id: i64,
    /// Local time, "YYYY-MM-DD HH:MM".
    pub at: String,
    pub user: String,
    pub what: String,
    /// A deleted transaction that can still be put back.
    pub undoable: bool,
}

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

fn bad(msg: &str) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(msg.into())
}

impl Books {
    pub fn log_change(&self, user: &str, what: &str, undo: Option<&str>) -> Result<()> {
        let at = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
        self.db.execute("INSERT INTO history(at, user, what, undo) VALUES (?1, ?2, ?3, ?4)", rusqlite::params![at, user, what, undo])?;
        Ok(())
    }

    /// The newest `limit` changes, newest first.
    pub fn history(&self, limit: i64) -> Result<Vec<Change>> {
        let mut st = self.db.prepare("SELECT id, at, user, what, undo IS NOT NULL FROM history ORDER BY id DESC LIMIT ?1")?;
        let rows = st.query_map([limit], |r| Ok(Change { id: r.get(0)?, at: r.get(1)?, user: r.get(2)?, what: r.get(3)?, undoable: r.get(4)? }))?;
        rows.collect()
    }

    /// A description and a restorable copy of a transaction, taken just before deleting it.
    pub fn entry_snapshot(&self, id: i64) -> Result<(String, String)> {
        let entry = self.rows("entries", "id", id)?.into_iter().next().ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        let lines = self.rows("lines", "entry_id", id)?;
        let attachment = self.rows("attachments", "entry_id", id)?;
        let text = |k: &str| entry.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        let biggest = lines.iter().filter_map(|l| l.get("cents").and_then(Value::as_i64)).map(i64::abs).max().unwrap_or(0);
        let what = format!("Deleted the transaction {} · {} · {}", nice_dates(&text("date")), text("memo"), fmt_cents(biggest));
        Ok((what, json!({ "entry": entry, "lines": lines, "attachments": attachment }).to_string()))
    }

    /// Puts back the transaction deleted in history row `id`, then marks that row as undone.
    pub fn undo_change(&self, id: i64) -> Result<()> {
        let undo: Option<String> = self.db.query_row("SELECT undo FROM history WHERE id = ?1", [id], |r| r.get(0)).optional()?.flatten();
        let snap: Value = serde_json::from_str(&undo.ok_or(bad("that change can't be undone"))?).map_err(|_| bad("the saved copy is damaged"))?;
        let entry = snap["entry"].as_object().ok_or(bad("the saved copy is damaged"))?;
        self.check_open(entry.get("date").and_then(Value::as_str).unwrap_or_default())?;
        self.atomic(|| {
            let new = self.insert_row("entries", entry, None)?;
            for (table, rows) in [("lines", &snap["lines"]), ("attachments", &snap["attachments"])] {
                for row in rows.as_array().into_iter().flatten().filter_map(Value::as_object) {
                    self.insert_row(table, row, Some(new))?;
                }
            }
            self.db.execute("UPDATE history SET undo = NULL WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    /// Rows of `table` where `col` = `id`, as JSON objects (blobs as {"b64": ...}).
    fn rows(&self, table: &str, col: &str, id: i64) -> Result<Vec<Map<String, Value>>> {
        let mut st = self.db.prepare(&format!("SELECT * FROM {table} WHERE {col} = ?1"))?;
        let names: Vec<String> = st.column_names().map(str::to_string).collect();
        let rows = st.query_map([id], |r| {
            let mut m = Map::new();
            for (i, n) in names.iter().enumerate() {
                let v = match r.get_ref(i)? {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(x) => x.into(),
                    ValueRef::Real(x) => x.into(),
                    ValueRef::Text(t) => String::from_utf8_lossy(t).into(),
                    ValueRef::Blob(b) => json!({ "b64": B64.encode(b) }),
                };
                m.insert(n.clone(), v);
            }
            Ok(m)
        })?;
        rows.collect()
    }

    /// Inserts a row saved by `rows` under a new id; `entry` replaces its entry_id.
    fn insert_row(&self, table: &str, row: &Map<String, Value>, entry: Option<i64>) -> Result<i64> {
        let mut cols = vec![];
        let mut vals: Vec<Sql> = vec![];
        for (k, v) in row {
            // Column names come from our own snapshot of this table; refuse anything else.
            if k == "id" || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                continue;
            }
            cols.push(k.clone());
            vals.push(match (k.as_str(), v) {
                ("entry_id", _) => entry.map_or(Sql::Null, Sql::Integer),
                (_, Value::Null) => Sql::Null,
                (_, Value::Number(n)) => n.as_i64().map_or_else(|| Sql::Real(n.as_f64().unwrap_or(0.0)), Sql::Integer),
                (_, Value::String(s)) => Sql::Text(s.clone()),
                (_, Value::Object(o)) => Sql::Blob(o.get("b64").and_then(Value::as_str).and_then(|s| B64.decode(s).ok()).unwrap_or_default()),
                _ => Sql::Null,
            });
        }
        let marks = vec!["?"; cols.len()].join(", ");
        self.db.execute(&format!("INSERT INTO {table} ({}) VALUES ({marks})", cols.join(", ")), rusqlite::params_from_iter(vals))?;
        Ok(self.db.last_insert_rowid())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn deleted_transactions_come_back() {
        let b = crate::books::Books::open(":memory:").unwrap();
        let acct = |n: &str| b.accounts().unwrap().into_iter().find(|a| a.name == n).unwrap().id;
        let id = b.post_simple("2026-03-02", "Boards", acct("Checking"), acct("Job Materials"), -12_000).unwrap();
        b.attach(id, "receipt.png", &[0x89, 1, 2, 3]).unwrap();
        let (what, snap) = b.entry_snapshot(id).unwrap();
        assert_eq!(what, "Deleted the transaction Mar 2, 2026 · Boards · $120.00");
        b.delete_entry(id).unwrap();
        b.log_change("alex", &what, Some(&snap)).unwrap();
        let h = &b.history(10).unwrap()[0];
        assert!(h.undoable && h.user == "alex");
        b.undo_change(h.id).unwrap();
        let back = b.entries().unwrap().into_iter().find(|e| e.memo == "Boards").unwrap();
        assert_eq!(b.attachment(back.id).unwrap().unwrap().1, vec![0x89, 1, 2, 3]);
        assert!(!b.history(10).unwrap()[0].undoable, "can't undo twice");
        assert!(b.undo_change(h.id).is_err());
    }
}
