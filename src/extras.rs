//! Bookkeeping helpers around the core ledger: import rules, reconciliation, backups, receipts,
//! mileage, recurring items and time tracking. All of it lives in the same single books file.

use crate::books::{fmt_cents, Books, DocInput, Item};
use chrono::{Datelike, Months, NaiveDate};
use rusqlite::{params, OptionalExtension, Result};

fn bad(msg: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(msg.into())
}

/// "Anything whose description contains `pattern` goes to `account`" (plus optional vendor/job).
#[derive(Clone, Debug)]
pub struct Rule {
    pub id: i64,
    pub pattern: String,
    pub account: i64,
    pub vendor: Option<i64>,
    pub job: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct ReconLine {
    pub line_id: i64,
    pub date: String,
    pub memo: String,
    pub cents: i64,
    pub ticked: bool,
}

#[derive(Clone, Debug)]
pub struct Trip {
    pub id: i64,
    pub date: String,
    pub purpose: String,
    pub tenths: i64,
    pub job: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Recurring {
    pub id: i64,
    pub every: String,
    pub next_date: String,
    pub summary: String,
}

#[derive(Clone, Debug)]
pub struct TimeEntry {
    pub id: i64,
    pub job_id: i64,
    pub date: String,
    pub minutes: i64,
    pub rate: i64,
    pub note: String,
}

impl TimeEntry {
    pub fn cents(&self) -> i64 {
        (self.minutes * self.rate + 30) / 60
    }
}

pub const EVERY: [&str; 3] = ["week", "month", "year"];

/// The next date after `date` for a repeat period. Monthly and yearly items land on `anchor_day`
/// (the day of the first occurrence) or the month's last day when it's shorter, so the 31st stays
/// the 31st after February instead of drifting to the 28th.
pub fn advance(date: &str, every: &str, anchor_day: u32) -> Option<String> {
    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    let n = match every {
        "week" => return d.checked_add_days(chrono::Days::new(7)).map(|n| n.format("%Y-%m-%d").to_string()),
        "month" => d.with_day(1)?.checked_add_months(Months::new(1))?,
        "year" => d.with_day(1)?.checked_add_months(Months::new(12))?,
        _ => return None,
    };
    let last = n.checked_add_months(Months::new(1))?.pred_opt()?.day();
    Some(n.with_day(anchor_day.clamp(1, last))?.format("%Y-%m-%d").to_string())
}

fn day_of(date: &str) -> u32 {
    NaiveDate::parse_from_str(date, "%Y-%m-%d").map(|d| d.day()).unwrap_or(1)
}

impl Books {
    // ---------- import rules ----------

    pub fn rules(&self) -> Result<Vec<Rule>> {
        let mut st = self.db.prepare("SELECT id, pattern, account_id, vendor_id, job_id FROM rules ORDER BY id")?;
        let rows = st.query_map([], |r| Ok(Rule { id: r.get(0)?, pattern: r.get(1)?, account: r.get(2)?, vendor: r.get(3)?, job: r.get(4)? }))?;
        rows.collect()
    }

    pub fn add_rule(&self, pattern: &str, account: i64, vendor: Option<i64>, job: Option<i64>) -> Result<()> {
        if pattern.trim().len() < 2 || account == 0 {
            return Err(bad("enter at least two characters to match and pick a category"));
        }
        self.db.execute(
            "INSERT INTO rules(pattern, account_id, vendor_id, job_id) VALUES (?1, ?2, ?3, ?4)",
            params![pattern.trim(), account, vendor, job],
        )?;
        Ok(())
    }

    pub fn delete_rule(&self, id: i64) -> Result<()> {
        self.db.execute("DELETE FROM rules WHERE id = ?1", [id])?;
        Ok(())
    }

    /// The first rule (oldest first) whose text appears in `memo`, ignoring case.
    pub(crate) fn rule_for(&self, memo: &str) -> Result<Option<Rule>> {
        let memo = memo.to_lowercase();
        Ok(self.rules()?.into_iter().find(|r| memo.contains(&r.pattern.to_lowercase())))
    }

