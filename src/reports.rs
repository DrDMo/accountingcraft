//! Reports as plain tables, so one renderer, one CSV export and one print page serve them all.

use crate::books::{fmt_cents, net_income, Books};
use crate::pdf::{Align, Block, Row as PdfRow, Style};
use rusqlite::{params, Result};

pub enum Cell {
    Text(String),
    Money(i64),
}

pub struct Row {
    pub cells: Vec<Cell>,
    pub strong: bool,
}

pub struct Table {
    pub title: String,
    pub columns: Vec<&'static str>,
    pub rows: Vec<Row>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    ProfitLoss,
    BalanceSheet,
    TrialBalance,
    GeneralLedger,
    ArAging,
    SalesByCustomer,
    ExpensesByVendor,
    JobProfit,
    SalesTax,
    ScheduleC,
    Vendors1099,
    EstimatedTax,
    Mileage,
    Assets,
}

pub const ALL: [(Kind, &str); 14] = [
    (Kind::ProfitLoss, "Profit & Loss"),
    (Kind::BalanceSheet, "Balance Sheet"),
    (Kind::TrialBalance, "Trial Balance"),
    (Kind::GeneralLedger, "General Ledger"),
    (Kind::ArAging, "Accounts Receivable Aging"),
    (Kind::SalesByCustomer, "Sales by Customer"),
    (Kind::ExpensesByVendor, "Expenses by Vendor"),
    (Kind::JobProfit, "Job Profitability"),
    (Kind::SalesTax, "Sales Tax Summary"),
    (Kind::ScheduleC, "Schedule C Summary (tax time)"),
    (Kind::Vendors1099, "1099 Vendors"),
    (Kind::EstimatedTax, "Estimated Quarterly Tax"),
    (Kind::Mileage, "Mileage Log"),
    (Kind::Assets, "Fixed Assets & Depreciation"),
];

impl Kind {
    /// Reports that describe one moment use only the end date.
    pub fn as_of(self) -> bool {
        matches!(self, Kind::BalanceSheet | Kind::TrialBalance | Kind::ArAging | Kind::Assets)
    }
}

fn t(s: impl Into<String>) -> Cell {
    Cell::Text(s.into())
}
fn row(cells: Vec<Cell>) -> Row {
    Row { cells, strong: false }
}
fn bold(cells: Vec<Cell>) -> Row {
    Row { cells, strong: true }
}
/// Debit-normal accounts read positive with a debit balance, the rest with a credit balance.
fn natural(kind: &str, cents: i64) -> i64 {
    if kind == "asset" || kind == "expense" { cents } else { -cents }
}
/// One ledger line with who it was with and the other side of the entry, for detail rows.
struct Line {
    acct: String,
    kind: String,
    date: String,
    memo: String,
    party: String,
    split: String,
    cents: i64,
    vendor: Option<i64>,
    job: Option<i64>,
}

fn lines(b: &Books, cond: &str, p: impl rusqlite::Params) -> Result<Vec<Line>> {
    let sql = format!(
        "SELECT a.name, a.kind, e.date, e.memo,
           COALESCE(v.name,
             (SELECT c.name FROM invoices i JOIN customers c ON c.id = i.customer_id WHERE i.entry_id = e.id),
             (SELECT c.name FROM payments p JOIN invoices i ON i.id = p.invoice_id JOIN customers c ON c.id = i.customer_id WHERE p.entry_id = e.id),
             ''),
           COALESCE((SELECT group_concat(a2.name, ', ') FROM lines l2 JOIN accounts a2 ON a2.id = l2.account_id
                     WHERE l2.entry_id = e.id AND l2.id != l.id), ''),
           l.cents, e.vendor_id, e.job_id
         FROM lines l JOIN entries e ON e.id = l.entry_id JOIN accounts a ON a.id = l.account_id
         LEFT JOIN vendors v ON v.id = e.vendor_id
         WHERE {cond} ORDER BY e.date, e.id"
    );
    let mut st = b.db.prepare(&sql)?;
    let v = st.query_map(p, |r| {
        Ok(Line {
            acct: r.get(0)?, kind: r.get(1)?, date: r.get(2)?, memo: r.get(3)?, party: r.get(4)?,
            split: r.get(5)?, cents: r.get(6)?, vendor: r.get(7)?, job: r.get(8)?,
        })
    })?;
    v.collect()
}

fn clone_line(l: &Line) -> Line {
    Line {
        acct: l.acct.clone(), kind: l.kind.clone(), date: l.date.clone(), memo: l.memo.clone(), party: l.party.clone(),
        split: l.split.clone(), cents: l.cents, vendor: l.vendor, job: l.job,
    }
}

/// "date   memo · party · other", indented under its summary row.
fn line_text(l: &Line, other: &str) -> String {
    let mut s = format!("        {}   {}", l.date, l.memo);
    for x in [l.party.as_str(), other] {
        if !x.is_empty() {
            s += &format!(" · {x}");
        }
    }
    s
}

/// A detail row: text in the first column, money in column `col`, blanks elsewhere.
fn line_row(text: String, width: usize, col: usize, cents: i64) -> Row {
    let mut cells: Vec<Cell> = (0..width).map(|_| t("")).collect();
    cells[0] = t(text);
    cells[col] = Cell::Money(cents);
    row(cells)
}

const KIND_ORDER: &str = "CASE a.kind WHEN 'asset' THEN 0 WHEN 'liability' THEN 1 WHEN 'equity' THEN 2 WHEN 'income' THEN 3 ELSE 4 END";

