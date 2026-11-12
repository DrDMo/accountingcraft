//! Purchase orders to vendors: send one, mark it received (which books the cost, paid now or owed
//! in Accounts Payable), then pay the bill. Costs land on the PO's job, so job profit includes them.

use crate::books::{fmt_cents, Books};
use crate::pdf::{Align, Block, Row, Style};
use rusqlite::{params, Result};

fn bad(msg: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(msg.into())
}

/// One PO line: (description, cents, expense or asset account it's booked to).
pub type PoItem = (String, i64, i64);

#[derive(Clone, Debug, Default)]
pub struct PoInput {
    pub vendor_id: i64,
    pub date: String,
    pub job: Option<i64>,
    pub items: Vec<PoItem>,
    pub notes: String,
}

#[derive(Clone, Debug)]
pub struct Po {
    pub id: i64,
    pub vendor_id: i64,
    pub vendor: String,
    pub date: String,
    pub job: Option<String>,
    pub total: i64,
    /// "open", "received" or "cancelled".
    pub status: String,
    pub received: String,
    /// Received on credit and not yet paid.
    pub owed: bool,
}

impl Books {
    pub fn create_po(&self, d: &PoInput) -> Result<i64> {
        let total: i64 = d.items.iter().map(|i| i.1).sum();
        if d.vendor_id == 0 || d.items.is_empty() || total <= 0 || d.items.iter().any(|i| i.2 == 0) {
            return Err(bad("choose a vendor and give every line an amount and a category"));
        }
        self.atomic(|| {
            self.db.execute(
                "INSERT INTO purchase_orders(vendor_id, date, job_id, notes) VALUES (?1, ?2, ?3, ?4)",
                params![d.vendor_id, d.date, d.job, d.notes.trim()],
            )?;
            let id = self.db.last_insert_rowid();
            for (desc, cents, acct) in &d.items {
                self.db.execute(
                    "INSERT INTO po_items(po_id, description, cents, account_id) VALUES (?1, ?2, ?3, ?4)",
                    params![id, desc.trim(), cents, acct],
                )?;
            }
            Ok(id)
        })
    }

    pub fn pos(&self) -> Result<Vec<Po>> {
        let mut st = self.db.prepare(
            "SELECT p.id, p.vendor_id, v.name, p.date, j.name, (SELECT COALESCE(SUM(cents), 0) FROM po_items WHERE po_id = p.id),
               p.status, p.received, p.entry_id IS NOT NULL AND p.paid_from IS NULL AND p.paid_entry_id IS NULL
             FROM purchase_orders p JOIN vendors v ON v.id = p.vendor_id LEFT JOIN jobs j ON j.id = p.job_id ORDER BY p.id DESC",
        )?;
        let rows = st.query_map([], |r| {
            Ok(Po {
                id: r.get(0)?,
                vendor_id: r.get(1)?,
                vendor: r.get(2)?,
                date: r.get(3)?,
                job: r.get(4)?,
                total: r.get(5)?,
                status: r.get(6)?,
                received: r.get(7)?,
                owed: r.get(8)?,
            })
        })?;
        rows.collect()
    }

    pub fn po_items(&self, id: i64) -> Result<Vec<PoItem>> {
        let mut st = self.db.prepare("SELECT description, cents, account_id FROM po_items WHERE po_id = ?1 ORDER BY id")?;
        let rows = st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect()
    }

    pub fn cancel_po(&self, id: i64) -> Result<()> {
        let n = self.db.execute("UPDATE purchase_orders SET status = 'cancelled' WHERE id = ?1 AND status = 'open'", [id])?;
        if n == 0 {
            return Err(bad("only open purchase orders can be cancelled"));
        }
        Ok(())
    }

