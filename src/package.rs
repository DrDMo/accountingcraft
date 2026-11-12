//! The accountant package: a folder with every report (one PDF plus CSVs), the full journal,
//! QuickBooks Online import files, and a read-me that says how finished the period is.

use crate::books::{fmt_cents, Books};
use crate::reports::{self, Kind};
use rusqlite::params;
use std::path::{Path, PathBuf};

/// Reports in the package, and whether each uses its detail version.
const REPORTS: [(Kind, bool); 11] = [
    (Kind::ProfitLoss, false),
    (Kind::ProfitLoss, true),
    (Kind::BalanceSheet, false),
    (Kind::TrialBalance, false),
    (Kind::GeneralLedger, true),
    (Kind::ArAging, true),
    (Kind::SalesTax, true),
    (Kind::ScheduleC, true),
    (Kind::Vendors1099, true),
    (Kind::Assets, true),
    (Kind::Mileage, false),
];

type R<T> = Result<T, String>;
fn e(x: impl std::fmt::Display) -> String {
    x.to_string()
}

/// Things that make the period less than finished, in plain words. Empty means ready.
pub fn issues(b: &Books, from: &str, to: &str) -> R<Vec<String>> {
    let mut out = vec![];
    let uncat: i64 = b
        .db
        .query_row(
            "SELECT COUNT(DISTINCT e.id) FROM entries e JOIN lines l ON l.entry_id = e.id JOIN accounts a ON a.id = l.account_id
             WHERE a.system IN ('uncat_in', 'uncat_out') AND e.date BETWEEN ?1 AND ?2",
            params![from, to],
            |r| r.get(0),
        )
        .map_err(e)?;
    if uncat > 0 {
        out.push(format!("{uncat} transaction(s) are still uncategorized."));
    }
    for (id, name) in bank_accounts(b)? {
        match b.last_reconciled(id).map_err(e)? {
            Some((d, _)) if d.as_str() >= to => {}
            Some((d, _)) => out.push(format!("{name} is reconciled only through {d}.")),
            None => out.push(format!("{name} has never been reconciled.")),
        }
    }
    let lock = b.settings().map_err(e)?.lock_date;
    if lock.as_str() < to {
        out.push(if lock.is_empty() { "The books aren't locked, so this period can still change.".into() } else { format!("The books are locked only through {lock}.") });
    }
    Ok(out)
}

/// Bank and card accounts that have activity: the ones a CPA expects reconciled.
fn bank_accounts(b: &Books) -> R<Vec<(i64, String)>> {
    let mut st = b
        .db
        .prepare(
            "SELECT a.id, a.name FROM accounts a WHERE a.system IS NULL AND EXISTS (SELECT 1 FROM lines l WHERE l.account_id = a.id)
             AND ((a.kind = 'asset' AND a.number < '1100' AND a.number != '') OR (a.kind = 'liability' AND a.name LIKE '%card%'))
             ORDER BY a.number",
        )
        .map_err(e)?;
    let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(e)?;
    v.collect::<rusqlite::Result<_>>().map_err(e)
}