/// `account` narrows the General Ledger; `job` narrows Job Profitability to one job.
/// `detail` adds the transactions (or invoices, accounts, years) behind each summary row.
pub fn run(b: &Books, kind: Kind, from: &str, to: &str, account: Option<i64>, job: Option<i64>, detail: bool) -> Result<Table> {
    let range = format!("{from} to {to}");
    let name = ALL.iter().find(|k| k.0 == kind).map_or("", |k| k.1);
    let name = if detail { format!("{name} (detail)") } else { name.to_string() };
    let title = if kind.as_of() { format!("{name}, as of {to}") } else { format!("{name}, {range}") };
    let mut rows = vec![];
    let columns = match kind {
        Kind::ProfitLoss => {
            let pl = b.profit_and_loss(from, to)?;
            let ls = if detail { lines(b, "a.kind IN ('income', 'expense') AND e.date BETWEEN ?1 AND ?2", params![from, to])? } else { vec![] };
            for (k, label) in [("income", "Income"), ("expense", "Expenses")] {
                rows.push(bold(vec![t(label), t("")]));
                let mut total = 0;
                for (_, n, c) in pl.rows.iter().filter(|r| r.0 == k) {
                    rows.push(row(vec![t(format!("    {n}")), Cell::Money(*c)]));
                    for l in ls.iter().filter(|l| &l.acct == n) {
                        rows.push(line_row(line_text(l, &l.split), 2, 1, natural(k, l.cents)));
                    }
                    total += c;
                }
                rows.push(bold(vec![t(format!("Total {}", label.to_lowercase())), Cell::Money(total)]));
            }
            rows.push(bold(vec![t("Net income"), Cell::Money(net_income(&pl))]));
            vec!["Account", "Amount"]
        }
        Kind::BalanceSheet => {
            let bs = b.balance_sheet(to)?;
            let ls = if detail { lines(b, "a.kind IN ('asset', 'liability', 'equity') AND e.date <= ?1", [to])? } else { vec![] };
            let mut totals = vec![];
            for (k, label) in [("asset", "Assets"), ("liability", "Liabilities"), ("equity", "Equity")] {
                rows.push(bold(vec![t(label), t("")]));
                let mut total = 0;
                for (_, n, c) in bs.rows.iter().filter(|r| r.0 == k) {
                    rows.push(row(vec![t(format!("    {n}")), Cell::Money(*c)]));
                    for l in ls.iter().filter(|l| &l.acct == n) {
                        rows.push(line_row(line_text(l, &l.split), 2, 1, natural(k, l.cents)));
                    }
                    total += c;
                }
                rows.push(bold(vec![t(format!("Total {}", label.to_lowercase())), Cell::Money(total)]));
                totals.push(total);
            }
            rows.push(bold(vec![t("Total liabilities + equity"), Cell::Money(totals[1] + totals[2])]));
            vec!["Account", "Amount"]
        }
        Kind::TrialBalance => {
            let mut st = b.db.prepare(&format!(
                "SELECT a.number, a.name, SUM(l.cents) FROM lines l JOIN entries e ON e.id = l.entry_id
                 JOIN accounts a ON a.id = l.account_id WHERE e.date <= ?1
                 GROUP BY a.id HAVING SUM(l.cents) != 0 ORDER BY {KIND_ORDER}, a.number = '', a.number, a.name"
            ))?;
            let ls = if detail { lines(b, "e.date <= ?1", [to])? } else { vec![] };
            let (mut dr, mut cr) = (0, 0);
            for r in st.query_map([to], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))? {
                let (num, n, c) = r?;
                let (d, k) = if c > 0 { (Cell::Money(c), t("")) } else { (t(""), Cell::Money(-c)) };
                if c > 0 { dr += c } else { cr -= c }
                for l in ls.iter().filter(|l| l.acct == n) {
                    let (col, v) = if l.cents > 0 { (2, l.cents) } else { (3, -l.cents) };
                    rows.push(line_row(line_text(l, &l.split), 4, col, v));
                }
                rows.push(row(vec![t(num), t(n), d, k]));
            }
            rows.push(bold(vec![t(""), t("Total"), Cell::Money(dr), Cell::Money(cr)]));
            vec!["No.", "Account", "Debit", "Credit"]
        }
        Kind::GeneralLedger => {
            let mut accts = b.db.prepare(&format!(
                "SELECT a.id, a.number, a.name, a.kind FROM accounts a
                 WHERE (?1 IS NULL OR a.id = ?1) AND EXISTS (SELECT 1 FROM lines l WHERE l.account_id = a.id)
                 ORDER BY {KIND_ORDER}, a.number = '', a.number, a.name"
            ))?;
            let list: Vec<(i64, String, String, String)> =
                accts.query_map([account], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_>>()?;
            for (id, num, n, k) in list {
                let opening: i64 = b.db.query_row(
                    "SELECT COALESCE(SUM(l.cents), 0) FROM lines l JOIN entries e ON e.id = l.entry_id WHERE l.account_id = ?1 AND e.date < ?2",
                    params![id, from],
                    |r| r.get(0),
                )?;
                let mut bal = natural(&k, opening);
                let activity = lines(b, "l.account_id = ?1 AND e.date BETWEEN ?2 AND ?3", params![id, from, to])?;
                if activity.is_empty() && opening == 0 {
                    continue;
                }
                rows.push(bold(vec![t(format!("{num} {n}").trim().to_string()), t(""), t(""), t("")]));
                rows.push(row(vec![t(from), t("Opening balance"), t(""), Cell::Money(bal)]));
                for l in activity {
                    bal += natural(&k, l.cents);
                    let memo = if detail { line_text(&Line { date: String::new(), ..clone_line(&l) }, &l.split).trim().to_string() } else { l.memo.clone() };
                    rows.push(row(vec![t(l.date), t(memo), Cell::Money(natural(&k, l.cents)), Cell::Money(bal)]));
                }
                rows.push(bold(vec![t(to), t("Closing balance"), t(""), Cell::Money(bal)]));
            }
            vec!["Date", "Description", "Amount", "Balance"]
        }
        Kind::ArAging => {
            // Unpaid as of the date: issued by then, and not paid, or paid only later.
            let mut st = b.db.prepare(
                "SELECT name, owed, days, id, date FROM (
                   SELECT c.name AS name, i.id AS id, i.date AS date, CAST(julianday(?1) - julianday(i.date) AS INTEGER) AS days,
                     i.cents - (SELECT COALESCE(SUM(p.cents), 0) FROM payments p WHERE p.invoice_id = i.id AND p.date <= ?1) AS owed
                   FROM invoices i JOIN customers c ON c.id = i.customer_id WHERE i.date <= ?1)
                 WHERE owed > 0 ORDER BY name, date",
            )?;
            let mut by: Vec<(String, [i64; 4], Vec<Row>)> = vec![];
            for r in st.query_map([to], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, String>(4)?)))? {
                let (c, cents, days, id, date) = r?;
                let bucket = match days { ..=30 => 0, 31..=60 => 1, 61..=90 => 2, _ => 3 };
                if by.last().is_none_or(|l| l.0 != c) {
                    by.push((c, [0; 4], vec![]));
                }
                if let Some(last) = by.last_mut() {
                    last.1[bucket] += cents;
                    let mut r = line_row(format!("        Invoice #{id}   {date} · {days} days old"), 6, bucket + 1, cents);
                    r.cells[5] = Cell::Money(cents);
                    last.2.push(r);
                }
            }
            let mut tot = [0; 4];
            for (c, v, inv) in by {
                (0..4).for_each(|i| tot[i] += v[i]);
                rows.push(row(vec![t(c), Cell::Money(v[0]), Cell::Money(v[1]), Cell::Money(v[2]), Cell::Money(v[3]), Cell::Money(v.iter().sum())]));
                if detail {
                    rows.extend(inv);
                }
            }
            rows.push(bold(vec![
                t("Total"), Cell::Money(tot[0]), Cell::Money(tot[1]), Cell::Money(tot[2]), Cell::Money(tot[3]), Cell::Money(tot.iter().sum()),
            ]));
            vec!["Customer", "0–30 days", "31–60", "61–90", "Over 90", "Total"]
        }
        Kind::SalesByCustomer => {
            let mut st = b.db.prepare(
                "SELECT c.name, COUNT(*), SUM(i.cents), SUM(i.cents - (SELECT COALESCE(SUM(p.cents), 0) FROM payments p WHERE p.invoice_id = i.id)), c.id
                 FROM invoices i JOIN customers c ON c.id = i.customer_id
                 WHERE i.date BETWEEN ?1 AND ?2 GROUP BY c.id ORDER BY SUM(i.cents) DESC",
            )?;
            let mut inv = b.db.prepare(
                "SELECT i.id, i.date, i.description, i.cents, i.cents - (SELECT COALESCE(SUM(p.cents), 0) FROM payments p WHERE p.invoice_id = i.id)
                 FROM invoices i WHERE i.customer_id = ?1 AND i.date BETWEEN ?2 AND ?3 ORDER BY i.date, i.id",
            )?;
            let (mut n, mut s, mut u) = (0, 0, 0);
            let found: Vec<(String, i64, i64, i64, i64)> = st
                .query_map(params![from, to], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
                .collect::<Result<_>>()?;
            for (c, cnt, sold, unpaid, cid) in found {
                (n, s, u) = (n + cnt, s + sold, u + unpaid);
                rows.push(row(vec![t(c), t(cnt.to_string()), Cell::Money(sold), Cell::Money(unpaid)]));
                if detail {
                    for r in inv.query_map(params![cid, from, to], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(4)?)))? {
                        let (id, d, desc, cents, open) = r?;
                        let mut r = line_row(format!("        Invoice #{id}   {d} · {desc}"), 4, 2, cents);
                        r.cells[3] = Cell::Money(open);
                        rows.push(r);
                    }
                }
            }
            rows.push(bold(vec![t("Total"), t(n.to_string()), Cell::Money(s), Cell::Money(u)]));
            vec!["Customer", "Invoices", "Invoiced", "Still unpaid"]
        }
        Kind::ExpensesByVendor => {
            let mut st = b.db.prepare(
                "SELECT COALESCE(v.name, '(no vendor)'), COUNT(DISTINCT e.id), SUM(l.cents), v.id
                 FROM entries e JOIN lines l ON l.entry_id = e.id JOIN accounts a ON a.id = l.account_id AND a.kind = 'expense'
                 LEFT JOIN vendors v ON v.id = e.vendor_id
                 WHERE e.date BETWEEN ?1 AND ?2 GROUP BY v.id ORDER BY v.id IS NULL, SUM(l.cents) DESC",
            )?;
            let ls = if detail { lines(b, "a.kind = 'expense' AND e.date BETWEEN ?1 AND ?2", params![from, to])? } else { vec![] };
            let (mut n, mut s) = (0, 0);
            for r in st.query_map(params![from, to], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, Option<i64>>(3)?)))? {
                let (v, cnt, spent, vid) = r?;
                (n, s) = (n + cnt, s + spent);
                rows.push(row(vec![t(v), t(cnt.to_string()), Cell::Money(spent)]));
                for l in ls.iter().filter(|l| l.vendor == vid) {
                    rows.push(line_row(line_text(&Line { party: String::new(), ..clone_line(l) }, &l.acct), 3, 2, l.cents));
                }
            }
            rows.push(bold(vec![t("Total"), t(n.to_string()), Cell::Money(s)]));
            vec!["Vendor", "Transactions", "Spent"]
        }
        Kind::JobProfit if job.is_some() => {
            let mut st = b.db.prepare(&format!(
                "SELECT a.kind, a.name, SUM(l.cents) FROM entries e JOIN lines l ON l.entry_id = e.id
                 JOIN accounts a ON a.id = l.account_id
                 WHERE e.job_id = ?1 AND e.date BETWEEN ?2 AND ?3 AND a.kind IN ('income', 'expense')
                 GROUP BY a.id ORDER BY {KIND_ORDER}, a.number = '', a.number, a.name"
            ))?;
            let accts: Vec<(String, String, i64)> =
                st.query_map(params![job, from, to], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_>>()?;
            let ls = if detail {
                lines(b, "e.job_id = ?1 AND e.date BETWEEN ?2 AND ?3 AND a.kind IN ('income', 'expense')", params![job, from, to])?
            } else {
                vec![]
            };
            let mut profit = 0;
            for (k, label) in [("income", "Income"), ("expense", "Expenses")] {
                rows.push(bold(vec![t(label), t("")]));
                let mut total = 0;
                for (_, n, c) in accts.iter().filter(|l| l.0 == k) {
                    rows.push(row(vec![t(format!("    {n}")), Cell::Money(natural(k, *c))]));
                    for l in ls.iter().filter(|l| &l.acct == n) {
                        rows.push(line_row(line_text(l, &l.split), 2, 1, natural(k, l.cents)));
                    }
                    total += natural(k, *c);
                }
                profit += if k == "income" { total } else { -total };
                rows.push(bold(vec![t(format!("Total {}", label.to_lowercase())), Cell::Money(total)]));
            }
            rows.push(bold(vec![t("Job profit"), Cell::Money(profit)]));
            vec!["Account", "Amount"]
        }
        Kind::JobProfit => {
            let ls = if detail {
                lines(b, "e.job_id IS NOT NULL AND e.date BETWEEN ?1 AND ?2 AND a.kind IN ('income', 'expense')", params![from, to])?
            } else {
                vec![]
            };
            let (mut i, mut e) = (0, 0);
            for j in b.jobs_between(from, to)? {
                (i, e) = (i + j.income, e + j.expenses);
                rows.push(row(vec![
                    t(j.name), t(j.customer.unwrap_or_default()), Cell::Money(j.income), Cell::Money(j.expenses),
                    Cell::Money(j.income - j.expenses), t(margin(j.income, j.expenses)),
                ]));
                for l in ls.iter().filter(|l| l.job == Some(j.id)) {
                    let (col, v) = if l.kind == "income" { (2, -l.cents) } else { (3, l.cents) };
                    rows.push(line_row(line_text(l, &l.acct), 6, col, v));
                }
            }
            rows.push(bold(vec![t("Total"), t(""), Cell::Money(i), Cell::Money(e), Cell::Money(i - e), t(margin(i, e))]));
            vec!["Job", "Customer", "Income", "Expenses", "Profit", "Margin"]
        }
        Kind::SalesTax => {
            let q = |sql: &str| b.db.query_row(sql, params![from, to], |r| r.get::<_, i64>(0));
            let tax_acct = b.system_account("sales_tax")?;
            let sales = q("SELECT COALESCE(SUM(cents - tax_cents), 0) FROM invoices WHERE date BETWEEN ?1 AND ?2")?;
            let taxable = q(
                "SELECT COALESCE(SUM(ii.cents), 0) FROM invoice_items ii JOIN invoices i ON i.id = ii.invoice_id
                 WHERE ii.taxable = 1 AND i.tax_cents != 0 AND i.date BETWEEN ?1 AND ?2",
            )?;
            let charged = q("SELECT COALESCE(SUM(tax_cents), 0) FROM invoices WHERE date BETWEEN ?1 AND ?2")?;
            let paid: i64 = b.db.query_row(
                "SELECT COALESCE(SUM(l.cents), 0) FROM lines l JOIN entries e ON e.id = l.entry_id
                 WHERE l.account_id = ?1 AND l.cents > 0 AND e.date BETWEEN ?2 AND ?3",
                params![tax_acct, from, to],
                |r| r.get(0),
            )?;
            let owed: i64 = b.db.query_row(
                "SELECT COALESCE(-SUM(l.cents), 0) FROM lines l JOIN entries e ON e.id = l.entry_id WHERE l.account_id = ?1 AND e.date <= ?2",
                params![tax_acct, to],
                |r| r.get(0),
            )?;
            rows.push(row(vec![t("Invoiced sales (before tax)"), Cell::Money(sales)]));
            rows.push(row(vec![t("    Taxable sales"), Cell::Money(taxable)]));
            rows.push(row(vec![t("    Non-taxable sales"), Cell::Money(sales - taxable)]));
            rows.push(bold(vec![t("Sales tax charged"), Cell::Money(charged)]));
            if detail {
                let mut st = b.db.prepare(
                    "SELECT i.id, i.date, c.name, i.cents - i.tax_cents,
                       (SELECT COALESCE(SUM(ii.cents), 0) FROM invoice_items ii WHERE ii.invoice_id = i.id AND ii.taxable = 1), i.tax_cents
                     FROM invoices i JOIN customers c ON c.id = i.customer_id
                     WHERE i.date BETWEEN ?1 AND ?2 AND i.tax_cents != 0 ORDER BY i.date, i.id",
                )?;
                for r in st.query_map(params![from, to], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(4)?, r.get::<_, i64>(5)?))
                })? {
                    let (id, d, c, sale, taxable, tax) = r?;
                    let text = format!("        Invoice #{id}   {d} · {c} · {} taxable of {}", fmt_cents(taxable), fmt_cents(sale));
                    rows.push(line_row(text, 2, 1, tax));
                }
            }
            rows.push(row(vec![t("Sales tax paid to the state"), Cell::Money(paid)]));
            if detail {
                for l in lines(b, "l.account_id = ?1 AND l.cents > 0 AND e.date BETWEEN ?2 AND ?3", params![tax_acct, from, to])? {
                    rows.push(line_row(line_text(&l, &l.split), 2, 1, l.cents));
                }
            }
            rows.push(bold(vec![t(format!("Still owed as of {to}")), Cell::Money(owed)]));
            vec!["Item", "Amount"]
        }
        Kind::ScheduleC => {
            let pl = b.profit_and_loss(from, to)?;
            let mut lines: Vec<(&str, &str, i64)> = vec![];
            // (line, label, account, amount): the accounts behind each line, for detail.
            let mut src: Vec<(&str, &str, String, i64)> = vec![];
            let mut add = |line: &'static str, label: &'static str, from: String, c: i64| {
                match lines.iter_mut().find(|l| l.0 == line && l.1 == label) {
                    Some(l) => l.2 += c,
                    None => lines.push((line, label, c)),
                }
                src.push((line, label, from, c));
            };
            for (k, n, c) in &pl.rows {
                let (line, label) = schedule_c_line(k, n);
                if line == "2" {
                    add(line, label, n.clone(), -c); // refunds reduce income: shown as a positive return, then subtracted
                } else if line == "24b" {
                    add(line, label, n.clone(), c / 2); // only half of business meals is deductible
                    add("", "Meals not deductible (the other 50%)", n.clone(), c - c / 2);
                } else {
                    add(line, label, n.clone(), *c);
                }
            }
            let miles = b.mileage_deduction(from, to)?;
            if miles != 0 {
                add("9", "Car and truck: standard mileage (from the mileage log)", "Mileage log".into(), miles);
            }
            lines.sort_by_key(|l| (SCHEDULE_C_ORDER.iter().position(|o| *o == l.0).unwrap_or(99), l.1));
            let get = |l: &str| lines.iter().filter(|x| x.0 == l).map(|x| x.2).sum::<i64>();
            let gross = get("1") - get("2") - get("4") + get("6");
            let expenses: i64 = lines.iter().filter(|l| !["1", "2", "4", "6", "30", ""].contains(&l.0)).map(|l| l.2).sum();
            for (line, label, c) in &lines {
                rows.push(row(vec![t(if line.is_empty() { "—".to_string() } else { format!("Line {line}") }), t(*label), Cell::Money(*c)]));
                if detail {
                    for (_, _, acct, c) in src.iter().filter(|s| s.0 == *line && s.1 == *label) {
                        rows.push(row(vec![t(""), t(format!("        from {acct}")), Cell::Money(*c)]));
                    }
                }
            }
            rows.push(bold(vec![t("Line 7"), t("Gross income"), Cell::Money(gross)]));
            rows.push(bold(vec![t("Line 28"), t("Total expenses before home office"), Cell::Money(expenses)]));
            rows.push(bold(vec![t("Line 29"), t("Tentative profit"), Cell::Money(gross - expenses)]));
            rows.push(bold(vec![t("Line 31"), t("Net profit (or loss)"), Cell::Money(gross - expenses - get("30"))]));
            rows.push(row(vec![t(""), t("A guide for you or your tax preparer, not a filed return. If you use the mileage rate, don't also deduct actual car costs."), t("")]));
            vec!["Schedule C", "Description", "Amount"]
        }
        Kind::Vendors1099 => {
            let mut st = b.db.prepare(
                "SELECT v.name, v.is_1099, COALESCE(SUM(l.cents), 0), v.id FROM vendors v
                 JOIN entries e ON e.vendor_id = v.id AND e.date BETWEEN ?1 AND ?2
                 JOIN lines l ON l.entry_id = e.id JOIN accounts a ON a.id = l.account_id AND a.kind = 'expense'
                 GROUP BY v.id HAVING v.is_1099 OR SUM(l.cents) >= ?3 ORDER BY SUM(l.cents) DESC",
            )?;
            // The filing threshold was $600 through 2025 and $2,000 for payments from 2026 on.
            let year: i32 = to.get(..4).and_then(|y| y.parse().ok()).unwrap_or(2026);
            let limit: i64 = if year >= 2026 { 200_000 } else { 60_000 };
            let ls = if detail { lines(b, "a.kind = 'expense' AND e.vendor_id IS NOT NULL AND e.date BETWEEN ?1 AND ?2", params![from, to])? } else { vec![] };
            for r in st.query_map(params![from, to, limit], |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?)))? {
                let (v, flagged, paid, vid) = r?;
                let action = match (flagged, paid >= limit) {
                    (true, true) => "File a 1099-NEC".to_string(),
                    (true, false) => format!("Under {}: no form needed", fmt_cents(limit)),
                    (false, _) => format!("Over {}: mark as 1099 if they're a contractor", fmt_cents(limit)),
                };
                rows.push(row(vec![t(v), t(if flagged { "Yes" } else { "" }), Cell::Money(paid), t(action)]));
                // "paid from" matters: card payments are reported by the card company.
                for l in ls.iter().filter(|l| l.vendor == Some(vid)) {
                    let text = line_text(&Line { party: String::new(), ..clone_line(l) }, &format!("paid from {}", l.split));
                    rows.push(line_row(text, 4, 2, l.cents));
                }
            }
            rows.push(row(vec![t(""), t(""), t(""), t("Payments by credit card or PayPal are reported by the processor, not you.")]));
            vec!["Vendor", "1099", "Paid", "What to do"]
        }
        Kind::EstimatedTax => {
            let year: i32 = to.get(..4).and_then(|y| y.parse().ok()).unwrap_or(2026);
            let pct = b.settings()?.income_tax_pct;
            let periods = [
                ("Jan 1 – Mar 31", "01-01", "03-31", format!("Apr 15, {year}")),
                ("Apr 1 – May 31", "04-01", "05-31", format!("Jun 15, {year}")),
                ("Jun 1 – Aug 31", "06-01", "08-31", format!("Sep 15, {year}")),
                ("Sep 1 – Dec 31", "09-01", "12-31", format!("Jan 15, {}", year + 1)),
            ];
            let mut tot = [0i64; 4];
            for (label, a, z, due) in periods {
                let (pf, pt) = (format!("{year}-{a}"), format!("{year}-{z}"));
                let pl = b.profit_and_loss(&pf, &pt)?;
                // Same profit as Schedule C: only half of meals counts, and mileage is deducted.
                let meals: i64 = pl.rows.iter().filter(|r| r.0 == "expense" && r.1 == "Meals").map(|r| r.2).sum();
                let profit = net_income(&pl) + (meals - meals / 2) - b.mileage_deduction(&pf, &pt)?;
                let base = profit.max(0) as f64;
                let se = (base * 0.9235 * 0.153).round() as i64;
                let inc = (base * pct as f64 / 100.0).round() as i64;
                for (i, v) in [profit, se, inc, se + inc].into_iter().enumerate() {
                    tot[i] += v;
                }
                rows.push(row(vec![t(label), t(due), Cell::Money(profit), Cell::Money(se), Cell::Money(inc), Cell::Money(se + inc)]));
                if detail {
                    let sum = |k: &str| pl.rows.iter().filter(|r| r.0 == k).map(|r| r.2).sum::<i64>();
                    for (what, v) in [
                        ("Income", sum("income")),
                        ("Expenses", -sum("expense")),
                        ("Meals not deductible (added back)", meals - meals / 2),
                        ("Mileage deduction", -b.mileage_deduction(&pf, &pt)?),
                    ] {
                        if v != 0 {
                            rows.push(line_row(format!("        {what}"), 6, 2, v));
                        }
                    }
                }
            }
            rows.push(bold(vec![t(format!("{year} total")), t(""), Cell::Money(tot[0]), Cell::Money(tot[1]), Cell::Money(tot[2]), Cell::Money(tot[3])]));
            rows.push(row(vec![
                t(format!("Rough guide: 15.3% self-employment tax on 92.35% of profit, plus {pct}% income tax (change the rate on Business).")),
                t(""), t(""), t(""), t(""), t(""),
            ]));
            vec!["Period", "Due", "Profit", "SE tax", "Income tax", "Set aside"]
        }
        Kind::Mileage => {
            let rate = b.settings()?.mileage_rate;
            let mut miles = 0;
            for trip in b.trips(from, to)?.into_iter().rev() {
                let d = (trip.tenths * rate + 5) / 10;
                miles += trip.tenths;
                rows.push(row(vec![t(trip.date), t(trip.purpose), t(trip.job.unwrap_or_default()), t(format!("{:.1}", trip.tenths as f64 / 10.0)), Cell::Money(d)]));
            }
            let ded = b.mileage_deduction(from, to)?; // rounded once on the total, matching Schedule C
            rows.push(bold(vec![t("Total"), t(format!("at {} a mile", fmt_cents(rate))), t(""), t(format!("{:.1}", miles as f64 / 10.0)), Cell::Money(ded)]));
            if detail {
                let mut by: Vec<(String, i64)> = vec![];
                for trip in b.trips(from, to)? {
                    let j = trip.job.unwrap_or_else(|| "(no job)".into());
                    match by.iter_mut().find(|x| x.0 == j) {
                        Some(x) => x.1 += trip.tenths,
                        None => by.push((j, trip.tenths)),
                    }
                }
                by.sort();
                rows.push(bold(vec![t("By job"), t(""), t(""), t(""), t("")]));
                for (j, tenths) in by {
                    rows.push(row(vec![t(""), t(""), t(format!("    {j}")), t(format!("{:.1}", tenths as f64 / 10.0)), Cell::Money((tenths * rate + 5) / 10)]));
                }
            }
            vec!["Date", "Purpose", "Job", "Miles", "Deduction"]
        }
        Kind::Assets => {
            let mut tot = [0i64; 3];
            let mut cat = String::new();
            for a in b.assets()?.into_iter().filter(|a| a.purchased.as_str() <= to) {
                if a.category != cat {
                    cat = a.category.clone();
                    let head = if a.intangible { format!("{cat} (intangible: amortized)") } else { cat.clone() };
                    rows.push(bold(vec![t(head), t(""), t(""), t(""), t(""), t("")]));
                }
                let dep = a.depreciation_through(to);
                for (i, v) in [a.cost, dep, a.cost - dep].into_iter().enumerate() {
                    tot[i] += v;
                }
                let life = if a.life_years == 0 { "not depreciated".to_string() } else { format!("{} yr", a.life_years) };
                let name = if a.disposed.is_empty() { format!("    {}", a.name) } else { format!("    {} (disposed {})", a.name, a.disposed) };
                rows.push(row(vec![t(name), t(a.purchased.clone()), t(life), Cell::Money(a.cost), Cell::Money(dep), Cell::Money(a.cost - dep)]));
                if detail {
                    let info: Vec<String> = [("Serial", &a.serial), ("Notes", &a.notes)]
                        .iter()
                        .filter(|x| !x.1.is_empty())
                        .map(|x| format!("{}: {}", x.0, x.1))
                        .collect();
                    if !info.is_empty() {
                        rows.push(row(vec![t(format!("        {}", info.join(" · "))), t(""), t(""), t(""), t(""), t("")]));
                    }
                    let year = |d: &str| d.get(..4).and_then(|y| y.parse::<i32>().ok()).unwrap_or(0);
                    let mut before = 0;
                    for y in year(&a.purchased)..=year(to) {
                        let end = format!("{y}-12-31");
                        let through = a.depreciation_through(if end.as_str() < to { &end } else { to });
                        if through != before {
                            rows.push(line_row(format!("        {y} depreciation"), 6, 4, through - before));
                            if let Some(c) = rows.last_mut() {
                                c.cells[5] = Cell::Money(a.cost - through);
                            }
                        }
                        before = through;
                    }
                }
            }
            rows.push(bold(vec![t("Total"), t(""), t(""), Cell::Money(tot[0]), Cell::Money(tot[1]), Cell::Money(tot[2])]));
            rows.push(row(vec![t("Straight-line book depreciation (amortization for intangibles) by month. Tax rules (Section 179, bonus, 15-year Section 197 intangibles) can differ; ask your preparer."), t(""), t(""), t(""), t(""), t("")]));
            vec!["Asset", "Bought", "Life", "Cost", "Depreciated / amortized", "Book value"]
        }
    };
    Ok(Table { title, columns, rows })
}