    /// The goods arrived: book each line to its category, credited to `paid_from` (a bank or card)
    /// or, when None, to Accounts Payable to pay later. Tagged with the PO's vendor and job.
    pub fn receive_po(&self, id: i64, date: &str, paid_from: Option<i64>) -> Result<()> {
        let (vendor, job, status): (i64, Option<i64>, String) =
            self.db.query_row("SELECT vendor_id, job_id, status FROM purchase_orders WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        if status != "open" {
            return Err(bad(format!("PO #{id} is already {status}")));
        }
        let items = self.po_items(id)?;
        let total: i64 = items.iter().map(|i| i.1).sum();
        let credit = match paid_from {
            Some(b) => b,
            None => self.system_account("ap")?,
        };
        let name: String = self.db.query_row("SELECT name FROM vendors WHERE id = ?1", [vendor], |r| r.get(0))?;
        // The money side first, like every other entry, so the register shows it as the account.
        let mut lines = vec![(credit, -total)];
        lines.extend(items.iter().map(|i| (i.2, i.1)));
        self.atomic(|| {
            let entry = self.post(date, &format!("PO #{id} received from {name}"), &lines)?;
            self.set_tags(entry, job, Some(vendor))?;
            self.db.execute(
                "UPDATE purchase_orders SET status = 'received', received = ?1, entry_id = ?2, paid_from = ?3 WHERE id = ?4",
                params![date, entry, paid_from, id],
            )?;
            Ok(())
        })
    }

    /// Pays a PO that was received on credit: Dr Accounts Payable, Cr the bank or card.
    pub fn pay_po(&self, id: i64, date: &str, bank: i64) -> Result<()> {
        let po = self.pos()?.into_iter().find(|p| p.id == id).ok_or_else(|| bad("no such PO"))?;
        if !po.owed {
            return Err(bad(format!("PO #{id} has nothing owed")));
        }
        let ap = self.system_account("ap")?;
        self.atomic(|| {
            let entry = self.post(date, &format!("Paid {} for PO #{id}", po.vendor), &[(bank, -po.total), (ap, po.total)])?;
            self.set_tags(entry, None, Some(po.vendor_id))?;
            self.db.execute("UPDATE purchase_orders SET paid_entry_id = ?1 WHERE id = ?2", params![entry, id])?;
            Ok(())
        })
    }