/// Writes the package into a new folder inside `parent`; returns the folder.
/// A missing PDF (no Edge) isn't fatal: the HTML is kept and the read-me says so.
pub fn write(b: &Books, parent: &Path, from: &str, to: &str) -> R<PathBuf> {
    let s = b.settings().map_err(e)?;
    let dir = parent.join(format!("Accountant package {from} to {to}"));
    let csv_dir = dir.join("Reports (CSV)");
    let qbo = dir.join("QuickBooks Online import");
    for d in [&csv_dir, &qbo] {
        std::fs::create_dir_all(d).map_err(e)?;
    }

    // Reports: one CSV each, and all of them in one PDF with page breaks.
    let mut tables = vec![];
    for (k, detail) in REPORTS {
        let t = reports::run(b, k, from, to, None, None, detail).map_err(e)?;
        let file = t.title.replace([',', '/', ':', '(', ')', '\\'], "").replace("  ", " ");
        std::fs::write(csv_dir.join(format!("{file}.csv")), reports::to_csv(&t)?).map_err(e)?;
        tables.push(t);
    }
    let names: Vec<String> = tables.iter().map(|t| t.title.clone()).collect();
    std::fs::write(dir.join("Reports.pdf"), reports::to_pdf(&tables, &s.name)?).map_err(e)?;
    let pdf_note = "Reports.pdf: every report below, each starting on a new page.".to_string();

    // The full journal for the period, every line of every entry.
    let lines = journal(b, from, to)?;
    let mut w = csv::Writer::from_writer(vec![]);
    w.write_record(["Entry", "Date", "Account number", "Account", "Debit", "Credit", "Description", "Name", "Job"]).map_err(e)?;
    for l in &lines {
        let (d, c) = dr_cr(l.cents);
        w.write_record([&l.entry.to_string(), &l.date, &l.number, &l.account, &d, &c, &l.memo, &l.name, &l.job]).map_err(e)?;
    }
    std::fs::write(dir.join("Journal (every line).csv"), w.into_inner().map_err(e)?).map_err(e)?;

    // QuickBooks Online: chart of accounts, customers, vendors, journal entries (US dates).
    let mut w = csv::Writer::from_writer(vec![]);
    w.write_record(["Account Number", "Account Name", "Type", "Detail Type", "Description"]).map_err(e)?;
    for a in b.accounts().map_err(e)? {
        let (ty, detail) = if a.intangible { ("Other Assets", "Intangible Assets") } else { qbo_type(&a.kind, &a.number, &a.name, a.system.as_deref()) };
        w.write_record([a.number.as_str(), &a.name, ty, detail, &a.description]).map_err(e)?;
    }
    std::fs::write(qbo.join("1 Chart of accounts.csv"), w.into_inner().map_err(e)?).map_err(e)?;
    for (file, list, is_vendor) in [("2 Customers.csv", b.customers().map_err(e)?, false), ("3 Vendors.csv", b.vendors().map_err(e)?, true)] {
        let mut w = csv::Writer::from_writer(vec![]);
        if is_vendor {
            w.write_record(["Name", "Email", "Track payments for 1099"]).map_err(e)?;
        } else {
            w.write_record(["Name", "Email", "Billing Address"]).map_err(e)?;
        }
        for p in list {
            let third = if is_vendor { if p.is_1099 { "Yes" } else { "No" }.to_string() } else { p.address.replace('\n', ", ") };
            w.write_record([&p.name, &p.email, &third]).map_err(e)?;
        }
        std::fs::write(qbo.join(file), w.into_inner().map_err(e)?).map_err(e)?;
    }
    let mut w = csv::Writer::from_writer(vec![]);
    w.write_record(["Journal No", "Journal Date", "Account Name", "Debits", "Credits", "Description", "Name"]).map_err(e)?;
    for l in &lines {
        let (d, c) = dr_cr(l.cents);
        w.write_record([&format!("AC{}", l.entry), &us_date(&l.date), &l.account, &d, &c, &l.memo, &l.name]).map_err(e)?;
    }
    std::fs::write(qbo.join("4 Journal entries.csv"), w.into_inner().map_err(e)?).map_err(e)?;

    // Read-me: who, what period, how finished.
    let mut r = format!("{}\n", s.name);
    for l in s.address.lines() {
        r += &format!("{l}\n");
    }
    r += &format!("\nAccountant package for {from} to {to}, made {}.\n\n", chrono::Local::now().format("%Y-%m-%d %H:%M"));
    r += "STATUS\n";
    let iss = issues(b, from, to)?;
    if iss.is_empty() {
        r += "  Ready: everything categorized, bank and card accounts reconciled, period locked.\n";
    }
    for i in &iss {
        r += &format!("  ! {i}\n");
    }
    r += "\nRECONCILED THROUGH\n";
    for (id, name) in bank_accounts(b)? {
        let d = b.last_reconciled(id).map_err(e)?.map_or("never".to_string(), |(d, c)| format!("{d} (statement balance {})", fmt_cents(c)));
        r += &format!("  {name}: {d}\n");
    }
    r += &format!("\nWHAT'S HERE\n  {pdf_note}\n");
    for n in &names {
        r += &format!("    - {n}\n");
    }
    r += "  Reports (CSV): the same reports as spreadsheets.\n";
    r += &format!("  Journal (every line).csv: all {} lines of every entry in the period, debits and credits.\n", lines.len());
    r += "  QuickBooks Online import: import in order 1-4 (Settings > Import data). Journal dates are MM/DD/YYYY.\n";
    r += "    Account types are a best guess from the account numbers; check them in step 1.\n";
    r += "    Balances before the start date aren't included; post an opening entry from the Trial Balance if needed.\n";
    std::fs::write(dir.join("Read me first.txt"), r.replace('\n', "\r\n")).map_err(e)?;
    Ok(dir)
}

struct JLine {
    entry: i64,
    date: String,
    number: String,
    account: String,
    cents: i64,
    memo: String,
    name: String,
    job: String,
}