    /// Runs the rules over everything still uncategorized. Returns how many changed.
    pub fn apply_rules(&self) -> Result<usize> {
        let uncat = [self.system_account("uncat_in")?, self.system_account("uncat_out")?];
        let mut st = self.db.prepare(
            "SELECT l.id, e.id, e.memo, e.job_id, e.vendor_id FROM lines l JOIN entries e ON e.id = l.entry_id
             WHERE l.account_id IN (?1, ?2)",
        )?;
        let todo: Vec<(i64, i64, String, Option<i64>, Option<i64>)> =
            st.query_map([uncat[0], uncat[1]], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.collect::<Result<_>>()?;
        let mut n = 0;
        for (line, entry, memo, job, vendor) in todo {
            if let Some(rule) = self.rule_for(&memo)? {
                // Locked periods are skipped rather than failing the whole run.
                if self.set_line_account(line, rule.account).is_ok() {
                    self.set_tags(entry, job.or(rule.job), vendor.or(rule.vendor))?;
                    n += 1;
                }
            }
        }
        Ok(n)
    }

    // ---------- reconciliation ----------

    /// Lines in `account` dated on or before `through` that aren't reconciled yet.
    pub fn recon_lines(&self, account: i64, through: &str) -> Result<Vec<ReconLine>> {
        let mut st = self.db.prepare(
            "SELECT l.id, e.date, e.memo, l.cents, l.cleared FROM lines l JOIN entries e ON e.id = l.entry_id
             WHERE l.account_id = ?1 AND l.cleared < 2 AND e.date <= ?2 ORDER BY e.date, e.id",
        )?;
        let rows = st.query_map(params![account, through], |r| {
            Ok(ReconLine { line_id: r.get(0)?, date: r.get(1)?, memo: r.get(2)?, cents: r.get(3)?, ticked: r.get::<_, i64>(4)? == 1 })
        })?;
        rows.collect()
    }

    pub fn tick(&self, line_id: i64, on: bool) -> Result<()> {
        self.db.execute("UPDATE lines SET cleared = ?1 WHERE id = ?2 AND cleared < 2", params![on as i64, line_id])?;
        Ok(())
    }

    /// Sum of reconciled plus ticked lines (raw debit-positive cents).
    pub fn cleared_balance(&self, account: i64) -> Result<i64> {
        self.db.query_row("SELECT COALESCE(SUM(cents), 0) FROM lines WHERE account_id = ?1 AND cleared >= 1", [account], |r| r.get(0))
    }

    /// Locks in the ticked lines once they match the statement. `ending` is raw debit-positive cents.
    pub fn finish_reconcile(&self, account: i64, date: &str, ending: i64) -> Result<()> {
        let cleared = self.cleared_balance(account)?;
        if cleared != ending {
            return Err(bad(format!("still off by {}", fmt_cents(ending - cleared))));
        }
        self.atomic(|| {
            self.db.execute("UPDATE lines SET cleared = 2 WHERE account_id = ?1 AND cleared = 1", [account])?;
            self.db.execute("INSERT INTO reconciliations(account_id, date, cents) VALUES (?1, ?2, ?3)", params![account, date, ending])?;
            Ok(())
        })
    }

    /// The most recent finished reconciliation: (statement date, raw ending cents).
    pub fn last_reconciled(&self, account: i64) -> Result<Option<(String, i64)>> {
        self.db
            .query_row("SELECT date, cents FROM reconciliations WHERE account_id = ?1 ORDER BY date DESC, id DESC LIMIT 1", [account], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()
    }

    // ---------- users (logins live in lock.rs; roles live here, inside the encrypted file) ----------

    pub fn users(&self) -> Result<Vec<(String, crate::lock::Role)>> {
        let mut st = self.db.prepare("SELECT name, role FROM users ORDER BY name")?;
        st.query_map([], |r| Ok((r.get(0)?, crate::lock::Role::parse(&r.get::<_, String>(1)?))))?.collect()
    }

    pub fn user_role(&self, name: &str) -> Result<Option<crate::lock::Role>> {
        self.db
            .query_row("SELECT role FROM users WHERE name = ?1", [name.trim()], |r| r.get::<_, String>(0))
            .optional()
            .map(|r| r.map(|s| crate::lock::Role::parse(&s)))
    }

    pub fn set_user(&self, name: &str, role: crate::lock::Role) -> Result<()> {
        // Never leave the books without an owner.
        if role != crate::lock::Role::Owner && self.user_role(name)? == Some(crate::lock::Role::Owner) && self.owners()? < 2 {
            return Err(bad("the books need at least one owner"));
        }
        self.db.execute("INSERT INTO users(name, role) VALUES(?1, ?2) ON CONFLICT(name) DO UPDATE SET role = ?2", [name.trim(), role.as_str()])?;
        Ok(())
    }

    pub fn remove_user(&self, name: &str) -> Result<()> {
        if self.user_role(name)? == Some(crate::lock::Role::Owner) && self.owners()? < 2 {
            return Err(bad("the books need at least one owner"));
        }
        self.db.execute("DELETE FROM users WHERE name = ?1", [name.trim()])?;
        Ok(())
    }

    fn owners(&self) -> Result<i64> {
        self.db.query_row("SELECT COUNT(*) FROM users WHERE role = 'owner'", [], |r| r.get(0))
    }

    // ---------- backups ----------

    /// A consistent copy of the whole books file (safe while the app is open).
    pub fn backup_to(&self, dest: &std::path::Path) -> Result<()> {
        let _ = std::fs::remove_file(dest); // VACUUM INTO refuses to overwrite
        self.db.execute("VACUUM INTO ?1", [dest.to_string_lossy()])?;
        Ok(())
    }

    // ---------- receipts ----------

    /// Stores (or replaces) the receipt for a transaction. Up to 10 MB of any file type.
    pub fn attach(&self, entry: i64, name: &str, data: &[u8]) -> Result<()> {
        if data.is_empty() || data.len() > 10_000_000 {
            return Err(bad("receipts must be under 10 MB"));
        }
        self.db.execute(
            "INSERT INTO attachments(entry_id, name, data) VALUES (?1, ?2, ?3) ON CONFLICT DO UPDATE SET name = ?2, data = ?3",
            params![entry, name, data],
        )?;
        Ok(())
    }

    pub fn attachment(&self, entry: i64) -> Result<Option<(String, Vec<u8>)>> {
        self.db.query_row("SELECT name, data FROM attachments WHERE entry_id = ?1", [entry], |r| Ok((r.get(0)?, r.get(1)?))).optional()
    }

    pub fn remove_attachment(&self, entry: i64) -> Result<()> {
        self.db.execute("DELETE FROM attachments WHERE entry_id = ?1", [entry])?;
        Ok(())
    }

    // ---------- mileage ----------

    pub fn add_trip(&self, date: &str, purpose: &str, tenths: i64, job: Option<i64>) -> Result<()> {
        if tenths <= 0 || purpose.trim().is_empty() {
            return Err(bad("enter the miles and what the trip was for"));
        }
        self.db.execute("INSERT INTO trips(date, purpose, tenths, job_id) VALUES (?1, ?2, ?3, ?4)", params![date, purpose.trim(), tenths, job])?;
        Ok(())
    }

    pub fn trips(&self, from: &str, to: &str) -> Result<Vec<Trip>> {
        let mut st = self.db.prepare(
            "SELECT t.id, t.date, t.purpose, t.tenths, j.name FROM trips t LEFT JOIN jobs j ON j.id = t.job_id
             WHERE t.date BETWEEN ?1 AND ?2 ORDER BY t.date DESC, t.id DESC",
        )?;
        let rows = st.query_map([from, to], |r| Ok(Trip { id: r.get(0)?, date: r.get(1)?, purpose: r.get(2)?, tenths: r.get(3)?, job: r.get(4)? }))?;
        rows.collect()
    }

    pub fn delete_trip(&self, id: i64) -> Result<()> {
        self.db.execute("DELETE FROM trips WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Standard-mileage deduction for trips in the range, in cents.
    pub fn mileage_deduction(&self, from: &str, to: &str) -> Result<i64> {
        let rate: i64 = self.db.query_row("SELECT mileage_rate FROM settings WHERE id = 1", [], |r| r.get(0))?;
        let tenths: i64 = self.db.query_row("SELECT COALESCE(SUM(tenths), 0) FROM trips WHERE date BETWEEN ?1 AND ?2", [from, to], |r| r.get(0))?;
        Ok((tenths * rate + 5) / 10)
    }

    // ---------- recurring ----------

    /// Repeats a money in/out transaction that was just posted on `first_date`.
    #[allow(clippy::too_many_arguments)]
    pub fn add_recurring_txn(
        &self,
        every: &str,
        first_date: &str,
        memo: &str,
        bank: i64,
        category: i64,
        cents: i64,
        job: Option<i64>,
        vendor: Option<i64>,
    ) -> Result<()> {
        let data = serde_json::json!({ "memo": memo, "bank": bank, "category": category, "cents": cents, "job": job, "vendor": vendor });
        self.add_recurring("txn", every, first_date, data)
    }

    /// Repeats an invoice that was just created on `d.date`; `terms_days` sets each copy's due date.
    pub fn add_recurring_invoice(&self, every: &str, d: &DocInput, terms_days: i64) -> Result<()> {
        let data = serde_json::json!({ "customer": d.customer_id, "items": d.items, "taxable": d.taxable, "customer_po": d.customer_po, "job": d.job, "tax_bp": d.tax_bp, "terms_days": terms_days });
        self.add_recurring("invoice", every, &d.date, data)
    }

    fn add_recurring(&self, kind: &str, every: &str, first_date: &str, mut data: serde_json::Value) -> Result<()> {
        if !EVERY.contains(&every) {
            return Err(bad("pick weekly, monthly or yearly"));
        }
        let anchor = day_of(first_date);
        data["anchor_day"] = anchor.into();
        let next_date = advance(first_date, every, anchor).ok_or_else(|| bad("bad date"))?;
        self.db.execute(
            "INSERT INTO recurring(kind, every, next_date, data) VALUES (?1, ?2, ?3, ?4)",
            params![kind, every, next_date, data.to_string()],
        )?;
        Ok(())
    }

    pub fn recurring(&self) -> Result<Vec<Recurring>> {
        let mut st = self.db.prepare("SELECT id, kind, every, next_date, data FROM recurring ORDER BY next_date")?;
        let rows = st.query_map([], |r| {
            let data: String = r.get(4)?;
            let v: serde_json::Value = serde_json::from_str(&data).unwrap_or_default();
            let kind: String = r.get(1)?;
            let summary = if kind == "txn" {
                format!("{} {}", v["memo"].as_str().unwrap_or(""), fmt_cents(v["cents"].as_i64().unwrap_or(0)))
            } else {
                let total: i64 = v["items"].as_array().into_iter().flatten().filter_map(|i| i[1].as_i64()).sum();
                format!("Invoice, {} before tax", fmt_cents(total))
            };
            Ok(Recurring { id: r.get(0)?, every: r.get(2)?, next_date: r.get(3)?, summary })
        })?;
        rows.collect()
    }

    pub fn delete_recurring(&self, id: i64) -> Result<()> {
        self.db.execute("DELETE FROM recurring WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Posts every occurrence due on or before `today` (catching up missed ones) and moves each
    /// schedule forward. Returns how many were posted.
    pub fn run_recurring(&self, today: &str) -> std::result::Result<usize, String> {
        let due: Vec<(i64, String, String, String, String)> = self
            .db
            .prepare("SELECT id, kind, every, next_date, data FROM recurring WHERE next_date <= ?1")
            .and_then(|mut st| st.query_map([today], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.collect())
            .map_err(|e| e.to_string())?;
        let (mut posted, mut problems) = (0, vec![]);
        for (id, kind, every, mut next, data) in due {
            let v: serde_json::Value = serde_json::from_str(&data).map_err(|e| e.to_string())?;
            let opt = |k: &str| v[k].as_i64();
            let anchor = v["anchor_day"].as_u64().map_or_else(|| day_of(&next), |a| a as u32);
            while next.as_str() <= today {
                // A date inside the locked period can't be posted; move past it instead of retrying forever.
                let r = if self.check_open(&next).is_err() {
                    Ok(())
                } else if kind == "txn" {
                    let (bank, cat, cents) = (opt("bank").unwrap_or(0), opt("category").unwrap_or(0), opt("cents").unwrap_or(0));
                    self.post_simple(&next, v["memo"].as_str().unwrap_or(""), bank, cat, cents)
                        .and_then(|e| self.set_tags(e, opt("job"), opt("vendor")))
                } else {
                    let items: Vec<Item> = serde_json::from_value(v["items"].clone()).map_err(|e| e.to_string())?;
                    let terms = opt("terms_days").unwrap_or(0);
                    let due = NaiveDate::parse_from_str(&next, "%Y-%m-%d")
                        .ok()
                        .and_then(|d| d.checked_add_days(chrono::Days::new(terms.max(0) as u64)))
                        .map(|d| d.format("%Y-%m-%d").to_string())
                        .unwrap_or_default();
                    let d = DocInput {
                        customer_id: opt("customer").unwrap_or(0),
                        date: next.clone(),
                        items,
                        job: opt("job"),
                        tax_bp: opt("tax_bp").unwrap_or(0),
                        due_date: if terms > 0 { due } else { String::new() },
                        taxable: serde_json::from_value(v["taxable"].clone()).unwrap_or_default(),
                        customer_po: v["customer_po"].as_str().unwrap_or("").to_string(),
                    };
                    self.create_invoice(&d).map(|_| ())
                };
                match r {
                    Ok(()) if self.check_open(&next).is_ok() => posted += 1,
                    Ok(()) => {}
                    Err(e) => {
                        // Leave this schedule where it is and carry on with the others.
                        problems.push(format!("recurring item #{id} on {next}: {e}"));
                        break;
                    }
                }
                next = advance(&next, &every, anchor).ok_or("bad recurring date")?;
                self.db.execute("UPDATE recurring SET next_date = ?1 WHERE id = ?2", params![next, id]).map_err(|e| e.to_string())?;
            }
        }
        if problems.is_empty() { Ok(posted) } else { Err(format!("posted {posted}, but {}", problems.join("; "))) }
    }

    // ---------- time tracking ----------

    pub fn add_time(&self, job: i64, date: &str, minutes: i64, rate: i64, note: &str) -> Result<()> {
        if job == 0 || minutes <= 0 || rate < 0 {
            return Err(bad("pick a job and enter the hours"));
        }
        self.db.execute(
            "INSERT INTO time_entries(job_id, date, minutes, rate, note) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![job, date, minutes, rate, note.trim()],
        )?;
        Ok(())
    }

    /// Time not yet on an invoice, oldest first.
    pub fn unbilled_time(&self) -> Result<Vec<TimeEntry>> {
        let mut st = self.db.prepare(
            "SELECT t.id, t.job_id, j.name, t.date, t.minutes, t.rate, t.note FROM time_entries t JOIN jobs j ON j.id = t.job_id
             WHERE t.invoice_id IS NULL ORDER BY t.date, t.id",
        )?;
        let rows = st.query_map([], |r| {
            Ok(TimeEntry { id: r.get(0)?, job_id: r.get(1)?, date: r.get(3)?, minutes: r.get(4)?, rate: r.get(5)?, note: r.get(6)? })
        })?;
        rows.collect()
    }

    pub fn delete_time(&self, id: i64) -> Result<()> {
        self.db.execute("DELETE FROM time_entries WHERE id = ?1 AND invoice_id IS NULL", [id])?;
        Ok(())
    }

    /// Bills all unbilled time on a job to the job's customer as one invoice, one line per entry.
    pub fn invoice_time(&self, job: i64, date: &str, due_date: &str, tax_bp: i64) -> Result<i64> {
        let cust: Option<i64> = self.db.query_row("SELECT customer_id FROM jobs WHERE id = ?1", [job], |r| r.get(0))?;
        let cust = cust.ok_or_else(|| bad("give this job a customer first (Jobs screen)"))?;
        let entries: Vec<TimeEntry> = self.unbilled_time()?.into_iter().filter(|t| t.job_id == job).collect();
        if entries.is_empty() {
            return Err(bad("no unbilled time on this job"));
        }
        let items: Vec<Item> = entries
            .iter()
            .map(|t| {
                let what = if t.note.is_empty() { "Labor" } else { &t.note };
                (format!("{} {what} ({:.2} h @ {}/h)", t.date, t.minutes as f64 / 60.0, fmt_cents(t.rate)), t.cents())
            })
            .collect();
        self.atomic(|| {
            let d = DocInput { customer_id: cust, date: date.into(), items, job: Some(job), tax_bp, due_date: due_date.into(), ..Default::default() };
            let inv = self.create_invoice(&d)?;
            for t in &entries {
                self.db.execute("UPDATE time_entries SET invoice_id = ?1 WHERE id = ?2", params![inv, t.id])?;
            }
            Ok(inv)
        })
    }
}

/// Something the business owns and uses for years (a table saw, a truck, a laptop).
#[derive(Clone, Debug, Default)]
pub struct Asset {
    pub id: i64,
    pub name: String,
    /// The asset account it belongs to (Equipment, Vehicles, ...), which is its category.
    pub account: i64,
    pub category: String,
    pub purchased: String,
    pub cost: i64,
    /// Straight-line useful life; 0 means not depreciated (land, for example).
    pub life_years: i64,
    pub serial: String,
    pub notes: String,
    pub disposed: String,
    /// Its category is intangible (software, website): amortized, not depreciated.
    pub intangible: bool,
    /// A photo is stored (for insurance).
    pub photo: bool,
}

impl Asset {
    /// Straight-line depreciation from the purchase month through `date`, by whole months, capped at cost.
    pub fn depreciation_through(&self, date: &str) -> i64 {
        if self.life_years <= 0 || date < self.purchased.as_str() {
            return 0;
        }
        let ym = |d: &str| d.get(..4).and_then(|y| y.parse::<i64>().ok()).unwrap_or(0) * 12 + d.get(5..7).and_then(|m| m.parse::<i64>().ok()).unwrap_or(1);
        let end = if !self.disposed.is_empty() && self.disposed.as_str() < date { self.disposed.as_str() } else { date };
        let months = (ym(end) - ym(&self.purchased) + 1).clamp(0, self.life_years * 12);
        self.cost * months / (self.life_years * 12)
    }
}

impl Books {
    pub fn assets(&self) -> Result<Vec<Asset>> {
        let mut st = self.db.prepare(
            "SELECT s.id, s.name, s.account_id, a.name, s.purchased, s.cost, s.life_years, s.serial, s.notes, s.disposed,
               EXISTS (SELECT 1 FROM asset_photos p WHERE p.asset_id = s.id), a.intangible
             FROM assets s JOIN accounts a ON a.id = s.account_id ORDER BY a.number, s.purchased",
        )?;
        let rows = st.query_map([], |r| {
            Ok(Asset {
                id: r.get(0)?,
                name: r.get(1)?,
                account: r.get(2)?,
                category: r.get(3)?,
                purchased: r.get(4)?,
                cost: r.get(5)?,
                life_years: r.get(6)?,
                serial: r.get(7)?,
                notes: r.get(8)?,
                disposed: r.get(9)?,
                photo: r.get(10)?,
                intangible: r.get(11)?,
            })
        })?;
        rows.collect()
    }

    /// Adds an asset to the list. With `paid_from`, also books the purchase (Dr the asset account, Cr the bank/card).
    pub fn add_asset(&self, a: &Asset, paid_from: Option<i64>) -> Result<i64> {
        if a.name.trim().is_empty() || a.cost <= 0 || a.account == 0 || !(0..=50).contains(&a.life_years) {
            return Err(bad("enter a name, a category, the cost, and a useful life of 0 to 50 years"));
        }
        self.atomic(|| {
            let entry = match paid_from {
                Some(bank) => Some(self.post(&a.purchased, &format!("Bought {}", a.name.trim()), &[(a.account, a.cost), (bank, -a.cost)])?),
                None => None,
            };
            self.db.execute(
                "INSERT INTO assets(name, account_id, purchased, cost, life_years, serial, notes, entry_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![a.name.trim(), a.account, a.purchased, a.cost, a.life_years, a.serial.trim(), a.notes.trim(), entry],
            )?;
            Ok(self.db.last_insert_rowid())
        })
    }

    /// Saves the descriptive fields (name, serial, notes, life, disposed date). Cost and category stay as booked.
    pub fn update_asset(&self, a: &Asset) -> Result<()> {
        if a.name.trim().is_empty() || !(0..=50).contains(&a.life_years) {
            return Err(bad("an asset needs a name and a useful life of 0 to 50 years"));
        }
        self.db.execute(
            "UPDATE assets SET name = ?1, life_years = ?2, serial = ?3, notes = ?4, disposed = ?5 WHERE id = ?6",
            params![a.name.trim(), a.life_years, a.serial.trim(), a.notes.trim(), a.disposed, a.id],
        )?;
        Ok(())
    }

    /// Removes it from the list only; any purchase already booked stays in the books.
    pub fn delete_asset(&self, id: i64) -> Result<()> {
        self.atomic(|| {
            self.db.execute("DELETE FROM asset_photos WHERE asset_id = ?1", [id])?;
            self.db.execute("DELETE FROM assets WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    /// One photo per asset, kept inside the books file so it travels with backups.
    pub fn set_asset_photo(&self, id: i64, name: &str, data: &[u8]) -> Result<()> {
        self.db.execute(
            "INSERT INTO asset_photos(asset_id, name, data) VALUES (?1, ?2, ?3) ON CONFLICT DO UPDATE SET name = ?2, data = ?3",
            params![id, name, data],
        )?;
        Ok(())
    }

    pub fn asset_photo(&self, id: i64) -> Result<Option<(String, Vec<u8>)>> {
        self.db.query_row("SELECT name, data FROM asset_photos WHERE asset_id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()
    }

    pub fn remove_asset_photo(&self, id: i64) -> Result<()> {
        self.db.execute("DELETE FROM asset_photos WHERE asset_id = ?1", [id])?;
        Ok(())
    }

    /// Depreciation that `post_depreciation(year)` would book.
    pub fn depreciation_for_year(&self, year: i32) -> Result<i64> {
        let (d, a) = self.depreciation_split(year)?;
        Ok(d + a)
    }

    /// (depreciation of physical assets, amortization of intangibles) for a year.
    fn depreciation_split(&self, year: i32) -> Result<(i64, i64)> {
        let (end, prev) = (format!("{year}-12-31"), format!("{}-12-31", year - 1));
        let (mut dep, mut amort) = (0, 0);
        for a in self.assets()? {
            let v = a.depreciation_through(&end) - a.depreciation_through(&prev);
            if a.intangible { amort += v } else { dep += v }
        }
        Ok((dep, amort))
    }

    /// Books one year's straight-line depreciation for every asset (Dr Depreciation, Cr Accumulated
    /// Depreciation) on Dec 31. Each year can be booked once.
    pub fn post_depreciation(&self, year: i32) -> Result<i64> {
        if self.db.prepare("SELECT 1 FROM depreciation_posts WHERE year = ?1")?.exists([year])? {
            return Err(bad(format!("{year} depreciation is already booked")));
        }
        let (dep, amort) = self.depreciation_split(year)?;
        let total = dep + amort;
        if total <= 0 {
            return Err(bad(format!("nothing to depreciate for {year}")));
        }
        let mut lines = vec![];
        if dep > 0 {
            lines.extend([(self.system_account("depreciation")?, dep), (self.system_account("accum_dep")?, -dep)]);
        }
        if amort > 0 {
            lines.extend([(self.system_account("amortization")?, amort), (self.system_account("accum_amort")?, -amort)]);
        }
        let memo = match (dep > 0, amort > 0) {
            (true, true) => format!("{year} depreciation and amortization"),
            (false, true) => format!("{year} amortization"),
            _ => format!("{year} depreciation"),
        };
        self.atomic(|| {
            let entry = self.post(&format!("{year}-12-31"), &memo, &lines)?;
            self.db.execute("INSERT INTO depreciation_posts(year, entry_id) VALUES (?1, ?2)", params![year, entry])?;
            Ok(total)
        })
    }

    pub fn depreciation_booked(&self, year: i32) -> Result<bool> {
        self.db.prepare("SELECT 1 FROM depreciation_posts WHERE year = ?1")?.exists([year])
    }
}

/// Copies the books file into a `backups` folder beside it, named by date and time, keeping the newest 30.
pub fn auto_backup(books: &Books, db_path: &str) -> std::result::Result<std::path::PathBuf, String> {
    let path = std::path::Path::new(db_path);
    let dir = path.parent().unwrap_or(std::path::Path::new(".")).join("backups");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("books");
    let dest = dir.join(format!("{stem}-{}.db", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    books.backup_to(&dest).map_err(|e| e.to_string())?;
    // Encrypted books: the backup opens with the same logins, so keep a copy of the keys beside it.
    let keys = crate::lock::keys_path(db_path);
    if keys.exists() {
        std::fs::copy(&keys, crate::lock::keys_path(&dest.to_string_lossy())).map_err(|e| e.to_string())?;
    }
    let mut old: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        // Exactly "{stem}-YYYYMMDD-HHMMSS.db", so "acme-llc" backups don't count as "acme" ones.
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_prefix(&format!("{stem}-"))?.strip_suffix(".db")).is_some_and(|t| {
                t.len() == 15 && t.chars().enumerate().all(|(i, c)| if i == 8 { c == '-' } else { c.is_ascii_digit() })
            })
        })
        .collect();
    old.sort(); // timestamped names sort oldest first
    for p in old.iter().take(old.len().saturating_sub(30)) {
        let _ = std::fs::remove_file(p);
        let _ = std::fs::remove_file(crate::lock::keys_path(&p.to_string_lossy()));
    }
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::books::tests::doc;

    fn acct(b: &Books, n: &str) -> i64 {
        b.accounts().unwrap().into_iter().find(|a| a.name == n).unwrap().id
    }

    #[test]
    fn rules_reconcile_recurring_time_trips_lock() {
        let b = Books::open(":memory:").unwrap();
        let (chk, fuel) = (acct(&b, "Checking"), acct(&b, "Car & Truck"));
        let shell = b.add_vendor("Shell", "").unwrap();

        // Rules apply on import and retroactively.
        b.import_csv("Date,Desc,Amt\n01/02/2026,COFFEE,-4.00\n", chk, true, 0, 1, 2).unwrap();
        b.add_rule("shell", fuel, Some(shell), None).unwrap();
        b.import_csv("Date,Desc,Amt\n01/03/2026,SHELL OIL 123,-40.00\n", chk, true, 0, 1, 2).unwrap();
        let e = b.entries().unwrap();
        let gas = e.iter().find(|e| e.memo.starts_with("SHELL")).unwrap();
        assert_eq!((gas.lines[1].1, gas.vendor), (fuel, Some(shell)));
        b.add_rule("coffee", acct(&b, "Meals"), None, None).unwrap();
        assert_eq!(b.apply_rules().unwrap(), 1);

        // Reconcile: tick both lines, match a statement of -$44.00.
        let lines = b.recon_lines(chk, "2026-01-31").unwrap();
        assert_eq!(lines.len(), 2);
        b.tick(lines[0].line_id, true).unwrap();
        assert!(b.finish_reconcile(chk, "2026-01-31", -4_400).is_err(), "one line short");
        b.tick(lines[1].line_id, true).unwrap();
        b.finish_reconcile(chk, "2026-01-31", -4_400).unwrap();
        assert!(b.recon_lines(chk, "2026-01-31").unwrap().is_empty());
        assert_eq!(b.last_reconciled(chk).unwrap(), Some(("2026-01-31".into(), -4_400)));

        // Recurring rent catches up three months.
        b.add_recurring_txn("month", "2025-12-31", "Rent", chk, acct(&b, "Rent or Lease"), -100_000, None, None).unwrap();
        assert_eq!(b.run_recurring("2026-03-31").unwrap(), 3);
        assert_eq!(b.recurring().unwrap()[0].next_date, "2026-04-30");
        assert_eq!(b.run_recurring("2026-03-31").unwrap(), 0);
        let cust = b.add_customer("Acme", "").unwrap();
        b.add_recurring_invoice("month", &doc(cust, "2026-01-01", &[("Retainer".into(), 50_000)], None), 15).unwrap();
        assert_eq!(b.run_recurring("2026-02-10").unwrap(), 1);
        assert_eq!(b.invoices().unwrap()[0].due_date, "2026-02-16");

        // Time billed to the job's customer.
        b.add_job("Deck", Some(cust)).unwrap();
        let job = b.jobs().unwrap()[0].id;
        b.add_time(job, "2026-02-03", 150, 6_000, "Framing").unwrap();
        b.add_time(job, "2026-02-04", 60, 6_000, "").unwrap();
        let inv = b.invoice_time(job, "2026-02-05", "", 0).unwrap();
        assert_eq!(b.invoices().unwrap().iter().find(|i| i.id == inv).unwrap().cents, 21_000);
        assert!(b.unbilled_time().unwrap().is_empty());

        // Mileage at 70 cents.
        b.add_trip("2026-02-03", "Lumber yard", 125, Some(job)).unwrap();
        assert_eq!(b.mileage_deduction("2026-01-01", "2026-12-31").unwrap(), 875);

        // Lock date blocks changes on or before it.
        let mut s = b.settings().unwrap();
        s.lock_date = "2026-01-31".into();
        b.save_prefs(&s).unwrap();
        assert!(b.post_simple("2026-01-15", "late", chk, fuel, -100).is_err());
        assert!(b.delete_entry(gas.id).is_err());
        b.post_simple("2026-02-15", "ok", chk, fuel, -100).unwrap();

        // Receipts.
        let ok = b.entries().unwrap()[0].id;
        b.attach(ok, "r.pdf", b"%PDF").unwrap();
        assert!(b.entries().unwrap()[0].receipt);
        assert_eq!(b.attachment(ok).unwrap().unwrap().0, "r.pdf");
    }

    #[test]
    fn assets_and_depreciation() {
        let b = Books::open(":memory:").unwrap();
        let (chk, equip) = (acct(&b, "Checking"), acct(&b, "Equipment"));
        let saw = Asset { name: "Table saw".into(), account: equip, purchased: "2026-07-10".into(), cost: 120_000, life_years: 5, ..Default::default() };
        b.add_asset(&saw, Some(chk)).unwrap();
        assert!(b.add_asset(&Asset { cost: 0, ..saw.clone() }, None).is_err());
        let a = &b.assets().unwrap()[0];
        assert_eq!((a.category.as_str(), a.depreciation_through("2026-12-31")), ("Equipment", 12_000), "6 of 60 months");
        assert_eq!(a.depreciation_through("2040-01-01"), 120_000, "never more than cost");
        assert_eq!(b.depreciation_for_year(2027).unwrap(), 24_000);
        assert_eq!(b.post_depreciation(2026).unwrap(), 12_000);
        assert!(b.post_depreciation(2026).is_err(), "once per year");
        let bs = b.balance_sheet("2026-12-31").unwrap();
        let assets: i64 = bs.rows.iter().filter(|r| r.0 == "asset").map(|r| r.2).sum();
        assert_eq!(assets, 0 - 120_000 + 120_000 - 12_000, "cash out, saw in, less depreciation");

        // Software is intangible: amortized into its own accounts, in the same year-end entry.
        let sw = acct(&b, "Software");
        assert!(b.accounts().unwrap().iter().any(|a| a.id == sw && a.intangible));
        b.add_asset(&Asset { name: "CAD license".into(), account: sw, purchased: "2027-01-01".into(), cost: 36_000, life_years: 3, ..Default::default() }, None).unwrap();
        assert_eq!(b.post_depreciation(2027).unwrap(), 24_000 + 12_000);
        let amort = acct(&b, "Accumulated Amortization");
        let bal: i64 = b.db.query_row("SELECT SUM(cents) FROM lines WHERE account_id = ?1", [amort], |r| r.get(0)).unwrap();
        assert_eq!(bal, -12_000);
    }

    #[test]
    fn advance_keeps_the_day() {
        assert_eq!(advance("2026-01-31", "month", 31).as_deref(), Some("2026-02-28"));
        assert_eq!(advance("2026-02-28", "month", 31).as_deref(), Some("2026-03-31"));
        assert_eq!(advance("2024-02-29", "year", 29).as_deref(), Some("2025-02-28"));
        assert_eq!(advance("2026-12-28", "week", 28).as_deref(), Some("2027-01-04"));
    }

    #[test]
    fn backups_keep_thirty() {
        let dir = std::env::temp_dir().join(format!("ac-bk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("backups")).unwrap();
        for i in 0..32 {
            std::fs::write(dir.join("backups").join(format!("books-20000101-0000{i:02}.db")), b"x").unwrap();
        }
        let db = dir.join("books.db");
        let b = Books::open(db.to_str().unwrap()).unwrap();
        let made = auto_backup(&b, db.to_str().unwrap()).unwrap();
        assert!(Books::open(made.to_str().unwrap()).unwrap().accounts().unwrap().len() > 10, "backup opens");
        assert_eq!(std::fs::read_dir(dir.join("backups")).unwrap().count(), 30);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
