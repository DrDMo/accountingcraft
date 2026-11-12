//! The ledger: double-entry books in one SQLite file.
//! Every entry's lines sum to zero. Debits are positive, credits negative, amounts in cents.

use chrono::NaiveDate;
use rusqlite::{params, Connection, OptionalExtension, Result};

pub const KINDS: [&str; 5] = ["asset", "liability", "equity", "income", "expense"];

#[derive(Clone, Debug)]
pub struct Account {
    pub id: i64,
    pub number: String,
    pub name: String,
    pub kind: String,
    pub description: String,
    pub active: bool,
    /// Accounts the app posts to by itself ("ar", "sales", "uncat_in", "uncat_out"): renamable, never removable.
    pub system: Option<String>,
    /// How many transaction lines use it; used accounts can't change kind or be deleted.
    pub used: i64,
    /// An asset category for intangibles (software, websites): amortized instead of depreciated.
    pub intangible: bool,
}

/// The standard chart: (number, name, kind, system key). Names match the original short list so
/// older books gain the rest without duplicates. Expense names follow IRS Schedule C lines.
const DEFAULT_ACCOUNTS: &[(&str, &str, &str, Option<&str>)] = &[
    ("1000", "Checking", "asset", None),
    ("1010", "Savings", "asset", None),
    ("1050", "Petty Cash", "asset", None),
    ("1080", "Undeposited Funds", "asset", None),
    ("1100", "Accounts Receivable", "asset", Some("ar")),
    ("1200", "Inventory", "asset", None),
    ("1300", "Prepaid Expenses", "asset", None),
    ("1400", "Security Deposits", "asset", None),
    ("1500", "Equipment", "asset", None),
    ("1510", "Furniture & Fixtures", "asset", None),
    ("1520", "Vehicles", "asset", None),
    ("1530", "Computers", "asset", None),
    ("1540", "Software", "asset", None),
    ("1550", "Website & Digital Assets", "asset", None),
    ("1590", "Accumulated Depreciation", "asset", Some("accum_dep")),
    ("1595", "Accumulated Amortization", "asset", Some("accum_amort")),
    ("2000", "Accounts Payable", "liability", Some("ap")),
    ("2100", "Credit Card", "liability", None),
    ("2200", "Sales Tax Payable", "liability", Some("sales_tax")),
    ("2300", "Payroll Liabilities", "liability", None),
    ("2400", "Customer Deposits", "liability", Some("deposits")),
    ("2500", "Loans Payable", "liability", None),
    ("2600", "Vehicle Loan", "liability", None),
    ("3000", "Owner's Contributions", "equity", None),
    ("3100", "Owner's Draw", "equity", None),
    ("3200", "Retained Earnings", "equity", None),
    ("3900", "Opening Balance Equity", "equity", None),
    ("4000", "Sales", "income", Some("sales")),
    ("4100", "Service Income", "income", None),
    ("4200", "Product Sales", "income", None),
    ("4300", "Shipping & Delivery Income", "income", None),
    ("4400", "Refunds & Discounts Given", "income", None),
    ("4800", "Interest Income", "income", None),
    ("4900", "Other Income", "income", None),
    ("4990", "Uncategorized Income", "income", Some("uncat_in")),
    ("5000", "Cost of Goods Sold", "expense", None),
    ("5100", "Job Materials", "expense", None),
    ("5200", "Subcontractors", "expense", None),
    ("5300", "Equipment Rental for Jobs", "expense", None),
    ("6000", "Advertising", "expense", None),
    ("6050", "Bank Fees", "expense", None),
    ("6100", "Car & Truck", "expense", None),
    ("6150", "Commissions & Fees", "expense", None),
    ("6200", "Contract Labor", "expense", None),
    ("6250", "Depreciation", "expense", Some("depreciation")),
    ("6260", "Amortization", "expense", Some("amortization")),
    ("6300", "Dues & Memberships", "expense", None),
    ("6350", "Education & Training", "expense", None),
    ("6400", "Insurance", "expense", None),
    ("6420", "Health Insurance", "expense", None),
    ("6450", "Interest Expense", "expense", None),
    ("6500", "Legal & Professional Fees", "expense", None),
    ("6550", "Licenses & Permits", "expense", None),
    ("6600", "Meals", "expense", None),
    ("6650", "Office Supplies", "expense", None),
    ("6700", "Postage & Shipping", "expense", None),
    ("6750", "Rent or Lease", "expense", None),
    ("6800", "Repairs & Maintenance", "expense", None),
    ("6850", "Software & Subscriptions", "expense", None),
    ("6900", "Taxes & Licenses", "expense", None),
    ("6950", "Phone & Internet", "expense", None),
    ("7000", "Tools & Small Equipment", "expense", None),
    ("7050", "Travel", "expense", None),
    ("7100", "Utilities", "expense", None),
    ("7150", "Wages", "expense", None),
    ("7200", "Payroll Taxes", "expense", None),
    ("7250", "Employee Benefits", "expense", None),
    ("7300", "Home Office", "expense", None),
    ("7400", "Gifts", "expense", None),
    ("7900", "Other Expenses", "expense", None),
    ("7990", "Uncategorized Expense", "expense", Some("uncat_out")),
];

#[derive(Clone, Debug)]
pub struct Entry {
    pub id: i64,
    pub date: String,
    pub memo: String,
    pub lines: Vec<(i64, i64, i64)>, // (line id, account id, cents)
    pub job: Option<i64>,
    pub vendor: Option<i64>,
    pub receipt: bool,
    /// Part of an invoice or payment, so it's changed from the invoice side, not edited directly.
    pub invoice: bool,
}

/// A job's totals: everything tagged with it, from invoices and expenses alike.
#[derive(Clone, Debug, PartialEq)]
pub struct JobTotals {
    pub id: i64,
    pub name: String,
    pub customer: Option<String>,
    pub customer_id: Option<i64>,
    pub income: i64,
    pub expenses: i64,
    /// Total of the job's quotes before tax, to compare against what was actually billed.
    pub quoted: i64,
}

#[derive(Clone, Debug)]
pub struct Invoice {
    pub id: i64,
    pub number: i64,
    pub customer_id: i64,
    pub customer: String,
    pub date: String,
    pub description: String,
    /// Total including sales tax.
    pub cents: i64,
    pub paid_cents: i64,
    pub due_date: String,
    pub job: Option<i64>,
    pub paid: bool,
}

impl Invoice {
    pub fn balance(&self) -> i64 {
        self.cents - self.paid_cents
    }
    pub fn overdue(&self, today: &str) -> bool {
        !self.paid && !self.due_date.is_empty() && self.due_date.as_str() < today
    }
}

/// What the invoice/quote form submits.
#[derive(Clone, Debug, Default)]
pub struct DocInput {
    pub customer_id: i64,
    pub date: String,
    pub items: Vec<Item>,
    pub job: Option<i64>,
    /// Sales tax in basis points (8.25% = 825), applied to the whole subtotal.
    pub tax_bp: i64,
    /// Empty means due on receipt.
    pub due_date: String,
    /// Which items are taxable, by position; missing entries count as taxable.
    pub taxable: Vec<bool>,
    /// The customer's own purchase order number, printed on the invoice.
    pub customer_po: String,
}

impl DocInput {
    pub fn taxable_subtotal(&self) -> i64 {
        self.items.iter().enumerate().filter(|(i, _)| self.taxable.get(*i).copied().unwrap_or(true)).map(|(_, it)| it.1).sum()
    }
}

/// A customer or vendor. `address` is the printed mailing block (built from `info` on save);
/// `info.name` is the company / legal name, `info.extra` private notes; `is_1099` and `tax_id` are for vendors.
#[derive(Clone, Debug, Default)]
pub struct Party {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub address: String,
    pub is_1099: bool,
    pub info: Business,
    pub tax_id: String,
}

#[derive(Clone, Debug)]
pub struct Quote {
    pub id: i64,
    pub customer_id: i64,
    pub customer: String,
    pub date: String,
    pub cents: i64,
    pub invoice_id: Option<i64>,
    pub job: Option<String>,
    pub job_id: Option<i64>,
}

/// One line on an invoice or quote: (description, cents).
pub type Item = (String, i64);

/// Your business's contact details. `extra` is the old free-text address box, still printed after the rest.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Business {
    pub name: String,
    /// Trade names ("doing business as"), comma-separated.
    pub dba: String,
    pub contact: String,
    pub street: String,
    pub street2: String,
    pub city: String,
    pub state: String,
    pub zip: String,
    pub phone: String,
    pub phone2: String,
    pub email: String,
    pub website: String,
    pub extra: String,
}

impl Business {
    /// The lines printed under the business name on invoices, quotes, POs and proposals.
    pub fn letterhead(&self) -> String {
        let mut v = self.mailing();
        v.extend([&self.phone, &self.phone2, &self.email, &self.website, &self.extra].iter().map(|l| l.trim().to_string()));
        v.retain(|l| !l.is_empty());
        v.join("\n")
    }