fn journal(b: &Books, from: &str, to: &str) -> R<Vec<JLine>> {
    let mut st = b
        .db
        .prepare(
            "SELECT e.id, e.date, a.number, a.name, l.cents, e.memo,
               COALESCE(v.name,
                 (SELECT c.name FROM invoices i JOIN customers c ON c.id = i.customer_id WHERE i.entry_id = e.id),
                 (SELECT c.name FROM payments p JOIN invoices i ON i.id = p.invoice_id JOIN customers c ON c.id = i.customer_id WHERE p.entry_id = e.id),
                 ''),
               COALESCE(j.name, '')
             FROM lines l JOIN entries e ON e.id = l.entry_id JOIN accounts a ON a.id = l.account_id
             LEFT JOIN vendors v ON v.id = e.vendor_id LEFT JOIN jobs j ON j.id = e.job_id
             WHERE e.date BETWEEN ?1 AND ?2 ORDER BY e.date, e.id, l.id",
        )
        .map_err(e)?;
    let v = st
        .query_map(params![from, to], |r| {
            Ok(JLine { entry: r.get(0)?, date: r.get(1)?, number: r.get(2)?, account: r.get(3)?, cents: r.get(4)?, memo: r.get(5)?, name: r.get(6)?, job: r.get(7)? })
        })
        .map_err(e)?;
    v.collect::<rusqlite::Result<_>>().map_err(e)
}

/// Plain numbers (no $ or commas) so spreadsheets and QuickBooks read them as amounts.
fn dr_cr(cents: i64) -> (String, String) {
    let n = |c: i64| format!("{}.{:02}", c / 100, c % 100);
    if cents >= 0 { (n(cents), String::new()) } else { (String::new(), n(-cents)) }
}

fn us_date(iso: &str) -> String {
    match (iso.get(0..4), iso.get(5..7), iso.get(8..10)) {
        (Some(y), Some(m), Some(d)) => format!("{m}/{d}/{y}"),
        _ => iso.to_string(),
    }
}

/// QuickBooks Online account type and detail type, guessed from the standard chart's numbering.
fn qbo_type(kind: &str, number: &str, name: &str, system: Option<&str>) -> (&'static str, &'static str) {
    let n: u32 = number.parse().unwrap_or(0);
    match (kind, system) {
        ("asset", Some("ar")) => ("Accounts receivable (A/R)", "Accounts Receivable (A/R)"),
        ("asset", Some("accum_dep")) => ("Fixed Assets", "Accumulated Depreciation"),
        ("asset", Some("accum_amort")) => ("Other Assets", "Accumulated Amortization of Other Assets"),
        ("asset", _) if (1000..1100).contains(&n) => ("Bank", if name.contains("Saving") { "Savings" } else { "Checking" }),
        ("asset", _) if (1500..1600).contains(&n) => ("Fixed Assets", "Machinery & Equipment"),
        ("asset", _) => ("Other Current Assets", "Other Current Assets"),
        ("liability", Some("ap")) => ("Accounts payable (A/P)", "Accounts Payable (A/P)"),
        ("liability", _) if name.to_lowercase().contains("card") => ("Credit Card", "Credit Card"),
        ("liability", _) if n >= 2500 => ("Long Term Liabilities", "Notes Payable"),
        ("liability", _) => ("Other Current Liabilities", "Other Current Liabilities"),
        ("equity", _) => ("Equity", "Owner's Equity"),
        ("income", _) => ("Income", "Service/Fee Income"),
        ("expense", _) if (5000..6000).contains(&n) => ("Cost of Goods Sold", "Supplies & Materials - COGS"),
        _ => ("Expenses", "Other Miscellaneous Service Cost"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_files_and_journal_balance() {
        let b = Books::open(":memory:").unwrap();
        let acct = |n: &str| b.accounts().unwrap().into_iter().find(|a| a.name == n).unwrap().id;
        b.post_simple("2026-01-02", "Seed", acct("Checking"), acct("Owner's Contributions"), 50_000).unwrap();
        b.post_simple("2026-01-03", "Paint", acct("Checking"), acct("Job Materials"), -1_234).unwrap();
        assert!(issues(&b, "2026-01-01", "2026-03-31").unwrap().iter().any(|i| i.contains("Checking has never been reconciled")));
        let lines = journal(&b, "2026-01-01", "2026-03-31").unwrap();
        assert_eq!(lines.iter().map(|l| l.cents).sum::<i64>(), 0, "debits equal credits");
        assert_eq!(dr_cr(-1_234), (String::new(), "12.34".to_string()));
        assert_eq!(us_date("2026-01-03"), "01/03/2026");
        let tmp = std::env::temp_dir().join(format!("ac-pkg-test-{}", std::process::id()));
        // PDF needs Edge; the package still succeeds without it.
        let dir = write(&b, &tmp, "2026-01-01", "2026-03-31").unwrap();
        for f in ["Read me first.txt", "Journal (every line).csv", "QuickBooks Online import/4 Journal entries.csv", "QuickBooks Online import/1 Chart of accounts.csv"] {
            assert!(dir.join(f).exists(), "{f}");
        }
        let je = std::fs::read_to_string(dir.join("QuickBooks Online import/4 Journal entries.csv")).unwrap();
        assert!(je.contains("01/03/2026,Job Materials,12.34,,Paint"));
        let _ = std::fs::remove_dir_all(tmp);
    }
}