    /// What a printed PO shows, top to bottom.
    pub fn po_blocks(&self, id: i64) -> Result<Vec<Block>> {
        let (vendor, email, date, notes, job): (String, String, String, String, Option<String>) = self.db.query_row(
            "SELECT v.name, v.email, p.date, p.notes, j.name FROM purchase_orders p JOIN vendors v ON v.id = p.vendor_id
             LEFT JOIN jobs j ON j.id = p.job_id WHERE p.id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?;
        let items = self.po_items(id)?;
        let mut rows: Vec<Row> = items.iter().map(|(d, c, _)| Row { cells: vec![d.clone(), fmt_cents(*c)], style: Style::Normal }).collect();
        rows.push(Row { cells: vec!["Total".into(), fmt_cents(items.iter().map(|i| i.1).sum())], style: Style::Total });
        let mut blocks = vec![
            self.letterhead()?,
            Block::Title(format!("Purchase Order #{id}")),
            Block::text(format!("Date: {date}{}", job.map_or(String::new(), |j| format!("\nJob: {j}")))),
            Block::Space(6.0),
            Block::bold("Vendor"),
            Block::text([vendor, email].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join("\n")),
            Block::Table { cols: vec![("Description".into(), Align::Left), ("Amount".into(), Align::Right)], rows },
        ];
        if !notes.trim().is_empty() {
            blocks.push(Block::text(notes));
        }
        blocks.push(Block::text(format!("Please reference PO #{id} on your invoice.")));
        Ok(blocks)
    }

    pub fn po_pdf(&self, id: i64) -> std::result::Result<Vec<u8>, String> {
        crate::pdf::render(&format!("Purchase Order #{id}"), &self.po_blocks(id).map_err(|e| e.to_string())?)
    }

    /// (to, subject, body) for emailing a PO to the vendor.
    pub fn po_email(&self, id: i64) -> Result<(String, String, String)> {
        let (vendor, email): (String, String) =
            self.db.query_row("SELECT v.name, v.email FROM purchase_orders p JOIN vendors v ON v.id = p.vendor_id WHERE p.id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        let biz = self.settings()?.name;
        let items = self.po_items(id)?;
        let lines: String = items.iter().map(|(d, c, _)| format!("  {d}: {}\n", fmt_cents(*c))).collect();
        let total = fmt_cents(items.iter().map(|i| i.1).sum());
        let from = if biz.is_empty() { String::new() } else { format!(" from {biz}") };
        let body = format!("Hi {vendor},\n\nPlease supply the following on purchase order #{id}:\n\n{lines}\n  Total: {total}\n\nPlease reference PO #{id} on your invoice.\n\nThank you!\n{biz}\n");
        Ok((email, format!("Purchase Order #{id}{from}"), body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acct(b: &Books, n: &str) -> i64 {
        b.accounts().unwrap().into_iter().find(|a| a.name == n).unwrap().id
    }

    #[test]
    fn po_send_receive_pay() {
        let b = Books::open(":memory:").unwrap();
        let (chk, mat, ap) = (acct(&b, "Checking"), acct(&b, "Job Materials"), acct(&b, "Accounts Payable"));
        let v = b.add_vendor("Lumber Yard", "sales@lumber.test").unwrap();
        b.add_job("Deck", None).unwrap();
        let job = b.jobs().unwrap()[0].id;
        assert!(b.create_po(&PoInput { vendor_id: v, date: "2026-04-01".into(), items: vec![("Boards".into(), 1_000, 0)], ..Default::default() }).is_err());
        let po = b
            .create_po(&PoInput { vendor_id: v, date: "2026-04-01".into(), job: Some(job), items: vec![("Cedar".into(), 60_000, mat), ("Screws".into(), 4_000, mat)], notes: "Deliver Tue".into() })
            .unwrap();
        assert!(crate::pdf::plain_text(&b.po_blocks(po).unwrap()).contains("Purchase Order #1") && b.po_email(po).unwrap().0 == "sales@lumber.test");
        assert!(b.po_pdf(po).unwrap().starts_with(b"%PDF"));
        assert!(b.entries().unwrap().is_empty(), "an open PO books nothing");

        b.receive_po(po, "2026-04-05", None).unwrap();
        assert!(b.receive_po(po, "2026-04-06", None).is_err(), "received once");
        let p = &b.pos().unwrap()[0];
        assert_eq!((p.status.as_str(), p.owed, p.total), ("received", true, 64_000));
        assert_eq!(b.jobs().unwrap()[0].expenses, 64_000, "cost lands on the job");
        let bal = |a: i64| b.db.query_row("SELECT COALESCE(SUM(cents), 0) FROM lines WHERE account_id = ?1", [a], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!(bal(ap), -64_000, "owed to the vendor");
        let receipt = b.entries().unwrap()[0].id;
        assert!(b.delete_entry(receipt).is_err(), "the receipt belongs to the PO");

        b.pay_po(po, "2026-04-20", chk).unwrap();
        assert!(b.pay_po(po, "2026-04-21", chk).is_err());
        assert_eq!((bal(ap), bal(chk)), (0, -64_000));
        assert!(!b.pos().unwrap()[0].owed);

        // Paid on the spot: no payable.
        let po2 = b.create_po(&PoInput { vendor_id: v, date: "2026-05-01".into(), items: vec![("Glue".into(), 900, mat)], ..Default::default() }).unwrap();
        b.receive_po(po2, "2026-05-01", Some(chk)).unwrap();
        assert!(!b.pos().unwrap()[0].owed);
        let po3 = b.create_po(&PoInput { vendor_id: v, date: "2026-05-02".into(), items: vec![("Saw".into(), 9_000, mat)], ..Default::default() }).unwrap();
        b.cancel_po(po3).unwrap();
        assert!(b.receive_po(po3, "2026-05-03", None).is_err());
    }
}