    /// Street, suite and "City, ST ZIP", skipping blanks.
    pub fn mailing(&self) -> Vec<String> {
        let csz = format!("{}{}{} {}", self.city, if self.city.is_empty() || self.state.is_empty() { "" } else { ", " }, self.state, self.zip);
        [&self.street, &self.street2, &csz]
            .iter()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Settings {
    pub name: String,
    /// The printed letterhead lines (from `biz`).
    pub address: String,
    pub biz: Business,
    pub logo: Option<Vec<u8>>,
    pub simplefin_url: Option<String>,
    /// Entries dated on or before this can't change (empty = unlocked).
    pub lock_date: String,
    /// Shown as a "Pay online" button on invoices (Stripe/PayPal/Square payment link).
    pub pay_link: String,
    /// IRS standard mileage rate, cents per mile.
    pub mileage_rate: i64,
    /// Default hourly rate for time tracking, cents.
    pub hourly_rate: i64,
    /// Rough income-tax rate for the estimated-tax helper, percent.
    pub income_tax_pct: i64,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Doc {
    Invoice,
    Quote,
}

pub struct Report {
    pub rows: Vec<(String, String, i64)>, // (kind, account, cents shown with natural sign)
}

pub struct Books {
    pub db: Connection,
}

impl Books {
    #[cfg(test)] // the app always opens through lock.rs
    pub fn open(path: &str) -> Result<Self> {
        Self::open_key(path, None)
    }

    /// Opens a books file encrypted with `key` (SQLCipher raw key); None opens a plain file.
    pub fn open_key(path: &str, key: Option<&[u8; 32]>) -> Result<Self> {
        let db = Connection::open(path)?;
        if let Some(k) = key {
            db.execute_batch(&format!("PRAGMA key = \"x'{}'\";", crate::lock::hex(k)))?;
        }
        db.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS users(name TEXT PRIMARY KEY COLLATE NOCASE, role TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS accounts(
               id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE,
               kind TEXT NOT NULL CHECK(kind IN ('asset','liability','equity','income','expense')));
             CREATE TABLE IF NOT EXISTS entries(id INTEGER PRIMARY KEY, date TEXT NOT NULL, memo TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS lines(
               id INTEGER PRIMARY KEY,
               entry_id INTEGER NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
               account_id INTEGER NOT NULL REFERENCES accounts(id),
               cents INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS customers(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, email TEXT NOT NULL DEFAULT '');
             CREATE TABLE IF NOT EXISTS vendors(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, email TEXT NOT NULL DEFAULT '');
             CREATE TABLE IF NOT EXISTS jobs(
               id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, customer_id INTEGER REFERENCES customers(id));
             CREATE TABLE IF NOT EXISTS invoices(
               id INTEGER PRIMARY KEY,
               customer_id INTEGER NOT NULL REFERENCES customers(id),
               date TEXT NOT NULL, description TEXT NOT NULL, cents INTEGER NOT NULL,
               entry_id INTEGER NOT NULL REFERENCES entries(id),
               paid_entry_id INTEGER REFERENCES entries(id));
             CREATE TABLE IF NOT EXISTS invoice_items(
               id INTEGER PRIMARY KEY, invoice_id INTEGER NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,
               description TEXT NOT NULL, cents INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS quotes(
               id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL REFERENCES customers(id),
               date TEXT NOT NULL, invoice_id INTEGER REFERENCES invoices(id));
             CREATE TABLE IF NOT EXISTS quote_items(
               id INTEGER PRIMARY KEY, quote_id INTEGER NOT NULL REFERENCES quotes(id) ON DELETE CASCADE,
               description TEXT NOT NULL, cents INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS settings(
               id INTEGER PRIMARY KEY CHECK(id = 1), name TEXT NOT NULL DEFAULT '', address TEXT NOT NULL DEFAULT '',
               logo BLOB, simplefin_url TEXT);
             INSERT OR IGNORE INTO settings(id) VALUES (1);
             CREATE TABLE IF NOT EXISTS feed_accounts(
               feed_id TEXT PRIMARY KEY, name TEXT NOT NULL, account_id INTEGER REFERENCES accounts(id));
             -- SET NULL keeps the id after a user deletes the entry, so the next download doesn't bring it back.
             CREATE TABLE IF NOT EXISTS feed_txns(
               feed_id TEXT PRIMARY KEY, entry_id INTEGER REFERENCES entries(id) ON DELETE SET NULL);
             CREATE TABLE IF NOT EXISTS payments(
               id INTEGER PRIMARY KEY, invoice_id INTEGER NOT NULL REFERENCES invoices(id),
               entry_id INTEGER NOT NULL REFERENCES entries(id), date TEXT NOT NULL, cents INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS rules(
               id INTEGER PRIMARY KEY, pattern TEXT NOT NULL, account_id INTEGER NOT NULL REFERENCES accounts(id),
               vendor_id INTEGER REFERENCES vendors(id), job_id INTEGER REFERENCES jobs(id));
             CREATE TABLE IF NOT EXISTS reconciliations(
               id INTEGER PRIMARY KEY, account_id INTEGER NOT NULL REFERENCES accounts(id),
               date TEXT NOT NULL, cents INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS history(
               id INTEGER PRIMARY KEY, at TEXT NOT NULL, user TEXT NOT NULL, what TEXT NOT NULL, undo TEXT);
             CREATE TABLE IF NOT EXISTS attachments(
               entry_id INTEGER PRIMARY KEY REFERENCES entries(id) ON DELETE CASCADE, name TEXT NOT NULL, data BLOB NOT NULL);
             CREATE TABLE IF NOT EXISTS trips(
               id INTEGER PRIMARY KEY, date TEXT NOT NULL, purpose TEXT NOT NULL, tenths INTEGER NOT NULL,
               job_id INTEGER REFERENCES jobs(id));
             CREATE TABLE IF NOT EXISTS recurring(
               id INTEGER PRIMARY KEY, kind TEXT NOT NULL CHECK(kind IN ('txn','invoice')),
               every TEXT NOT NULL CHECK(every IN ('week','month','year')), next_date TEXT NOT NULL, data TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS time_entries(
               id INTEGER PRIMARY KEY, job_id INTEGER NOT NULL REFERENCES jobs(id), date TEXT NOT NULL,
               minutes INTEGER NOT NULL, rate INTEGER NOT NULL, note TEXT NOT NULL, invoice_id INTEGER REFERENCES invoices(id));
             CREATE TABLE IF NOT EXISTS deposits(
               id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL REFERENCES customers(id),
               entry_id INTEGER NOT NULL REFERENCES entries(id), date TEXT NOT NULL, cents INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS assets(
               id INTEGER PRIMARY KEY, name TEXT NOT NULL, account_id INTEGER NOT NULL REFERENCES accounts(id),
               purchased TEXT NOT NULL, cost INTEGER NOT NULL, life_years INTEGER NOT NULL, serial TEXT NOT NULL DEFAULT '',
               notes TEXT NOT NULL DEFAULT '', disposed TEXT NOT NULL DEFAULT '', entry_id INTEGER REFERENCES entries(id) ON DELETE SET NULL);
             CREATE TABLE IF NOT EXISTS asset_photos(
               asset_id INTEGER PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE, name TEXT NOT NULL, data BLOB NOT NULL);
             CREATE TABLE IF NOT EXISTS depreciation_posts(year INTEGER PRIMARY KEY, entry_id INTEGER REFERENCES entries(id) ON DELETE SET NULL);
             CREATE TABLE IF NOT EXISTS purchase_orders(
               id INTEGER PRIMARY KEY, vendor_id INTEGER NOT NULL REFERENCES vendors(id), date TEXT NOT NULL,
               job_id INTEGER REFERENCES jobs(id), notes TEXT NOT NULL DEFAULT '', status TEXT NOT NULL DEFAULT 'open',
               received TEXT NOT NULL DEFAULT '', entry_id INTEGER REFERENCES entries(id),
               paid_from INTEGER REFERENCES accounts(id), paid_entry_id INTEGER REFERENCES entries(id));
             CREATE TABLE IF NOT EXISTS po_items(
               id INTEGER PRIMARY KEY, po_id INTEGER NOT NULL REFERENCES purchase_orders(id) ON DELETE CASCADE,
               description TEXT NOT NULL, cents INTEGER NOT NULL, account_id INTEGER NOT NULL REFERENCES accounts(id));
             CREATE TABLE IF NOT EXISTS proposals(
               id INTEGER PRIMARY KEY, title TEXT NOT NULL, customer_id INTEGER REFERENCES customers(id),
               job_id INTEGER REFERENCES jobs(id), quote_id INTEGER REFERENCES quotes(id), date TEXT NOT NULL,
               status TEXT NOT NULL DEFAULT 'draft', body TEXT NOT NULL DEFAULT '');
             CREATE TABLE IF NOT EXISTS proposal_templates(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, body TEXT NOT NULL);",
        )?;
        // Columns added after the first release; older files get them on open.
        for (table, col) in [
            ("entries", "job_id INTEGER REFERENCES jobs(id)"),
            ("entries", "vendor_id INTEGER REFERENCES vendors(id)"),
            ("quotes", "job_id INTEGER REFERENCES jobs(id)"),
            ("accounts", "number TEXT NOT NULL DEFAULT ''"),
            ("accounts", "description TEXT NOT NULL DEFAULT ''"),
            ("accounts", "active INTEGER NOT NULL DEFAULT 1"),
            ("accounts", "system TEXT"),
            ("settings", "chart_version INTEGER NOT NULL DEFAULT 0"),
            ("settings", "lock_date TEXT NOT NULL DEFAULT ''"),
            ("settings", "pay_link TEXT NOT NULL DEFAULT ''"),
            ("settings", "mileage_rate INTEGER NOT NULL DEFAULT 70"),
            ("settings", "hourly_rate INTEGER NOT NULL DEFAULT 0"),
            ("settings", "income_tax_pct INTEGER NOT NULL DEFAULT 12"),
            ("lines", "cleared INTEGER NOT NULL DEFAULT 0"), // 0 no, 1 ticked, 2 reconciled
            ("invoices", "due_date TEXT NOT NULL DEFAULT ''"),
            ("invoices", "tax_cents INTEGER NOT NULL DEFAULT 0"),
            ("quotes", "tax_bp INTEGER NOT NULL DEFAULT 0"),
            ("customers", "address TEXT NOT NULL DEFAULT ''"),
            ("vendors", "is_1099 INTEGER NOT NULL DEFAULT 0"),
            ("invoice_items", "taxable INTEGER NOT NULL DEFAULT 1"),
            ("quote_items", "taxable INTEGER NOT NULL DEFAULT 1"),
            ("payments", "deposit INTEGER NOT NULL DEFAULT 0"), // 1 = applied from a customer deposit
            ("invoices", "customer_po TEXT NOT NULL DEFAULT ''"),
            ("quotes", "customer_po TEXT NOT NULL DEFAULT ''"),
            ("settings", "templates_seeded INTEGER NOT NULL DEFAULT 0"),
            ("accounts", "intangible INTEGER NOT NULL DEFAULT 0"),
            ("customers", "company TEXT NOT NULL DEFAULT ''"),
            ("customers", "dba TEXT NOT NULL DEFAULT ''"),
            ("customers", "contact TEXT NOT NULL DEFAULT ''"),
            ("customers", "street TEXT NOT NULL DEFAULT ''"),
            ("customers", "street2 TEXT NOT NULL DEFAULT ''"),
            ("customers", "city TEXT NOT NULL DEFAULT ''"),
            ("customers", "state TEXT NOT NULL DEFAULT ''"),
            ("customers", "zip TEXT NOT NULL DEFAULT ''"),
            ("customers", "phone TEXT NOT NULL DEFAULT ''"),
            ("customers", "phone2 TEXT NOT NULL DEFAULT ''"),
            ("customers", "website TEXT NOT NULL DEFAULT ''"),
            ("customers", "notes TEXT NOT NULL DEFAULT ''"),
            ("customers", "tax_id TEXT NOT NULL DEFAULT ''"),
            ("vendors", "company TEXT NOT NULL DEFAULT ''"),
            ("vendors", "dba TEXT NOT NULL DEFAULT ''"),
            ("vendors", "contact TEXT NOT NULL DEFAULT ''"),
            ("vendors", "street TEXT NOT NULL DEFAULT ''"),
            ("vendors", "street2 TEXT NOT NULL DEFAULT ''"),
            ("vendors", "city TEXT NOT NULL DEFAULT ''"),
            ("vendors", "state TEXT NOT NULL DEFAULT ''"),
            ("vendors", "zip TEXT NOT NULL DEFAULT ''"),
            ("vendors", "phone TEXT NOT NULL DEFAULT ''"),
            ("vendors", "phone2 TEXT NOT NULL DEFAULT ''"),
            ("vendors", "website TEXT NOT NULL DEFAULT ''"),
            ("vendors", "notes TEXT NOT NULL DEFAULT ''"),
            ("vendors", "tax_id TEXT NOT NULL DEFAULT ''"),
            ("vendors", "address TEXT NOT NULL DEFAULT ''"),
            ("settings", "dba TEXT NOT NULL DEFAULT ''"),
            ("settings", "contact TEXT NOT NULL DEFAULT ''"),
            ("settings", "street TEXT NOT NULL DEFAULT ''"),
            ("settings", "street2 TEXT NOT NULL DEFAULT ''"),
            ("settings", "city TEXT NOT NULL DEFAULT ''"),
            ("settings", "state TEXT NOT NULL DEFAULT ''"),
            ("settings", "zip TEXT NOT NULL DEFAULT ''"),
            ("settings", "phone TEXT NOT NULL DEFAULT ''"),
            ("settings", "phone2 TEXT NOT NULL DEFAULT ''"),
            ("settings", "email TEXT NOT NULL DEFAULT ''"),
            ("settings", "website TEXT NOT NULL DEFAULT ''"),
        ] {
            let name = col.split(' ').next().unwrap_or_default();
            if !db.prepare(&format!("SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1"))?.exists([name])? {
                db.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {col}"))?;
            }
        }
        db.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS accounts_system ON accounts(system);
             -- Invoices paid before the payments table existed: their single payment, carried over.
             INSERT INTO payments(invoice_id, entry_id, date, cents)
               SELECT i.id, i.paid_entry_id, e.date, i.cents FROM invoices i JOIN entries e ON e.id = i.paid_entry_id
               WHERE NOT EXISTS (SELECT 1 FROM payments p WHERE p.entry_id = i.paid_entry_id);",
        )?;
        let books = Books { db };
        // Adds the standard chart once; later the user's edits and deletions stand.
        let version: i64 = books.db.query_row("SELECT chart_version FROM settings WHERE id = 1", [], |r| r.get(0))?;
        // Version 5 adds the intangible-asset accounts; older charts get only those, so deletions stand.
        const V5: [&str; 4] = ["Software", "Website & Digital Assets", "Accumulated Amortization", "Amortization"];
        if version < 5 {
            books.atomic(|| {
            let tx = &books.db;
            for (num, name, kind, system) in DEFAULT_ACCOUNTS.iter().filter(|a| version < 4 || V5.contains(&a.1)) {
                tx.execute("INSERT OR IGNORE INTO accounts(name, kind) VALUES (?1, ?2)", params![name, kind])?;
                tx.execute("UPDATE accounts SET number = ?1 WHERE name = ?2 AND number = ''", params![num, name])?;
                if system.is_some() {
                    // Only claim a key no other account holds (a renamed system account keeps it).
                    tx.execute(
                        "UPDATE accounts SET system = ?1 WHERE name = ?2 AND NOT EXISTS (SELECT 1 FROM accounts WHERE system = ?1)",
                        params![system, name],
                    )?;
                }
            }
            tx.execute("UPDATE accounts SET intangible = 1 WHERE name IN ('Software', 'Website & Digital Assets')", [])?;
            tx.execute("UPDATE settings SET chart_version = 5 WHERE id = 1", [])?;
            Ok(())
            })?;
        }
        books.seed_templates()?;
        Ok(books)
    }

    pub fn add_account(&self, number: &str, name: &str, kind: &str, description: &str) -> Result<i64> {
        if name.trim().is_empty() {
            return Err(rusqlite::Error::InvalidParameterName("enter an account name".into()));
        }
        self.db.execute(
            "INSERT INTO accounts(number, name, kind, description) VALUES (?1, ?2, ?3, ?4)",
            params![number.trim(), name.trim(), kind, description.trim()],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    /// Marks an asset category as intangible (amortized) or physical (depreciated).
    pub fn set_intangible(&self, account: i64, on: bool) -> Result<()> {
        self.db.execute("UPDATE accounts SET intangible = ?1 WHERE id = ?2 AND kind = 'asset'", params![on, account])?;
        Ok(())
    }

    /// Saves number, name, description, active flag and kind. Kind changes only while unused,
    /// and system accounts stay active with their kind, so posting and reports never break.
    pub fn update_account(&self, a: &Account) -> Result<()> {
        let (used, system, kind): (i64, Option<String>, String) = self.db.query_row(
            "SELECT (SELECT COUNT(*) FROM lines WHERE account_id = ?1), system, kind FROM accounts WHERE id = ?1",
            [a.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let err = |m: &str| Err(rusqlite::Error::InvalidParameterName(m.into()));
        if a.name.trim().is_empty() {
            return err("an account needs a name");
        }
        if a.kind != kind && (used > 0 || system.is_some()) {
            return err("this account is in use, so its type can't change; make a new account instead");
        }
        if !a.active && system.is_some() {
            return err("the app posts to this account, so it can't be turned off");
        }
        self.db.execute(
            "UPDATE accounts SET number = ?1, name = ?2, description = ?3, active = ?4, kind = ?5 WHERE id = ?6",
            params![a.number.trim(), a.name.trim(), a.description.trim(), a.active, a.kind, a.id],
        )?;
        Ok(())
    }

    /// Deletes an account nothing refers to. Used accounts can be turned off instead.
    pub fn delete_account(&self, id: i64) -> Result<()> {
        let (used, system): (i64, Option<String>) = self.db.query_row(
            "SELECT (SELECT COUNT(*) FROM lines WHERE account_id = ?1), system FROM accounts WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if used > 0 || system.is_some() {
            return Err(rusqlite::Error::InvalidParameterName("this account is in use; turn it off instead".into()));
        }
        self.db.execute("UPDATE feed_accounts SET account_id = NULL WHERE account_id = ?1", [id])?;
        self.db.execute("DELETE FROM accounts WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn accounts(&self) -> Result<Vec<Account>> {
        let mut st = self.db.prepare(
            "SELECT id, number, name, kind, description, active, system,
               (SELECT COUNT(*) FROM lines WHERE account_id = accounts.id), intangible
             FROM accounts ORDER BY
             CASE kind WHEN 'asset' THEN 0 WHEN 'liability' THEN 1 WHEN 'equity' THEN 2 WHEN 'income' THEN 3 ELSE 4 END,
             number = '', number, name",
        )?;
        let rows = st.query_map([], |r| {
            Ok(Account {
                id: r.get(0)?,
                number: r.get(1)?,
                name: r.get(2)?,
                kind: r.get(3)?,
                description: r.get(4)?,
                active: r.get(5)?,
                system: r.get(6)?,
                used: r.get(7)?,
                intangible: r.get(8)?,
            })
        })?;
        rows.collect()
    }

    /// An account the app posts to by itself, found by key so the user may rename it.
    pub(crate) fn system_account(&self, key: &str) -> Result<i64> {
        self.db.query_row("SELECT id FROM accounts WHERE system = ?1", [key], |r| r.get(0))
    }

    #[cfg(test)]
    fn account_id(&self, name: &str) -> Result<i64> {
        self.db.query_row("SELECT id FROM accounts WHERE name = ?1", [name], |r| r.get(0))
    }

    /// Runs `f` all-or-nothing. Uses a savepoint, so it nests (a payment inside a larger step, say).
    pub(crate) fn atomic<T>(&self, f: impl FnOnce() -> Result<T>) -> Result<T> {
        self.db.execute_batch("SAVEPOINT atomic")?;
        match f() {
            Ok(v) => {
                self.db.execute_batch("RELEASE atomic")?;
                Ok(v)
            }
            Err(e) => {
                let _ = self.db.execute_batch("ROLLBACK TO atomic; RELEASE atomic");
                Err(e)
            }
        }
    }

    /// Refuses changes dated on or before the lock date, so filed periods stay as filed.
    pub(crate) fn check_open(&self, date: &str) -> Result<()> {
        let lock: String = self.db.query_row("SELECT lock_date FROM settings WHERE id = 1", [], |r| r.get(0))?;
        if !lock.is_empty() && date <= lock.as_str() {
            return Err(rusqlite::Error::InvalidParameterName(format!("the books are locked through {lock}; change the lock date on Business to edit")));
        }
        Ok(())
    }

    fn entry_date(&self, entry_id: i64) -> Result<String> {
        self.db.query_row("SELECT date FROM entries WHERE id = ?1", [entry_id], |r| r.get(0))
    }

    /// Posts a balanced entry. Rejects unbalanced or empty entries so the books can never drift.
    pub fn post(&self, date: &str, memo: &str, lines: &[(i64, i64)]) -> Result<i64> {
        self.check_open(date)?;
        let sum: i64 = lines.iter().map(|l| l.1).sum();
        if lines.len() < 2 || sum != 0 {
            return Err(rusqlite::Error::InvalidParameterName(format!("entry does not balance (off by {sum})")));
        }
        self.atomic(|| {
            self.db.execute("INSERT INTO entries(date, memo) VALUES (?1, ?2)", params![date, memo])?;
            let id = self.db.last_insert_rowid();
            for (acct, cents) in lines {
                self.db.execute("INSERT INTO lines(entry_id, account_id, cents) VALUES (?1, ?2, ?3)", params![id, acct, cents])?;
            }
            Ok(id)
        })
    }

    /// Money in (cents > 0) or out (cents < 0) of a bank/card account, against a category.
    pub fn post_simple(&self, date: &str, memo: &str, bank: i64, category: i64, cents: i64) -> Result<i64> {
        self.post(date, memo, &[(bank, cents), (category, -cents)])
    }

    pub fn entries(&self) -> Result<Vec<Entry>> {
        let mut st = self.db.prepare(
            "SELECT id, date, memo, job_id, vendor_id, EXISTS (SELECT 1 FROM attachments a WHERE a.entry_id = entries.id),
               EXISTS (SELECT 1 FROM invoices i WHERE i.entry_id = entries.id) OR EXISTS (SELECT 1 FROM payments p WHERE p.entry_id = entries.id)
             FROM entries ORDER BY date DESC, id DESC",
        )?;
        let mut ls = self.db.prepare("SELECT id, account_id, cents FROM lines WHERE entry_id = ?1 ORDER BY id")?;
        let heads = st
            .query_map([], |r| {
                Ok(Entry { id: r.get(0)?, date: r.get(1)?, memo: r.get(2)?, lines: vec![], job: r.get(3)?, vendor: r.get(4)?, receipt: r.get(5)?, invoice: r.get(6)? })
            })?
            .collect::<Result<Vec<_>>>()?;
        heads
            .into_iter()
            .map(|mut e| {
                e.lines = ls.query_map([e.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_>>()?;
                Ok(e)
            })
            .collect()
    }

    /// Tags an entry with a job and/or vendor (None clears it).
    pub fn set_tags(&self, entry_id: i64, job: Option<i64>, vendor: Option<i64>) -> Result<()> {
        self.check_open(&self.entry_date(entry_id)?)?;
        self.db.execute("UPDATE entries SET job_id = ?1, vendor_id = ?2 WHERE id = ?3", params![job, vendor, entry_id])?;
        Ok(())
    }

    pub fn add_job(&self, name: &str, customer_id: Option<i64>) -> Result<i64> {
        if name.trim().is_empty() {
            return Err(rusqlite::Error::InvalidParameterName("enter a job name".into()));
        }
        self.db.execute("INSERT INTO jobs(name, customer_id) VALUES (?1, ?2)", params![name.trim(), customer_id])?;
        Ok(self.db.last_insert_rowid())
    }

    /// Every job with income (credit-normal, shown positive) and expenses from its tagged entries.
    pub fn jobs(&self) -> Result<Vec<JobTotals>> {
        self.jobs_between("0000-00-00", "9999-12-31")
    }

    /// Job totals counting only entries dated in the range (inclusive).
    pub fn jobs_between(&self, from: &str, to: &str) -> Result<Vec<JobTotals>> {
        let mut st = self.db.prepare(
            "SELECT j.id, j.name, c.name,
               COALESCE(SUM(CASE WHEN a.kind = 'income' THEN -l.cents END), 0),
               COALESCE(SUM(CASE WHEN a.kind = 'expense' THEN l.cents END), 0),
               (SELECT COALESCE(SUM(qi.cents), 0) FROM quotes q JOIN quote_items qi ON qi.quote_id = q.id WHERE q.job_id = j.id),
               j.customer_id
             FROM jobs j LEFT JOIN customers c ON c.id = j.customer_id
             LEFT JOIN entries e ON e.job_id = j.id AND e.date BETWEEN ?1 AND ?2 LEFT JOIN lines l ON l.entry_id = e.id
             LEFT JOIN accounts a ON a.id = l.account_id
             GROUP BY j.id ORDER BY j.name",
        )?;
        let rows = st.query_map([from, to], |r| {
            Ok(JobTotals { id: r.get(0)?, name: r.get(1)?, customer: r.get(2)?, customer_id: r.get(6)?, income: r.get(3)?, expenses: r.get(4)?, quoted: r.get(5)? })
        })?;
        rows.collect()
    }

    /// Re-points one line (recategorizing). A reconciled bank line can't move, and on an invoice or
    /// payment only the income line may change, to another income account, so A/R and sales tax stay right.
    /// A job's costs: each expense line tagged with the job, as (date, description, account, cents).
    pub fn job_costs(&self, job: i64) -> Result<Vec<(String, String, String, i64)>> {
        let mut st = self.db.prepare(
            "SELECT e.date, e.memo, a.name, l.cents FROM entries e JOIN lines l ON l.entry_id = e.id
             JOIN accounts a ON a.id = l.account_id AND a.kind = 'expense' WHERE e.job_id = ?1 ORDER BY e.date DESC, e.id DESC",
        )?;
        let rows = st.query_map([job], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        rows.collect()
    }

    pub fn set_line_account(&self, line_id: i64, account_id: i64) -> Result<()> {
        let (entry, cleared, kind): (i64, i64, String) = self.db.query_row(
            "SELECT l.entry_id, l.cleared, a.kind FROM lines l JOIN accounts a ON a.id = l.account_id WHERE l.id = ?1",
            [line_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        self.check_open(&self.entry_date(entry)?)?;
        if cleared == 2 {
            return Err(rusqlite::Error::InvalidParameterName("this line is reconciled, so its account can't change".into()));
        }
        if self.invoice_entry(entry)? {
            let new_kind: String = self.db.query_row("SELECT kind FROM accounts WHERE id = ?1", [account_id], |r| r.get(0))?;
            if kind != "income" || new_kind != "income" {
                return Err(rusqlite::Error::InvalidParameterName("on an invoice you can only switch the income category".into()));
            }
        }
        self.db.execute("UPDATE lines SET account_id = ?1 WHERE id = ?2", params![account_id, line_id])?;
        Ok(())
    }

    fn invoice_entry(&self, id: i64) -> Result<bool> {
        self.db.query_row(
            "SELECT EXISTS (SELECT 1 FROM invoices WHERE entry_id = ?1) OR EXISTS (SELECT 1 FROM payments WHERE entry_id = ?1)
               OR EXISTS (SELECT 1 FROM deposits WHERE entry_id = ?1)
               OR EXISTS (SELECT 1 FROM purchase_orders WHERE entry_id = ?1 OR paid_entry_id = ?1)",
            [id],
            |r| r.get(0),
        )
    }

    fn reconciled(&self, entry: i64) -> Result<bool> {
        self.db.query_row("SELECT EXISTS (SELECT 1 FROM lines WHERE entry_id = ?1 AND cleared = 2)", [entry], |r| r.get(0))
    }

    pub fn delete_entry(&self, id: i64) -> Result<()> {
        if self.invoice_entry(id)? {
            return Err(rusqlite::Error::InvalidParameterName("this entry belongs to an invoice".into()));
        }
        if self.reconciled(id)? {
            return Err(rusqlite::Error::InvalidParameterName("this transaction is reconciled against a statement, so it can't be deleted".into()));
        }
        self.check_open(&self.entry_date(id)?)?;
        self.db.execute("DELETE FROM entries WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Edits a simple two-line entry (money in/out or transfer): date, memo, the first (bank) line's
    /// account and signed amount; the other line mirrors it so the entry stays balanced.
    pub fn update_simple(&self, id: i64, date: &str, memo: &str, bank: i64, cents: i64) -> Result<()> {
        if self.invoice_entry(id)? {
            return Err(rusqlite::Error::InvalidParameterName("this entry belongs to an invoice; change the invoice instead".into()));
        }
        let lines: Vec<i64> = self.db.prepare("SELECT id FROM lines WHERE entry_id = ?1 ORDER BY id")?.query_map([id], |r| r.get(0))?.collect::<Result<_>>()?;
        if lines.len() != 2 || cents == 0 {
            return Err(rusqlite::Error::InvalidParameterName("only simple transactions with a non-zero amount can be edited".into()));
        }
        if self.reconciled(id)? {
            return Err(rusqlite::Error::InvalidParameterName("this transaction is reconciled against a statement, so it can't be edited".into()));
        }
        self.check_open(&self.entry_date(id)?)?;
        self.check_open(date)?;
        self.atomic(|| {
            self.db.execute("UPDATE entries SET date = ?1, memo = ?2 WHERE id = ?3", params![date, memo.trim(), id])?;
            self.db.execute("UPDATE lines SET account_id = ?1, cents = ?2, cleared = 0 WHERE id = ?3", params![bank, cents, lines[0]])?;
            self.db.execute("UPDATE lines SET cents = ?1, cleared = 0 WHERE id = ?2", params![-cents, lines[1]])?;
            Ok(())
        })
    }

    /// Sum of lines per account over a date range (inclusive), as (kind, name, natural-sign cents).
    fn balances(&self, from: &str, to: &str, kinds: &[&str]) -> Result<Report> {
        let mut st = self.db.prepare(
            "SELECT a.kind, a.name, COALESCE(SUM(l.cents), 0) FROM accounts a
             JOIN lines l ON l.account_id = a.id JOIN entries e ON e.id = l.entry_id
             WHERE e.date BETWEEN ?1 AND ?2 GROUP BY a.id ORDER BY a.kind, a.name",
        )?;
        let rows = st
            .query_map(params![from, to], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))?
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter(|(k, _, c)| kinds.contains(&k.as_str()) && *c != 0)
            // Credit-normal accounts read positive when they hold a credit balance.
            .map(|(k, n, c)| { let c = if k == "asset" || k == "expense" { c } else { -c }; (k, n, c) })
            .collect();
        Ok(Report { rows })
    }

    /// (YYYY-MM, income, expenses) for each month in the range, oldest first, months with no activity included.
    pub fn monthly_totals(&self, first_month: &str, months: usize) -> Result<Vec<(String, i64, i64)>> {
        let start = NaiveDate::parse_from_str(&format!("{first_month}-01"), "%Y-%m-%d")
            .map_err(|_| rusqlite::Error::InvalidParameterName("bad month".into()))?;
        let mut st = self.db.prepare(
            "SELECT COALESCE(SUM(CASE WHEN a.kind = 'income' THEN -l.cents END), 0),
                    COALESCE(SUM(CASE WHEN a.kind = 'expense' THEN l.cents END), 0)
             FROM lines l JOIN entries e ON e.id = l.entry_id JOIN accounts a ON a.id = l.account_id
             WHERE substr(e.date, 1, 7) = ?1",
        )?;
        (0..months)
            .map(|i| {
                let m = start.checked_add_months(chrono::Months::new(i as u32)).unwrap_or(start).format("%Y-%m").to_string();
                let (inc, exp) = st.query_row([&m], |r| Ok((r.get(0)?, r.get(1)?)))?;
                Ok((m, inc, exp))
            })
            .collect()
    }

    pub fn profit_and_loss(&self, from: &str, to: &str) -> Result<Report> {
        self.balances(from, to, &["income", "expense"])
    }

    /// Balance sheet as of a date; profit to date appears as a "Net income" equity row.
    pub fn balance_sheet(&self, as_of: &str) -> Result<Report> {
        let mut r = self.balances("0000-00-00", as_of, &["asset", "liability", "equity"])?;
        let net = net_income(&self.profit_and_loss("0000-00-00", as_of)?);
        if net != 0 {
            r.rows.push(("equity".into(), "Net income (to date)".into(), net));
        }
        Ok(r)
    }

    pub fn add_customer(&self, name: &str, email: &str) -> Result<i64> {
        self.add_party("customers", name, email)
    }

    pub fn customers(&self) -> Result<Vec<Party>> {
        self.parties("customers", "0")
    }

    pub fn add_vendor(&self, name: &str, email: &str) -> Result<i64> {
        self.add_party("vendors", name, email)
    }

    pub fn vendors(&self) -> Result<Vec<Party>> {
        self.parties("vendors", "is_1099")
    }

    /// Adds (id 0) or updates a customer or vendor with all its details. The printed address
    /// is rebuilt from the fields: "Attn: contact", street, suite, city/state/ZIP.
    pub fn save_party(&self, vendor: bool, p: &Party) -> Result<i64> {
        let table = if vendor { "vendors" } else { "customers" };
        let i = &p.info;
        let id = if p.id == 0 { self.add_party(table, &p.name, &i.email)? } else { p.id };
        if p.name.trim().is_empty() {
            return Err(rusqlite::Error::InvalidParameterName("enter a name".into()));
        }
        let mut addr = vec![];
        if !i.contact.trim().is_empty() {
            addr.push(format!("Attn: {}", i.contact.trim()));
        }
        addr.extend(i.mailing());
        self.db.execute(
            &format!(
                "UPDATE {table} SET name = ?1, email = ?2, address = ?3, company = ?4, dba = ?5, contact = ?6, street = ?7, street2 = ?8,
                   city = ?9, state = ?10, zip = ?11, phone = ?12, phone2 = ?13, website = ?14, notes = ?15, tax_id = ?16{} WHERE id = ?17",
                if vendor { ", is_1099 = ?18" } else { "" }
            ),
            rusqlite::params_from_iter(
                [
                    p.name.trim(), i.email.trim(), addr.join("\n").as_str(), i.name.trim(), i.dba.trim(), i.contact.trim(), i.street.trim(), i.street2.trim(),
                    i.city.trim(), i.state.trim(), i.zip.trim(), i.phone.trim(), i.phone2.trim(), i.website.trim(), i.extra.trim(), p.tax_id.trim(),
                ]
                .iter()
                .map(|s| rusqlite::types::Value::Text(s.to_string()))
                .chain([rusqlite::types::Value::Integer(id)])
                .chain(vendor.then_some(rusqlite::types::Value::Integer(p.is_1099 as i64))),
            ),
        )?;
        Ok(id)
    }

    fn add_party(&self, table: &str, name: &str, email: &str) -> Result<i64> {
        if name.trim().is_empty() {
            return Err(rusqlite::Error::InvalidParameterName("enter a name".into()));
        }
        self.db.execute(&format!("INSERT INTO {table}(name, email) VALUES (?1, ?2)"), params![name.trim(), email.trim()])?;
        Ok(self.db.last_insert_rowid())
    }

    fn parties(&self, table: &str, extra: &str) -> Result<Vec<Party>> {
        let mut st = self.db.prepare(&format!(
            "SELECT id, name, email, address, {extra}, company, dba, contact, street, street2, city, state, zip, phone, phone2, website, notes, tax_id
             FROM {table} ORDER BY name"
        ))?;
        let rows = st.query_map([], |r| {
            let mut info = Business {
                name: r.get(5)?, dba: r.get(6)?, contact: r.get(7)?, street: r.get(8)?, street2: r.get(9)?, city: r.get(10)?, state: r.get(11)?,
                zip: r.get(12)?, phone: r.get(13)?, phone2: r.get(14)?, email: r.get(2)?, website: r.get(15)?, extra: r.get(16)?,
            };
            let address: String = r.get(3)?;
            // Older books had one free-text address: show it in Street until it's split up.
            if info.mailing().is_empty() && info.contact.is_empty() && !address.is_empty() {
                info.street = address.replace('\n', ", ");
            }
            Ok(Party { id: r.get(0)?, name: r.get(1)?, email: r.get(2)?, address, is_1099: r.get(4)?, info, tax_id: r.get(17)? })
        })?;
        rows.collect()
    }

    /// Invoicing books the sale now (Dr Accounts Receivable for the total, Cr Sales for the subtotal,
    /// Cr Sales Tax Payable for the tax): accrual basis.
    pub fn create_invoice(&self, d: &DocInput) -> Result<i64> {
        let (description, subtotal) = summarize(&d.items)?;
        if !(0..=5000).contains(&d.tax_bp) {
            return Err(rusqlite::Error::InvalidParameterName("sales tax must be between 0% and 50%".into()));
        }
        let tax = tax_cents(d.taxable_subtotal(), d.tax_bp);
        let (ar, sales) = (self.system_account("ar")?, self.system_account("sales")?);
        let name: String = self.db.query_row("SELECT name FROM customers WHERE id = ?1", [d.customer_id], |r| r.get(0))?;
        let mut lines = vec![(ar, subtotal + tax), (sales, -subtotal)];
        if tax != 0 {
            lines.push((self.system_account("sales_tax")?, -tax));
        }
        self.atomic(|| {
            let entry = self.post(&d.date, &format!("Invoice to {name}: {description}"), &lines)?;
            self.set_tags(entry, d.job, None)?;
            self.db.execute(
                "INSERT INTO invoices(customer_id, date, description, cents, entry_id, due_date, tax_cents, customer_po) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![d.customer_id, d.date, description, subtotal + tax, entry, d.due_date, tax, d.customer_po.trim()],
            )?;
            let id = self.db.last_insert_rowid();
            self.insert_items("invoice_items", "invoice_id", id, &d.items, &d.taxable)?;
            Ok(id)
        })
    }

    fn insert_items(&self, table: &str, key: &str, id: i64, items: &[Item], taxable: &[bool]) -> Result<()> {
        for (i, (d, c)) in items.iter().enumerate() {
            let t = taxable.get(i).copied().unwrap_or(true);
            self.db.execute(&format!("INSERT INTO {table}({key}, description, cents, taxable) VALUES (?1, ?2, ?3, ?4)"), params![id, d.trim(), c, t])?;
        }
        Ok(())
    }

    /// Which of a document's items are taxable, in the same order as `items`.
    pub fn item_taxable(&self, doc: Doc, id: i64) -> Result<Vec<bool>> {
        let (table, key) = match doc {
            Doc::Invoice => ("invoice_items", "invoice_id"),
            Doc::Quote => ("quote_items", "quote_id"),
        };
        let flags: Vec<bool> = self.db.prepare(&format!("SELECT taxable FROM {table} WHERE {key} = ?1 ORDER BY id"))?.query_map([id], |r| r.get(0))?.collect::<Result<_>>()?;
        Ok(if flags.is_empty() { vec![true] } else { flags })
    }

    /// Line items; invoices made before multi-line support show their single description.
    pub fn items(&self, doc: Doc, id: i64) -> Result<Vec<Item>> {
        let (table, key) = match doc {
            Doc::Invoice => ("invoice_items", "invoice_id"),
            Doc::Quote => ("quote_items", "quote_id"),
        };
        let mut st = self.db.prepare(&format!("SELECT description, cents FROM {table} WHERE {key} = ?1 ORDER BY id"))?;
        let items: Vec<Item> = st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_>>()?;
        if items.is_empty() && doc == Doc::Invoice {
            let old = self.db.query_row("SELECT description, cents FROM invoices WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            return Ok(vec![old]);
        }
        Ok(items)
    }

    /// A quote posts nothing to the books until it is converted. Without a `job`, it starts a new job
    /// named "Customer: first line" so costs can be tracked from the first quote.
    pub fn create_quote(&self, d: &DocInput) -> Result<i64> {
        let (customer_id, items, job) = (d.customer_id, &d.items, d.job);
        summarize(items)?;
        if !(0..=5000).contains(&d.tax_bp) {
            return Err(rusqlite::Error::InvalidParameterName("sales tax must be between 0% and 50%".into()));
        }
        self.atomic(|| {
        self.db.execute(
            "INSERT INTO quotes(customer_id, date, tax_bp, customer_po) VALUES (?1, ?2, ?3, ?4)",
            params![customer_id, d.date, d.tax_bp, d.customer_po.trim()],
        )?;
        let id = self.db.last_insert_rowid();
        self.insert_items("quote_items", "quote_id", id, items, &d.taxable)?;
        let job = match job {
            Some(j) => j,
            None => {
                let cust: String = self.db.query_row("SELECT name FROM customers WHERE id = ?1", [customer_id], |r| r.get(0))?;
                let first = items.iter().map(|i| i.0.trim()).find(|d| !d.is_empty()).unwrap_or("quote");
                let name = format!("{cust}: {first}");
                let taken = self.db.prepare("SELECT 1 FROM jobs WHERE name = ?1")?.exists([&name])?;
                self.add_job(&if taken { format!("{name} (quote #{id})") } else { name }, Some(customer_id))?;
                self.db.last_insert_rowid()
            }
        };
        self.db.execute("UPDATE quotes SET job_id = ?1 WHERE id = ?2", params![job, id])?;
        Ok(id)
        })
    }

    pub fn quotes(&self) -> Result<Vec<Quote>> {
        let mut st = self.db.prepare(
            "SELECT q.id, c.name, q.date, (SELECT COALESCE(SUM(cents), 0) FROM quote_items WHERE quote_id = q.id), q.invoice_id, j.name, q.customer_id, q.job_id
             FROM quotes q JOIN customers c ON c.id = q.customer_id LEFT JOIN jobs j ON j.id = q.job_id ORDER BY q.id DESC",
        )?;
        let rows = st.query_map([], |r| {
            Ok(Quote { id: r.get(0)?, customer_id: r.get(6)?, customer: r.get(1)?, date: r.get(2)?, cents: r.get(3)?, invoice_id: r.get(4)?, job: r.get(5)?, job_id: r.get(7)? })
        })?;
        rows.collect()
    }

    /// Turns a quote into a real invoice dated `date`. Converting twice is refused.
    pub fn convert_quote(&self, quote_id: i64, date: &str, due_date: &str) -> Result<i64> {
        let (cust, done, job, tax_bp, customer_po): (i64, Option<i64>, Option<i64>, i64, String) = self.db.query_row(
            "SELECT customer_id, invoice_id, job_id, tax_bp, customer_po FROM quotes WHERE id = ?1",
            [quote_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?;
        if let Some(inv) = done {
            return Err(rusqlite::Error::InvalidParameterName(format!("quote #{quote_id} is already invoice #{inv}")));
        }
        let d = DocInput {
            customer_id: cust,
            date: date.into(),
            items: self.items(Doc::Quote, quote_id)?,
            job,
            tax_bp,
            due_date: due_date.into(),
            taxable: self.item_taxable(Doc::Quote, quote_id)?,
            customer_po,
        };
        self.atomic(|| {
            let inv = self.create_invoice(&d)?;
            self.db.execute("UPDATE quotes SET invoice_id = ?1 WHERE id = ?2", params![inv, quote_id])?;
            Ok(inv)
        })
    }

    pub fn settings(&self) -> Result<Settings> {
        self.db.query_row(
            "SELECT name, address, logo, simplefin_url, lock_date, pay_link, mileage_rate, hourly_rate, income_tax_pct,
               dba, contact, street, street2, city, state, zip, phone, phone2, email, website
             FROM settings WHERE id = 1",
            [],
            |r| {
                let biz = Business {
                    name: r.get(0)?,
                    extra: r.get(1)?,
                    dba: r.get(9)?,
                    contact: r.get(10)?,
                    street: r.get(11)?,
                    street2: r.get(12)?,
                    city: r.get(13)?,
                    state: r.get(14)?,
                    zip: r.get(15)?,
                    phone: r.get(16)?,
                    phone2: r.get(17)?,
                    email: r.get(18)?,
                    website: r.get(19)?,
                };
                Ok(Settings {
                    name: r.get(0)?,
                    address: biz.letterhead(),
                    biz,
                    logo: r.get(2)?,
                    simplefin_url: r.get(3)?,
                    lock_date: r.get(4)?,
                    pay_link: r.get(5)?,
                    mileage_rate: r.get(6)?,
                    hourly_rate: r.get(7)?,
                    income_tax_pct: r.get(8)?,
                })
            },
        )
    }

    /// Saves the preference fields (everything but name, address, logo and the bank feed).
    pub fn save_prefs(&self, s: &Settings) -> Result<()> {
        let link = s.pay_link.trim();
        if !link.is_empty() && !link.starts_with("https://") {
            return Err(rusqlite::Error::InvalidParameterName("the payment link must start with https://".into()));
        }
        if !(0..=100).contains(&s.income_tax_pct) || s.mileage_rate < 0 || s.hourly_rate < 0 {
            return Err(rusqlite::Error::InvalidParameterName("check the rates".into()));
        }
        self.db.execute(
            "UPDATE settings SET lock_date = ?1, pay_link = ?2, mileage_rate = ?3, hourly_rate = ?4, income_tax_pct = ?5 WHERE id = 1",
            params![s.lock_date, link, s.mileage_rate, s.hourly_rate, s.income_tax_pct],
        )?;
        Ok(())
    }

    pub fn save_business(&self, b: &Business) -> Result<()> {
        self.db.execute(
            "UPDATE settings SET name = ?1, address = ?2, dba = ?3, contact = ?4, street = ?5, street2 = ?6, city = ?7, state = ?8, zip = ?9, phone = ?10, phone2 = ?11, email = ?12, website = ?13 WHERE id = 1",
            params![b.name.trim(), b.extra.trim(), b.dba.trim(), b.contact.trim(), b.street.trim(), b.street2.trim(), b.city.trim(), b.state.trim(), b.zip.trim(), b.phone.trim(), b.phone2.trim(), b.email.trim(), b.website.trim()],
        )?;
        Ok(())
    }

    /// Stores the logo inside the db file so a backup copy carries it. Only PNG/JPEG up to 2 MB.
    pub fn set_logo(&self, bytes: Option<&[u8]>) -> Result<()> {
        if let Some(b) = bytes
            && (image_mime(b).is_none() || b.len() > 2_000_000) {
                return Err(rusqlite::Error::InvalidParameterName("logo must be a PNG or JPEG under 2 MB".into()));
            }
        self.db.execute("UPDATE settings SET logo = ?1 WHERE id = 1", [bytes])?;
        Ok(())
    }

    /// (customer name, email, address, date, due date, tax cents, paid cents) for a document.
    fn doc_header(&self, doc: Doc, id: i64) -> Result<(String, String, String, String, String, i64, i64)> {
        let sql = match doc {
            Doc::Invoice => {
                "SELECT c.name, c.email, c.address, i.date, i.due_date, i.tax_cents,
                   (SELECT COALESCE(SUM(cents), 0) FROM payments WHERE invoice_id = i.id)
                 FROM invoices i JOIN customers c ON c.id = i.customer_id WHERE i.id = ?1"
            }
            Doc::Quote => {
                "SELECT c.name, c.email, c.address, q.date, '', q.tax_bp, 0
                 FROM quotes q JOIN customers c ON c.id = q.customer_id WHERE q.id = ?1"
            }
        };
        let mut h = self.db.query_row(sql, [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)))?;
        if doc == Doc::Quote {
            let flags = self.item_taxable(doc, id)?;
            let taxable: i64 = self.items(doc, id)?.iter().zip(flags.iter().chain(std::iter::repeat(&true))).filter(|(_, t)| **t).map(|(i, _)| i.1).sum();
            h.5 = tax_cents(taxable, h.5); // quotes store the rate; show the amount
        }
        Ok(h)
    }

    /// Business name, address and logo for the top of anything printed.
    pub fn letterhead(&self) -> Result<crate::pdf::Block> {
        let s = self.settings()?;
        Ok(crate::pdf::Block::Letterhead { logo: s.logo, name: s.name, address: s.address })
    }

    /// The line items, tax and totals of an invoice or quote (also the pricing in proposals).
    pub fn doc_table(&self, doc: Doc, id: i64) -> Result<crate::pdf::Block> {
        use crate::pdf::{Align, Block, Row, Style};
        let (_, _, _, _, _, tax, paid) = self.doc_header(doc, id)?;
        let items = self.items(doc, id)?;
        let flags = self.item_taxable(doc, id)?;
        let mixed = tax != 0 && flags.iter().any(|t| !t);
        let row = |a: &str, b: String, style| Row { cells: vec![a.to_string(), b], style };
        let mut rows: Vec<Row> = items
            .iter()
            .zip(flags.iter().chain(std::iter::repeat(&true)))
            .map(|((d, c), t)| row(&format!("{d}{}", if mixed && !t { "  (not taxed)" } else { "" }), fmt_cents(*c), Style::Normal))
            .collect();
        let subtotal: i64 = items.iter().map(|i| i.1).sum();
        let total = subtotal + tax;
        if tax != 0 {
            rows.push(row("Subtotal", fmt_cents(subtotal), Style::Sub));
            rows.push(row("Sales tax", fmt_cents(tax), Style::Sub));
        }
        rows.push(row("Total", fmt_cents(total), Style::Total));
        if doc == Doc::Invoice && paid != 0 {
            rows.push(row("Paid", format!("-{}", fmt_cents(paid)), Style::Sub));
            rows.push(row("Balance due", fmt_cents(total - paid), Style::Total));
        }
        Ok(Block::Table { cols: vec![("Description".into(), Align::Left), ("Amount".into(), Align::Right)], rows })
    }

    /// What a printed invoice or quote shows, top to bottom.
    pub fn document_blocks(&self, doc: Doc, id: i64) -> Result<Vec<crate::pdf::Block>> {
        use crate::pdf::{Block, GREEN};
        let (cust, email, cust_addr, date, due, tax, paid) = self.doc_header(doc, id)?;
        let s = self.settings()?;
        let title = format!("{} #{id}", if doc == Doc::Invoice { "Invoice" } else { "Quote" });
        let total: i64 = self.items(doc, id)?.iter().map(|i| i.1).sum::<i64>() + tax;
        let balance = total - paid;
        let po_sql = if doc == Doc::Invoice { "SELECT customer_po FROM invoices WHERE id = ?1" } else { "SELECT customer_po FROM quotes WHERE id = ?1" };
        let customer_po: String = self.db.query_row(po_sql, [id], |r| r.get(0))?;
        let mut facts = format!("Date: {date}");
        if !customer_po.is_empty() {
            facts += &format!("\nYour PO: {customer_po}");
        }
        match doc {
            Doc::Invoice if balance <= 0 => {}
            Doc::Invoice if !due.is_empty() => facts += &format!("\nDue: {due}"),
            Doc::Invoice => facts += "\nDue on receipt",
            Doc::Quote => {}
        }
        let mut blocks = vec![self.letterhead()?, Block::Title(title), Block::text(facts)];
        if doc == Doc::Invoice && balance <= 0 {
            blocks.push(Block::Text { text: "PAID".into(), size: 13.0, bold: true, color: GREEN });
        }
        blocks.push(Block::Space(6.0));
        blocks.push(Block::bold(if doc == Doc::Invoice { "Bill to" } else { "Prepared for" }));
        let who: Vec<&str> = [cust.as_str(), cust_addr.as_str(), email.as_str()].into_iter().filter(|x| !x.is_empty()).collect();
        blocks.push(Block::text(who.join("\n")));
        blocks.push(self.doc_table(doc, id)?);
        if doc == Doc::Invoice && balance > 0 && !s.pay_link.is_empty() {
            blocks.push(Block::Button { text: "Pay online".into(), url: s.pay_link.clone() });
        }
        Ok(blocks)
    }

    /// The printable invoice or quote as a PDF file.
    pub fn document_pdf(&self, doc: Doc, id: i64) -> std::result::Result<Vec<u8>, String> {
        let title = format!("{} #{id}", if doc == Doc::Invoice { "Invoice" } else { "Quote" });
        crate::pdf::render(&title, &self.document_blocks(doc, id).map_err(|e| e.to_string())?)
    }

    /// (to, subject, plain-text body) for emailing an invoice or quote from the user's mail app.
    /// `reminder` words it as a friendly nudge about the unpaid balance.
    pub fn document_email(&self, doc: Doc, id: i64, reminder: bool) -> Result<(String, String, String)> {
        let (cust, email, _, _, due, tax, paid) = self.doc_header(doc, id)?;
        let s = self.settings()?;
        let biz = s.name;
        let what = if doc == Doc::Invoice { "Invoice" } else { "Quote" };
        let items = self.items(doc, id)?;
        let flags = self.item_taxable(doc, id)?;
        let mixed = tax != 0 && flags.iter().any(|t| !t);
        let mut lines: String = items
            .iter()
            .zip(flags.iter().chain(std::iter::repeat(&true)))
            .map(|((d, c), t)| format!("  {d}{}: {}\n", if mixed && !t { " (not taxed)" } else { "" }, fmt_cents(*c)))
            .collect();
        let total = items.iter().map(|i| i.1).sum::<i64>() + tax;
        if tax != 0 {
            lines += &format!("  Sales tax: {}\n", fmt_cents(tax));
        }
        let from = if biz.is_empty() { String::new() } else { format!(" from {biz}") };
        let mut tail = String::new();
        if doc == Doc::Invoice {
            if paid != 0 {
                tail += &format!("  Paid so far: {}\n  Balance due: {}\n", fmt_cents(paid), fmt_cents(total - paid));
            }
            tail += &if due.is_empty() { "\nDue on receipt.\n".to_string() } else { format!("\nDue {}.\n", nice_dates(&due)) };
            if !s.pay_link.is_empty() {
                tail += &format!("Pay online: {}\n", s.pay_link);
            }
        }
        let (subject, opening) = if reminder {
            (format!("Reminder: {what} #{id}{from}"), format!("Just a friendly reminder that {} #{id}{from} is still open:", what.to_lowercase()))
        } else {
            (format!("{what} #{id}{from}"), format!("Here is {} #{id}{from}:", what.to_lowercase()))
        };
        let body = format!("Hi {cust},\n\n{opening}\n\n{lines}  Total: {}\n{tail}\nThank you!\n{biz}\n", fmt_cents(total));
        Ok((email, subject, body))
    }

    pub fn set_simplefin_url(&self, url: &str) -> Result<()> {
        self.db.execute("UPDATE settings SET simplefin_url = ?1 WHERE id = 1", [url])?;
        Ok(())
    }

    /// Bank-feed accounts seen so far: (feed id, bank's name, mapped local account or None).
    pub fn feed_accounts(&self) -> Result<Vec<(String, String, Option<i64>)>> {
        let mut st = self.db.prepare("SELECT feed_id, name, account_id FROM feed_accounts ORDER BY name")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect()
    }

    pub fn map_feed_account(&self, feed_id: &str, account_id: i64) -> Result<()> {
        self.db.execute("UPDATE feed_accounts SET account_id = ?1 WHERE feed_id = ?2", params![account_id, feed_id])?;
        Ok(())
    }

    /// Imports a SimpleFIN `/accounts` JSON response. New accounts are remembered for mapping;
    /// only mapped accounts post. Each bank transaction id posts at most once, ever.
    /// Returns (imported, skipped as already seen).
    pub fn import_feed(&self, json: &str) -> std::result::Result<(usize, usize), String> {
        let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("bank feed sent bad data: {e}"))?;
        let s = |v: &serde_json::Value, k: &str| v[k].as_str().unwrap_or_default().to_string();
        let (mut imported, mut skipped) = (0, 0);
        for acct in v["accounts"].as_array().ok_or("bank feed sent no accounts")? {
            let fid = s(acct, "id");
            let name = format!("{} {}", s(&acct["org"], "name"), s(acct, "name")).trim().to_string();
            self.db
                .execute("INSERT INTO feed_accounts(feed_id, name) VALUES (?1, ?2) ON CONFLICT DO UPDATE SET name = ?2", params![fid, name])
                .map_err(|e| e.to_string())?;
            let bank: Option<i64> = self
                .db
                .query_row("SELECT account_id FROM feed_accounts WHERE feed_id = ?1", [&fid], |r| r.get(0))
                .map_err(|e| e.to_string())?;
            let Some(bank) = bank else { continue };
            for t in acct["transactions"].as_array().into_iter().flatten() {
                let posted = t["posted"].as_i64().unwrap_or(0);
                if posted == 0 {
                    continue; // pending; it comes back with a posted date later
                }
                let key = format!("{fid}/{}", s(t, "id"));
                let seen = self
                    .db
                    .query_row("SELECT 1 FROM feed_txns WHERE feed_id = ?1", [&key], |_| Ok(()))
                    .optional()
                    .map_err(|e| e.to_string())?
                    .is_some();
                if seen {
                    skipped += 1;
                    continue;
                }
                let date = chrono::DateTime::from_timestamp(posted, 0)
                    .ok_or("bad posted date")?
                    .with_timezone(&chrono::Local)
                    .format("%Y-%m-%d")
                    .to_string();
                let cents = parse_cents(&s(t, "amount")).ok_or(format!("bad amount in transaction {key}"))?;
                let memo = [s(t, "description"), s(t, "payee")].into_iter().find(|m| !m.is_empty()).unwrap_or_default();
                // Rows in a locked (filed) period are skipped for good, so they never block later syncs.
                let locked = self.check_open(&date).is_err();
                let entry = match cents {
                    _ if locked => None,
                    0 => None,
                    _ => Some(self.post_import(&date, &memo, bank, cents).map_err(|e| e.to_string())?),
                };
                if locked {
                    skipped += 1;
                    self.db.execute("INSERT INTO feed_txns(feed_id, entry_id) VALUES (?1, NULL)", [&key]).map_err(|e| e.to_string())?;
                    continue;
                }
                self.db.execute("INSERT INTO feed_txns(feed_id, entry_id) VALUES (?1, ?2)", params![key, entry]).map_err(|e| e.to_string())?;
                imported += 1;
            }
        }
        Ok((imported, skipped))
    }

    /// Imported money lands in Uncategorized Income/Expense until the user recategorizes it,
    /// The first matching import rule picks the category (and vendor/job) when there is one.
    fn post_import(&self, date: &str, memo: &str, bank: i64, cents: i64) -> Result<i64> {
        let rule = self.rule_for(memo)?;
        let cat = match &rule {
            Some(r) => r.account,
            None => self.system_account(if cents > 0 { "uncat_in" } else { "uncat_out" })?,
        };
        let entry = self.post_simple(date, memo, bank, cat, cents)?;
        if let Some(r) = rule {
            self.set_tags(entry, r.job, r.vendor)?;
        }
        Ok(entry)
    }

    fn invoice_balance(&self, invoice_id: i64) -> Result<i64> {
        self.db.query_row(
            "SELECT cents - (SELECT COALESCE(SUM(cents), 0) FROM payments WHERE invoice_id = ?1) FROM invoices WHERE id = ?1",
            [invoice_id],
            |r| r.get(0),
        )
    }

    /// A payment (full, partial, or a deposit) moves money from Accounts Receivable into the bank.
    pub fn record_payment(&self, invoice_id: i64, date: &str, bank: i64, cents: i64) -> Result<()> {
        let balance = self.invoice_balance(invoice_id)?;
        if cents <= 0 || cents > balance {
            return Err(rusqlite::Error::InvalidParameterName(format!("the payment must be between $0.01 and the balance, {}", fmt_cents(balance))));
        }
        let ar = self.system_account("ar")?;
        self.atomic(|| {
            let entry = self.post(date, &format!("Payment for invoice #{invoice_id}"), &[(bank, cents), (ar, -cents)])?;
            self.db.execute("INSERT INTO payments(invoice_id, entry_id, date, cents) VALUES (?1, ?2, ?3, ?4)", params![invoice_id, entry, date, cents])?;
            Ok(())
        })
    }

    /// Money a customer pays before there's an invoice. It isn't earned yet, so it's held in
    /// Customer Deposits (a liability) until applied to an invoice.
    pub fn record_deposit(&self, customer: i64, date: &str, bank: i64, cents: i64) -> Result<()> {
        if cents <= 0 {
            return Err(rusqlite::Error::InvalidParameterName("enter the deposit amount".into()));
        }
        let held = self.system_account("deposits")?;
        let name: String = self.db.query_row("SELECT name FROM customers WHERE id = ?1", [customer], |r| r.get(0))?;
        self.atomic(|| {
            let entry = self.post(date, &format!("Deposit from {name}"), &[(bank, cents), (held, -cents)])?;
            self.db.execute("INSERT INTO deposits(customer_id, entry_id, date, cents) VALUES (?1, ?2, ?3, ?4)", params![customer, entry, date, cents])?;
            Ok(())
        })
    }

    /// A customer's deposits not yet applied to invoices.
    pub fn deposit_available(&self, customer: i64) -> Result<i64> {
        self.db.query_row(
            "SELECT (SELECT COALESCE(SUM(cents), 0) FROM deposits WHERE customer_id = ?1)
                  - (SELECT COALESCE(SUM(p.cents), 0) FROM payments p JOIN invoices i ON i.id = p.invoice_id WHERE p.deposit = 1 AND i.customer_id = ?1)",
            [customer],
            |r| r.get(0),
        )
    }

    /// Uses the customer's held deposit toward an invoice (Dr Customer Deposits, Cr A/R). Returns the amount applied.
    pub fn apply_deposit(&self, invoice_id: i64, date: &str) -> Result<i64> {
        let customer: i64 = self.db.query_row("SELECT customer_id FROM invoices WHERE id = ?1", [invoice_id], |r| r.get(0))?;
        let cents = self.invoice_balance(invoice_id)?.min(self.deposit_available(customer)?);
        if cents <= 0 {
            return Err(rusqlite::Error::InvalidParameterName("there's no deposit to apply".into()));
        }
        let (held, ar) = (self.system_account("deposits")?, self.system_account("ar")?);
        self.atomic(|| {
            let entry = self.post(date, &format!("Deposit applied to invoice #{invoice_id}"), &[(held, cents), (ar, -cents)])?;
            self.db.execute(
                "INSERT INTO payments(invoice_id, entry_id, date, cents, deposit) VALUES (?1, ?2, ?3, ?4, 1)",
                params![invoice_id, entry, date, cents],
            )?;
            Ok(cents)
        })
    }

    /// Pays whatever is still owed. Paying a paid invoice is a no-op.
    #[cfg(test)]
    pub fn mark_paid(&self, invoice_id: i64, date: &str, bank: i64) -> Result<()> {
        match self.invoice_balance(invoice_id)? {
            b if b > 0 => self.record_payment(invoice_id, date, bank, b),
            _ => Ok(()),
        }
    }

    pub fn invoices(&self) -> Result<Vec<Invoice>> {
        let mut st = self.db.prepare(
            "SELECT i.id, c.name, i.date, i.description, i.cents,
               (SELECT COALESCE(SUM(cents), 0) FROM payments WHERE invoice_id = i.id), i.due_date, i.customer_id,
               (SELECT job_id FROM entries WHERE id = i.entry_id)
             FROM invoices i JOIN customers c ON c.id = i.customer_id ORDER BY i.id DESC",
        )?;
        let rows = st.query_map([], |r| {
            let (cents, paid_cents): (i64, i64) = (r.get(4)?, r.get(5)?);
            Ok(Invoice {
                id: r.get(0)?,
                number: r.get(0)?,
                customer_id: r.get(7)?,
                customer: r.get(1)?,
                date: r.get(2)?,
                description: r.get(3)?,
                cents,
                paid_cents,
                due_date: r.get(6)?,
                job: r.get(8)?,
                paid: paid_cents >= cents,
            })
        })?;
        rows.collect()
    }

    /// Imports a bank CSV (columns are 0-based) into Uncategorized Income/Expense.
    /// Rows already present (same bank, date, memo and amount) are skipped so re-importing is safe.
    /// Returns (imported, skipped).
    pub fn import_csv(
        &self,
        csv_text: &str,
        bank: i64,
        has_header: bool,
        date_col: usize,
        memo_col: usize,
        amount_col: usize,
    ) -> std::result::Result<(usize, usize), String> {
        let mut rdr = csv::ReaderBuilder::new().has_headers(has_header).flexible(true).from_reader(csv_text.as_bytes());
        let mut parsed = vec![];
        for (i, rec) in rdr.records().enumerate() {
            let rec = rec.map_err(|e| e.to_string())?;
            let row = i + 1 + has_header as usize;
            let get = |c: usize| rec.get(c).map(str::trim).ok_or(format!("row {row}: missing column {}", c + 1));
            let date = parse_date(get(date_col)?).ok_or(format!("row {row}: bad date {:?}", get(date_col)?))?;
            let cents = parse_cents(get(amount_col)?).ok_or(format!("row {row}: bad amount {:?}", get(amount_col)?))?;
            if cents != 0 {
                parsed.push((date, get(memo_col)?.to_string(), cents));
            }
        }
        // All rows parse before anything is written, so a bad file imports nothing.
        // Two identical rows in one file (two $4.50 coffees on the same day) are both real, so a row is a
        // duplicate only up to the number of matching entries already in the books before this import.
        let mut already: std::collections::HashMap<(String, String, i64), i64> = std::collections::HashMap::new();
        for (date, memo, cents) in &parsed {
            let key = (date.clone(), memo.clone(), *cents);
            if let std::collections::hash_map::Entry::Vacant(e) = already.entry(key) {
                let n: i64 = self
                    .db
                    .query_row(
                        "SELECT COUNT(DISTINCT e.id) FROM entries e JOIN lines l ON l.entry_id = e.id
                         WHERE e.date = ?1 AND e.memo = ?2 AND l.account_id = ?3 AND l.cents = ?4",
                        params![date, memo, bank, cents],
                        |r| r.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                e.insert(n);
            }
        }
        let (mut imported, mut skipped) = (0, 0);
        self.atomic(|| {
            for (date, memo, cents) in parsed {
                let Some(left) = already.get_mut(&(date.clone(), memo.clone(), cents)) else { continue };
                if *left > 0 || self.check_open(&date).is_err() {
                    *left -= 1;
                    skipped += 1;
                    continue;
                }
                self.post_import(&date, &memo, bank, cents)?;
                imported += 1;
            }
            Ok(())
        })
        .map_err(|e| e.to_string())?;
        Ok((imported, skipped))
    }
}

/// Totals line items; the description summary is what the ledger memo shows.
fn summarize(items: &[Item]) -> Result<(String, i64)> {
    let total: i64 = items.iter().map(|i| i.1).sum();
    if items.is_empty() || total <= 0 {
        return Err(rusqlite::Error::InvalidParameterName("add at least one line; the total must be positive".into()));
    }
    Ok((items.iter().map(|i| i.0.trim()).filter(|d| !d.is_empty()).collect::<Vec<_>>().join("; "), total))
}

/// Sales tax on a subtotal, rounded half away from zero to the cent.
pub fn tax_cents(subtotal: i64, bp: i64) -> i64 {
    let x = subtotal * bp;
    (x + x.signum() * 5_000) / 10_000
}

/// "8.25" or "8.25%" -> 825 basis points.
pub fn parse_rate_bp(s: &str) -> Option<i64> {
    let s = s.trim().trim_end_matches('%').trim();
    if s.is_empty() {
        return Some(0);
    }
    let (w, f) = s.split_once('.').unwrap_or((s, ""));
    if f.len() > 2 || !w.chars().chain(f.chars()).all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(w.parse::<i64>().unwrap_or(0) * 100 + format!("{f:0<2}").parse::<i64>().ok()?)
}

fn image_mime(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"\x89PNG") {
        Some("image/png")
    } else if b.starts_with(&[0xFF, 0xD8]) {
        Some("image/jpeg")
    } else {
        None
    }
}

pub fn net_income(pl: &Report) -> i64 {
    pl.rows.iter().map(|(k, _, c)| if k == "income" { *c } else { -*c }).sum()
}

/// Accepts "1,234.56", "-12.3", "$5", "(40.00)" (accounting negative).
pub fn parse_cents(s: &str) -> Option<i64> {
    let s = s.trim();
    let (neg, s) = match s.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        Some(inner) => (true, inner),
        None => (false, s),
    };
    let s: String = s.chars().filter(|c| !matches!(c, ',' | '$' | ' ')).collect();
    let (neg, s) = match s.strip_prefix('-') {
        Some(rest) => (!neg, rest.to_string()),
        None => (neg, s),
    };
    let (whole, frac) = s.split_once('.').unwrap_or((&s, ""));
    if whole.is_empty() && frac.is_empty() || frac.len() > 2 || !(whole.chars().chain(frac.chars()).all(|c| c.is_ascii_digit())) {
        return None;
    }
    let cents = whole.parse::<i64>().unwrap_or(0).checked_mul(100)? + format!("{frac:0<2}").parse::<i64>().ok()?;
    Some(if neg { -cents } else { cents })
}

pub fn fmt_cents(c: i64) -> String {
    let sign = if c < 0 { "-" } else { "" };
    let (d, r) = (c.unsigned_abs() / 100, c.unsigned_abs() % 100);
    let digits = d.to_string();
    let mut grouped = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    format!("{sign}${grouped}.{r:02}")
}

/// "2026-11-04" becomes "Nov 4, 2026" wherever an ISO date appears in `s`. For people only:
/// the database and CSV exports keep ISO dates, which sort and import cleanly.
// shortcut: US style only, fine while the app is US-focused; use the system locale if it goes international.
pub fn nice_dates(s: &str) -> String {
    let b = s.as_bytes();
    let not_digit = |c: Option<&u8>| c.is_none_or(|c| !c.is_ascii_digit());
    let iso = |i: usize| -> Option<NaiveDate> {
        let t = b.get(i..i + 10)?;
        let shape = t.iter().enumerate().all(|(k, c)| if k == 4 || k == 7 { *c == b'-' } else { c.is_ascii_digit() });
        let alone = not_digit(i.checked_sub(1).and_then(|j| b.get(j))) && not_digit(b.get(i + 10));
        if !(shape && alone) {
            return None;
        }
        NaiveDate::parse_from_str(s.get(i..i + 10)?, "%Y-%m-%d").ok()
    };
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while let Some(c) = s.get(i..).and_then(|r| r.chars().next()) {
        match iso(i) {
            Some(d) => {
                out += &d.format("%b %-d, %Y").to_string();
                i += 10;
            }
            None => {
                out.push(c);
                i += c.len_utf8();
            }
        }
    }
    out
}

/// Normalizes common bank date formats to ISO "YYYY-MM-DD" (which sorts correctly as text).
pub fn parse_date(s: &str) -> Option<String> {
    ["%Y-%m-%d", "%m/%d/%Y", "%m/%d/%y", "%Y/%m/%d", "%d-%b-%Y", "%b %d, %Y"]
        .iter()
        .find_map(|f| NaiveDate::parse_from_str(s.trim(), f).ok())
        .map(|d| d.format("%Y-%m-%d").to_string())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn friendly_dates() {
        assert_eq!(nice_dates("2026-11-04"), "Nov 4, 2026");
        assert_eq!(nice_dates("Profit & Loss, 2026-01-01 to 2026-12-31"), "Profit & Loss, Jan 1, 2026 to Dec 31, 2026");
        assert_eq!(nice_dates("PO 12026-11-04 · 2026-13-01 · é 2026-02-03"), "PO 12026-11-04 · 2026-13-01 · é Feb 3, 2026");
    }

    pub(crate) fn doc(customer_id: i64, date: &str, items: &[Item], job: Option<i64>) -> DocInput {
        DocInput { customer_id, date: date.into(), items: items.to_vec(), job, ..Default::default() }
    }

    fn id(b: &Books, n: &str) -> i64 {
        b.account_id(n).unwrap()
    }

    #[test]
    fn money_parsing_and_formatting() {
        assert_eq!(parse_cents("1,234.56"), Some(123456));
        assert_eq!(parse_cents("-12.3"), Some(-1230));
        assert_eq!(parse_cents("(40.00)"), Some(-4000));
        assert_eq!(parse_cents("$5"), Some(500));
        assert_eq!(parse_cents(".5"), Some(50));
        assert_eq!(parse_cents("1.234"), None);
        assert_eq!(parse_cents("abc"), None);
        assert_eq!(parse_cents(""), None);
        assert_eq!(fmt_cents(-123456789), "-$1,234,567.89");
        assert_eq!(parse_date("3/7/2026").as_deref(), Some("2026-03-07"));
    }

    #[test]
    fn unbalanced_entries_are_rejected() {
        let b = Books::open(":memory:").unwrap();
        assert!(b.post("2026-01-01", "bad", &[(id(&b, "Checking"), 100), (id(&b, "Sales"), -99)]).is_err());
        assert!(b.entries().unwrap().is_empty());
    }

    #[test]
    fn full_cycle_balances() {
        let b = Books::open(":memory:").unwrap();
        let (chk, card) = (id(&b, "Checking"), id(&b, "Credit Card"));
        b.post_simple("2026-01-01", "Seed money", chk, id(&b, "Owner's Contributions"), 100_000).unwrap();
        b.post_simple("2026-01-05", "Adobe", card, id(&b, "Software & Subscriptions"), -5_000).unwrap();
        b.add_customer("Acme", "").unwrap();
        let cust = b.customers().unwrap()[0].id;
        let inv = b.create_invoice(&doc(cust, "2026-01-10", &[("Logo design".into(), 80_000)], None)).unwrap();
        b.mark_paid(inv, "2026-01-20", chk).unwrap();
        b.mark_paid(inv, "2026-01-21", chk).unwrap(); // paying twice is a no-op

        let csv = "Date,Description,Amount\n01/25/2026,COFFEE SHOP,-4.50\n01/26/2026,\"Client, Inc\",\"1,000.00\"\n";
        assert_eq!(b.import_csv(csv, chk, true, 0, 1, 2), Ok((2, 0)));
        assert_eq!(b.import_csv(csv, chk, true, 0, 1, 2), Ok((0, 2)));
        assert!(b.import_csv("x,y,zz\n", chk, false, 0, 1, 2).is_err());

        let pl = b.profit_and_loss("2026-01-01", "2026-01-31").unwrap();
        assert_eq!(net_income(&pl), 80_000 + 100_000 - 5_000 - 450);

        let bs = b.balance_sheet("2026-12-31").unwrap();
        let side = |k: &str| bs.rows.iter().filter(|r| r.0 == k).map(|r| r.2).sum::<i64>();
        assert_eq!(side("asset"), side("liability") + side("equity"));
        assert_eq!(side("asset"), 100_000 + 80_000 + 100_000 - 450);
        assert!(b.delete_entry(b.entries().unwrap().iter().find(|e| e.memo.starts_with("Invoice")).unwrap().id).is_err());
    }

    #[test]
    fn quotes_documents_and_feeds() {
        let b = Books::open(":memory:").unwrap();
        b.add_customer("Bob <&> Co", "bob@x.test").unwrap();
        let cust = b.customers().unwrap()[0].id;
        let items = vec![("Design".to_string(), 50_000), ("Discount".to_string(), -5_000)];
        assert!(b.create_quote(&doc(cust, "2026-02-01", &[("Free".into(), 0)], None)).is_err());
        assert!(b.jobs().unwrap().is_empty(), "a rejected quote leaves no job behind");
        let q = b.create_quote(&doc(cust, "2026-02-01", &items, None)).unwrap();
        b.create_quote(&doc(cust, "2026-02-02", &items, None)).unwrap();
        let names: Vec<String> = b.jobs().unwrap().into_iter().map(|j| j.name).collect();
        assert_eq!(names, ["Bob <&> Co: Design", "Bob <&> Co: Design (quote #2)"]);
        assert_eq!(b.quotes().unwrap()[0].job.as_deref(), Some("Bob <&> Co: Design (quote #2)"));
        assert!(b.entries().unwrap().is_empty(), "a quote posts nothing");
        let inv = b.convert_quote(q, "2026-02-05", "").unwrap();
        assert!(b.convert_quote(q, "2026-02-06", "").is_err());
        assert_eq!(b.invoices().unwrap()[0].cents, 45_000);
        assert_eq!(b.jobs().unwrap()[0].income, 45_000, "the converted invoice counts toward the quote's job");
        assert_eq!(b.items(Doc::Invoice, inv).unwrap(), items);
        assert_eq!(b.quotes().unwrap().iter().find(|x| x.id == q).unwrap().invoice_id, Some(inv));

        assert!(b.set_logo(Some(b"not an image")).is_err());
        b.set_logo(Some(b"\x89PNG fake")).unwrap();
        let biz = Business { name: "Me LLC".into(), street: "1 Main St".into(), city: "Austin".into(), state: "TX".into(), zip: "78701".into(), website: "me.test".into(), ..Default::default() };
        b.save_business(&biz).unwrap();
        let s = b.settings().unwrap();
        assert_eq!(s.biz, Business { extra: String::new(), ..biz });
        assert_eq!(s.address, "1 Main St\nAustin, TX 78701\nme.test");
        let (to, subject, body) = b.document_email(Doc::Quote, q, false).unwrap();
        assert_eq!((to.as_str(), subject.as_str()), ("bob@x.test", "Quote #1 from Me LLC"));
        assert!(body.contains("Discount: -$50.00") && body.contains("Total: $450.00"));
        b.add_vendor("Adobe", "").unwrap();
        b.add_job("Kitchen remodel", Some(cust)).unwrap();
        assert!(b.add_job(" ", None).is_err());
        let job = b.jobs().unwrap().into_iter().find(|j| j.name == "Kitchen remodel").unwrap().id;
        b.create_invoice(&doc(cust, "2026-03-01", &[("Remodel".into(), 300_000)], Some(job))).unwrap();
        let lumber = b.post_simple("2026-03-02", "Lumber", id(&b, "Checking"), id(&b, "Office Supplies"), -40_000).unwrap();
        b.set_tags(lumber, Some(job), Some(b.vendors().unwrap()[0].id)).unwrap();
        let j = b.jobs().unwrap().into_iter().find(|j| j.id == job).unwrap();
        assert_eq!((j.customer.as_deref(), j.income, j.expenses), (Some("Bob <&> Co"), 300_000, 40_000));
        assert!(b.entries().unwrap().iter().any(|e| e.memo == "Lumber" && e.job == Some(job) && e.vendor.is_some()));
        assert!(b.add_vendor("  ", "").is_err());
        assert_eq!(b.vendors().unwrap()[0].name, "Adobe");
        let blocks = b.document_blocks(Doc::Invoice, inv).unwrap();
        let text = crate::pdf::plain_text(&blocks);
        assert!(text.contains("Bob <&> Co") && text.contains("$450.00"));
        assert!(matches!(&blocks[0], crate::pdf::Block::Letterhead { logo: Some(l), .. } if l.starts_with(&[0x89, b'P', b'N', b'G'])), "logo on the letterhead");
        assert!(b.document_pdf(Doc::Invoice, inv).unwrap().starts_with(b"%PDF"));

        let feed = r#"{"errors":[],"accounts":[{"id":"A1","name":"Checking","org":{"name":"My Bank"},
          "transactions":[{"id":"t1","posted":1767312000,"amount":"-12.50","description":"LUNCH"},
                          {"id":"t2","posted":0,"amount":"-1.00","description":"PENDING"}]}]}"#;
        assert_eq!(b.import_feed(feed), Ok((0, 0)), "unmapped accounts post nothing");
        let fa = b.feed_accounts().unwrap();
        assert_eq!((fa[0].1.as_str(), fa[0].2), ("My Bank Checking", None));
        b.map_feed_account("A1", id(&b, "Checking")).unwrap();
        assert_eq!(b.import_feed(feed), Ok((1, 0)));
        let lunch = b.entries().unwrap().into_iter().find(|e| e.memo == "LUNCH").unwrap();
        b.delete_entry(lunch.id).unwrap();
        assert_eq!(b.import_feed(feed), Ok((0, 1)), "a deleted transaction stays deleted");
    }

    #[test]
    fn chart_of_accounts_editing() {
        let b = Books::open(":memory:").unwrap();
        let all = b.accounts().unwrap();
        assert_eq!(all.len(), DEFAULT_ACCOUNTS.len());
        assert_eq!((all[0].number.as_str(), all[0].name.as_str()), ("1000", "Checking"));

        // Renaming a system account keeps invoicing working.
        let mut ar = all.iter().find(|a| a.system.as_deref() == Some("ar")).unwrap().clone();
        ar.name = "A/R".into();
        b.update_account(&ar).unwrap();
        b.add_customer("Acme", "").unwrap();
        b.create_invoice(&doc(b.customers().unwrap()[0].id, "2026-01-01", &[("Work".into(), 100)], None)).unwrap();
        ar.active = false;
        assert!(b.update_account(&ar).is_err(), "system accounts stay on");
        ar.active = true;
        ar.kind = "expense".into();
        assert!(b.update_account(&ar).is_err(), "used accounts keep their type");
        assert!(b.delete_account(ar.id).is_err());

        let id = b.add_account("6999", "Bad Debts", "expense", "write-offs").unwrap();
        assert!(b.add_account("", " ", "expense", "").is_err());
        let mut acct = b.accounts().unwrap().into_iter().find(|a| a.id == id).unwrap();
        acct.kind = "income".into();
        b.update_account(&acct).unwrap();
        b.delete_account(id).unwrap();
        assert!(b.accounts().unwrap().iter().all(|a| a.id != id));

        // Reopening never re-adds the standard chart over the user's choices.
        let path = std::env::temp_dir().join(format!("ac-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let p = path.to_str().unwrap();
        let first = Books::open(p).unwrap();
        let savings = first.accounts().unwrap().into_iter().find(|a| a.name == "Savings").unwrap().id;
        first.delete_account(savings).unwrap();
        drop(first);
        assert!(Books::open(p).unwrap().accounts().unwrap().iter().all(|a| a.name != "Savings"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn tax_partial_payments_and_edits() {
        let b = Books::open(":memory:").unwrap();
        let (chk, tax_acct) = (id(&b, "Checking"), id(&b, "Sales Tax Payable"));
        let cust = b.add_customer("Acme", "a@x.test").unwrap();
        assert_eq!((tax_cents(10_000, 825), tax_cents(1, 5_000), parse_rate_bp("8.25%")), (825, 1, Some(825)));
        let mut d = doc(cust, "2026-05-01", &[("Shelf".into(), 10_000)], None);
        d.tax_bp = 825;
        d.due_date = "2026-05-31".into();
        let inv = b.create_invoice(&d).unwrap();
        let i = &b.invoices().unwrap()[0];
        assert_eq!((i.cents, i.balance(), i.overdue("2026-06-01"), i.overdue("2026-05-31")), (10_825, 10_825, true, false));
        let bal = |a: i64| b.db.query_row("SELECT SUM(cents) FROM lines WHERE account_id = ?1", [a], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!(bal(tax_acct), -825, "tax is owed to the state, not income");

        assert!(b.record_payment(inv, "2026-05-10", chk, 20_000).is_err(), "can't overpay");
        b.record_payment(inv, "2026-05-10", chk, 5_000).unwrap();
        let i = &b.invoices().unwrap()[0];
        assert_eq!((i.paid, i.balance()), (false, 5_825));
        assert!(crate::pdf::plain_text(&b.document_blocks(Doc::Invoice, inv).unwrap()).contains("Balance due"));
        let (_, subj, body) = b.document_email(Doc::Invoice, inv, true).unwrap();
        assert!(subj.starts_with("Reminder") && body.contains("Balance due: $58.25"));
        b.mark_paid(inv, "2026-05-20", chk).unwrap();
        assert!(b.invoices().unwrap()[0].paid);
        assert_eq!(bal(chk), 10_825);
        let pay = b.db.query_row("SELECT entry_id FROM payments LIMIT 1", [], |r| r.get::<_, i64>(0)).unwrap();
        assert!(b.delete_entry(pay).is_err() && b.update_simple(pay, "2026-05-10", "x", chk, 1).is_err(), "payments belong to the invoice");

        // Editing a simple transaction keeps it balanced.
        let e = b.post_simple("2026-05-02", "Glue", chk, id(&b, "Job Materials"), -1_000).unwrap();
        b.update_simple(e, "2026-05-03", "Wood glue", chk, -1_250).unwrap();
        let got = b.entries().unwrap().into_iter().find(|x| x.id == e).unwrap();
        assert_eq!((got.date.as_str(), got.memo.as_str(), got.lines[0].2, got.lines[1].2), ("2026-05-03", "Wood glue", -1_250, 1_250));

        // A failed step leaves nothing half-written.
        let before = b.entries().unwrap().len();
        assert!(b.create_invoice(&doc(9999, "2026-05-04", &[("Ghost".into(), 100)], None)).is_err());
        assert_eq!(b.entries().unwrap().len(), before);
    }

    #[test]
    fn party_details_and_old_addresses() {
        let b = Books::open(":memory:").unwrap();
        let id = b.add_customer("Bob", "bob@x.test").unwrap();
        b.db.execute("UPDATE customers SET address = '9 Elm St\nAustin TX' WHERE id = ?1", [id]).unwrap();
        let bob = b.customers().unwrap().remove(0);
        assert_eq!(bob.info.street, "9 Elm St, Austin TX", "old free-text address shows in Street");
        let info = Business { name: "Bob's Diner LLC".into(), contact: "Bob Ray".into(), street: "9 Elm St".into(), city: "Austin".into(), state: "TX".into(), zip: "78701".into(), phone: "555-1234".into(), email: "bob@x.test".into(), ..Default::default() };
        b.save_party(false, &Party { info, ..bob }).unwrap();
        let bob = b.customers().unwrap().remove(0);
        assert_eq!(bob.address, "Attn: Bob Ray\n9 Elm St\nAustin, TX 78701");
        assert_eq!((bob.info.name.as_str(), bob.info.phone.as_str()), ("Bob's Diner LLC", "555-1234"));
        let v = Party { name: "Sam Sub".into(), is_1099: true, tax_id: "12-3456789".into(), ..Default::default() };
        let vid = b.save_party(true, &v).unwrap();
        let sam = b.vendors().unwrap().into_iter().find(|p| p.id == vid).unwrap();
        assert!(sam.is_1099 && sam.tax_id == "12-3456789");
        assert!(b.save_party(true, &Party { id: vid, ..Default::default() }).is_err(), "a name is required");
    }

    #[test]
    fn review_regressions() {
        let b = Books::open(":memory:").unwrap();
        let (chk, sales) = (id(&b, "Checking"), id(&b, "Sales"));
        // Two identical rows in one file are both real; re-importing still skips both.
        let csv = "D,M,A
01/05/2026,STARBUCKS,-4.50
01/05/2026,STARBUCKS,-4.50
";
        assert_eq!(b.import_csv(csv, chk, true, 0, 1, 2), Ok((2, 0)));
        assert_eq!(b.import_csv(csv, chk, true, 0, 1, 2), Ok((0, 2)));
        let three = "D,M,A
01/05/2026,STARBUCKS,-4.50
01/05/2026,STARBUCKS,-4.50
01/05/2026,STARBUCKS,-4.50
";
        assert_eq!(b.import_csv(three, chk, true, 0, 1, 2), Ok((1, 2)), "only the extra one is new");

        // Reconciled transactions can't be deleted, edited, or have their bank line moved.
        let lines = b.recon_lines(chk, "2026-01-31").unwrap();
        lines.iter().for_each(|l| b.tick(l.line_id, true).unwrap());
        b.finish_reconcile(chk, "2026-01-31", -1_350).unwrap();
        let e = b.entries().unwrap()[0].clone();
        assert!(b.delete_entry(e.id).is_err());
        assert!(b.update_simple(e.id, &e.date, "x", chk, -1).is_err());
        assert!(b.set_line_account(e.lines[0].0, id(&b, "Savings")).is_err());
        b.set_line_account(e.lines[1].0, id(&b, "Meals")).unwrap(); // the category side can still change

        // On an invoice only the income line may move, and only to income.
        let cust = b.add_customer("Acme", "").unwrap();
        b.create_invoice(&doc(cust, "2026-02-01", &[("Work".into(), 1_000)], None)).unwrap();
        let inv = b.entries().unwrap().into_iter().find(|e| e.invoice).unwrap();
        assert!(b.set_line_account(inv.lines[0].0, chk).is_err(), "A/R stays A/R");
        assert!(b.set_line_account(inv.lines[1].0, chk).is_err(), "income can't become a bank");
        b.set_line_account(inv.lines[1].0, id(&b, "Service Income")).unwrap();
        let _ = sales;

        // Quotes check the tax rate like invoices do.
        let mut q = doc(cust, "2026-02-01", &[("Work".into(), 1_000)], None);
        q.tax_bp = 99_999;
        assert!(b.create_quote(&q).is_err());

        // A lock date skips locked rows instead of stopping the import or the recurring run.
        let mut s = b.settings().unwrap();
        s.lock_date = "2026-02-28".into();
        b.save_prefs(&s).unwrap();
        assert_eq!(b.import_csv("D,M,A
02/10/2026,OLD,-1.00
03/10/2026,NEW,-2.00
", chk, true, 0, 1, 2), Ok((1, 1)));
        b.add_recurring_txn("month", "2026-01-15", "Rent", chk, id(&b, "Rent or Lease"), -10_000, None, None).unwrap();
        b.add_recurring_txn("month", "2026-02-20", "Bad", chk, 999_999, -1, None, None).unwrap();
        b.add_recurring_txn("month", "2026-02-20", "Phone", chk, id(&b, "Phone & Internet"), -5_000, None, None).unwrap();
        let r = b.run_recurring("2026-03-31");
        assert!(r.as_ref().is_err_and(|e| e.contains("posted 2")), "{r:?}"); // Feb rent skipped (locked), Mar rent + Mar phone posted
        assert!(b.entries().unwrap().iter().any(|e| e.memo == "Phone" && e.date == "2026-03-20"));
    }

    #[test]
    fn line_tax_deposits_and_job_costs() {
        let b = Books::open(":memory:").unwrap();
        let chk = id(&b, "Checking");
        let cust = b.add_customer("Acme", "").unwrap();
        // Labor untaxed, materials taxed at 10%.
        let mut d = doc(cust, "2026-06-01", &[("Labor".into(), 50_000), ("Materials".into(), 20_000)], None);
        d.tax_bp = 1_000;
        d.taxable = vec![false, true];
        let q = b.create_quote(&d).unwrap();
        assert!(crate::pdf::plain_text(&b.document_blocks(Doc::Quote, q).unwrap()).contains("Sales tax | $20.00"), "quote tax only on materials");
        let job = b.jobs().unwrap()[0].id;
        assert_eq!(b.jobs().unwrap()[0].quoted, 70_000);

        // A deposit is held as a liability, not income, until applied.
        b.record_deposit(cust, "2026-06-02", chk, 30_000).unwrap();
        assert_eq!(b.deposit_available(cust).unwrap(), 30_000);
        let held = |b: &Books| b.balance_sheet("2026-12-31").unwrap().rows.iter().find(|r| r.1 == "Customer Deposits").map_or(0, |r| r.2);
        assert_eq!(held(&b), 30_000);
        assert_eq!(net_income(&b.profit_and_loss("2026-01-01", "2026-12-31").unwrap()), 0);

        let inv = b.convert_quote(q, "2026-06-10", "").unwrap();
        let i = b.invoices().unwrap().into_iter().find(|i| i.id == inv).unwrap();
        assert_eq!(i.cents, 72_000, "tax carried over from the quote, per line");
        assert!(crate::pdf::plain_text(&b.document_blocks(Doc::Invoice, inv).unwrap()).contains("(not taxed)"));
        assert_eq!(b.apply_deposit(inv, "2026-06-10").unwrap(), 30_000);
        assert!(b.apply_deposit(inv, "2026-06-10").is_err(), "nothing left to apply");
        assert_eq!((held(&b), b.invoices().unwrap()[0].balance()), (0, 42_000));
        let dep_entry = b.db.query_row("SELECT entry_id FROM deposits", [], |r| r.get::<_, i64>(0)).unwrap();
        assert!(b.delete_entry(dep_entry).is_err(), "deposits belong to the customer record");

        // Job costs list what was tagged.
        let e = b.post_simple("2026-06-05", "Lumber", chk, id(&b, "Job Materials"), -8_000).unwrap();
        b.set_tags(e, Some(job), None).unwrap();
        assert_eq!(b.job_costs(job).unwrap(), vec![("2026-06-05".into(), "Lumber".into(), "Job Materials".into(), 8_000)]);
    }
}