/// Schedule C (Form 1040) line order used to sort the summary.
const SCHEDULE_C_ORDER: [&str; 24] =
    ["1", "2", "4", "6", "8", "9", "10", "11", "13", "14", "15", "16b", "17", "18", "20b", "21", "22", "23", "24a", "24b", "25", "26", "27a", "30"];

/// Maps an account to its Schedule C line by the standard chart's names; anything else is "other".
fn schedule_c_line(kind: &str, name: &str) -> (&'static str, &'static str) {
    if kind == "income" {
        return match name {
            "Refunds & Discounts Given" => ("2", "Returns and allowances"),
            "Interest Income" | "Other Income" | "Uncategorized Income" => ("6", "Other income"),
            _ => ("1", "Gross receipts or sales"),
        };
    }
    match name {
        "Cost of Goods Sold" | "Job Materials" | "Subcontractors" | "Equipment Rental for Jobs" => ("4", "Cost of goods sold (materials, subcontractors)"),
        "Advertising" => ("8", "Advertising"),
        "Car & Truck" => ("9", "Car and truck expenses"),
        "Commissions & Fees" => ("10", "Commissions and fees"),
        "Contract Labor" => ("11", "Contract labor"),
        "Depreciation" => ("13", "Depreciation"),
        "Employee Benefits" => ("14", "Employee benefit programs"),
        "Insurance" => ("15", "Insurance (other than health)"),
        "Interest Expense" => ("16b", "Interest (other)"),
        "Legal & Professional Fees" => ("17", "Legal and professional services"),
        "Office Supplies" => ("18", "Office expense"),
        "Rent or Lease" => ("20b", "Rent or lease (other business property)"),
        "Repairs & Maintenance" => ("21", "Repairs and maintenance"),
        "Tools & Small Equipment" => ("22", "Supplies"),
        "Taxes & Licenses" | "Licenses & Permits" | "Payroll Taxes" => ("23", "Taxes and licenses"),
        "Travel" => ("24a", "Travel"),
        "Meals" => ("24b", "Deductible meals (50%)"),
        "Utilities" | "Phone & Internet" => ("25", "Utilities"),
        "Wages" => ("26", "Wages"),
        "Home Office" => ("30", "Business use of home"),
        "Health Insurance" => ("", "Health insurance: goes on Schedule 1, not Schedule C"),
        _ => ("27a", "Other expenses"),
    }
}

fn margin(income: i64, expenses: i64) -> String {
    if income == 0 { String::new() } else { format!("{:.0}%", (income - expenses) as f64 * 100.0 / income as f64) }
}

pub fn to_csv(table: &Table) -> std::result::Result<String, String> {
    let mut w = csv::Writer::from_writer(vec![]);
    w.write_record(&table.columns).map_err(|e| e.to_string())?;
    for r in &table.rows {
        // Plain numbers so spreadsheets can add them up.
        let cells = r.cells.iter().map(|c| match c {
            // A leading ' stops spreadsheets running text from bank descriptions as a formula.
            Cell::Text(s) if s.trim().starts_with(['=', '+', '-', '@']) => format!("'{}", s.trim()),
            Cell::Text(s) => s.trim().to_string(),
            Cell::Money(m) => format!("{}{}.{:02}", if *m < 0 { "-" } else { "" }, m.abs() / 100, m.abs() % 100),
        });
        w.write_record(cells).map_err(|e| e.to_string())?;
    }
    String::from_utf8(w.into_inner().map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// A report as PDF blocks: title, business name, then the table (money columns right-aligned).
pub fn to_blocks(table: &Table, business: &str) -> Vec<Block> {
    let money: Vec<bool> = (0..table.columns.len()).map(|i| table.rows.iter().any(|r| matches!(r.cells.get(i), Some(Cell::Money(_))))).collect();
    let cols = table
        .columns
        .iter()
        .enumerate()
        .map(|(i, c)| (c.to_string(), if money[i] { Align::Right } else { Align::Left }))
        .collect();
    let rows = table
        .rows
        .iter()
        .map(|r| PdfRow {
            cells: r.cells.iter().map(|c| match c {
                Cell::Text(s) => s.clone(),
                Cell::Money(m) => fmt_cents(*m),
            }).collect(),
            style: if r.strong { Style::Strong } else { Style::Normal },
        })
        .collect();
    vec![Block::Title(table.title.clone()), Block::note(business), Block::Space(4.0), Block::Table { cols, rows }]
}

/// One or more reports in one PDF, each starting on its own page.
pub fn to_pdf(tables: &[Table], business: &str) -> Result<Vec<u8>, String> {
    let mut blocks = vec![];
    for t in tables {
        blocks.push(Block::PageBreak);
        blocks.extend(to_blocks(t, business));
    }
    crate::pdf::render(tables.first().map_or("Reports", |t| t.title.as_str()), &blocks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::books::tests::doc;

    #[test]
    fn every_report_runs_and_balances() {
        let b = Books::open(":memory:").unwrap();
        let acct = |n: &str| b.accounts().unwrap().into_iter().find(|a| a.name == n).unwrap().id;
        let cust = b.add_customer("Acme", "").unwrap();
        let vend = b.add_vendor("Lumber Yard", "").unwrap();
        b.add_job("Deck", Some(cust)).unwrap();
        let job = b.jobs().unwrap()[0].id;
        b.post_simple("2026-01-01", "Seed", acct("Checking"), acct("Owner's Contributions"), 100_000).unwrap();
        let old = b.create_invoice(&doc(cust, "2026-01-05", &[("Old work".into(), 20_000)], None)).unwrap();
        b.create_invoice(&doc(cust, "2026-03-01", &[("Deck".into(), 50_000)], Some(job))).unwrap();
        let wood = b.post_simple("2026-03-02", "Boards", acct("Checking"), acct("Job Materials"), -12_000).unwrap();
        b.set_tags(wood, Some(job), Some(vend)).unwrap();
        b.mark_paid(old, "2026-04-15", acct("Checking")).unwrap();

        for (k, _) in ALL {
            for detail in [false, true] {
                let tbl = run(&b, k, "2026-01-01", "2026-03-31", None, None, detail).unwrap();
                assert!(to_csv(&tbl).is_ok() && crate::pdf::plain_text(&to_blocks(&tbl, "Me")).contains(&tbl.title), "{k:?}");
            }
        }
        // Detail rows under each P&L account add up to that account's total.
        let money0 = |c: &Cell| if let Cell::Money(m) = c { *m } else { 0 };
        let pl = run(&b, Kind::ProfitLoss, "2026-01-01", "2026-03-31", None, None, true).unwrap();
        let i = pl.rows.iter().position(|r| matches!(&r.cells[0], Cell::Text(s) if s.trim() == "Job Materials")).unwrap();
        let under: i64 = pl.rows[i + 1..].iter().take_while(|r| matches!(&r.cells[0], Cell::Text(s) if s.starts_with("        "))).map(|r| money0(&r.cells[1])).sum();
        assert_eq!(under, money0(&pl.rows[i].cells[1]));
        assert!(pl.rows.iter().any(|r| matches!(&r.cells[0], Cell::Text(s) if s.contains("Boards · Lumber Yard · Checking"))));
        let ar = run(&b, Kind::ArAging, "", "2026-03-31", None, None, true).unwrap();
        assert!(ar.rows.iter().any(|r| matches!(&r.cells[0], Cell::Text(s) if s.contains("Invoice #"))));
        let money = |c: &Cell| if let Cell::Money(m) = c { *m } else { 0 };
        let tb = run(&b, Kind::TrialBalance, "", "2026-03-31", None, None, false).unwrap();
        let total = tb.rows.last().unwrap();
        assert_eq!(money(&total.cells[2]), money(&total.cells[3]), "debits equal credits");

        // On 3/31 the January invoice was still unpaid (paid 4/15) and 85 days old.
        let ar = run(&b, Kind::ArAging, "", "2026-03-31", None, None, false).unwrap();
        let acme = &ar.rows[0].cells;
        assert_eq!((money(&acme[1]), money(&acme[3]), money(&acme[5])), (50_000, 20_000, 70_000));

        let jp = run(&b, Kind::JobProfit, "2026-01-01", "2026-12-31", None, Some(job), false).unwrap();
        assert_eq!(money(&jp.rows.last().unwrap().cells[1]), 38_000);
        let ev = run(&b, Kind::ExpensesByVendor, "2026-01-01", "2026-12-31", None, None, false).unwrap();
        assert!(matches!(&ev.rows[0].cells[0], Cell::Text(v) if v == "Lumber Yard"));
        let sc = run(&b, Kind::ScheduleC, "2026-01-01", "2026-12-31", None, None, false).unwrap();
        let net = sc.rows.iter().find(|r| matches!(&r.cells[0], Cell::Text(s) if s == "Line 31")).unwrap();
        assert_eq!(money(&net.cells[2]), net_income(&b.profit_and_loss("2026-01-01", "2026-12-31").unwrap()));
        // A refund lowers Schedule C income instead of raising it.
        b.post_simple("2026-03-05", "Refund", acct("Checking"), acct("Refunds & Discounts Given"), -5_000).unwrap();
        let sc = run(&b, Kind::ScheduleC, "2026-01-01", "2026-12-31", None, None, false).unwrap();
        let net = sc.rows.iter().find(|r| matches!(&r.cells[0], Cell::Text(s) if s == "Line 31")).unwrap();
        assert_eq!(money(&net.cells[2]), net_income(&b.profit_and_loss("2026-01-01", "2026-12-31").unwrap()));
        // 2026 uses the $2,000 1099 threshold: $120 to Lumber Yard isn't listed unless flagged.
        let v = run(&b, Kind::Vendors1099, "2026-01-01", "2026-12-31", None, None, false).unwrap();
        assert_eq!(v.rows.len(), 1, "only the note row");
        let gl = run(&b, Kind::GeneralLedger, "2026-02-01", "2026-12-31", Some(acct("Checking")), None, false).unwrap();
        assert_eq!(money(&gl.rows[1].cells[3]), 100_000, "opening balance carries in");
        assert_eq!(money(&gl.rows.last().unwrap().cells[3]), 100_000 - 12_000 + 20_000 - 5_000);
    }

    /// Writes sample PDFs to the temp folder to look at by eye. Run: cargo test -- --ignored sample_pdfs
    #[test]
    #[ignore]
    fn sample_pdfs() {
        let b = Books::open(":memory:").unwrap();
        let acct = |n: &str| b.accounts().unwrap().into_iter().find(|a| a.name == n).unwrap().id;
        b.save_business(&crate::books::Business { name: "Riverside Woodworks".into(), street: "12 Mill Road".into(), city: "Portland".into(), state: "OR".into(), zip: "97201".into(), phone: "503-555-0100".into(), email: "hello@riverside.test".into(), ..Default::default() }).unwrap();
        let mut s = b.settings().unwrap();
        s.pay_link = "https://example.com/pay".into();
        b.save_prefs(&s).unwrap();
        let cust = b.add_customer("Acme Coffee", "orders@acme.test").unwrap();
        let items: Vec<crate::books::Item> = vec![("Walnut counter, 8 ft, oil finish".into(), 240_000), ("Install and haul-away".into(), 35_000), ("Delivery".into(), 7_500)];
        let inv = b.create_invoice(&crate::books::DocInput { tax_bp: 825, due_date: "2026-04-01".into(), customer_po: "PO-7781".into(), ..doc(cust, "2026-03-01", &items, None) }).unwrap();
        b.post_simple("2026-01-01", "Seed", acct("Checking"), acct("Owner's Contributions"), 100_000).unwrap();
        b.post_simple("2026-03-02", "Boards", acct("Checking"), acct("Job Materials"), -12_000).unwrap();
        let dir = std::env::temp_dir();
        std::fs::write(dir.join("ac-sample-invoice.pdf"), b.document_pdf(crate::books::Doc::Invoice, inv).unwrap()).unwrap();
        let tables: Vec<Table> = [Kind::ProfitLoss, Kind::GeneralLedger].iter().map(|k| run(&b, *k, "2026-01-01", "2026-12-31", None, None, true).unwrap()).collect();
        std::fs::write(dir.join("ac-sample-reports.pdf"), to_pdf(&tables, "Riverside Woodworks").unwrap()).unwrap();
        println!("{}", dir.display());
    }
}
