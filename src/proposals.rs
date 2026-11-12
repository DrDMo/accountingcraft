//! Proposals: longer write-ups than a quote (scope, timeline, terms), kept forever whatever their
//! outcome, with reusable templates. Text uses a tiny markup: "# " heading, "- " bullet, blank line
//! between paragraphs, and fill-ins like {customer} and {pricing}.

use crate::books::{fmt_cents, Books, Doc};
use crate::pdf::Block;
use rusqlite::{params, OptionalExtension, Result};

pub const STATUSES: [&str; 4] = ["draft", "sent", "accepted", "declined"];

#[derive(Clone, Debug, Default)]
pub struct Proposal {
    pub id: i64,
    pub title: String,
    pub customer_id: Option<i64>,
    pub customer: String,
    pub job_id: Option<i64>,
    pub quote_id: Option<i64>,
    pub date: String,
    pub status: String,
    pub body: String,
}

const STARTERS: [(&str, &str); 3] = [
    (
        "Project proposal",
        "# Overview\nThank you for the chance to work with you, {customer}. This proposal covers {title}.\n\n\
         # Scope of work\n- \n- \n- \n\n# Timeline\nWork can begin within two weeks of approval and takes about _ weeks.\n\n\
         # Investment\n{pricing}\n\n# Terms\n- 50% deposit to schedule the work, balance due on completion.\n\
         - Changes to the scope are quoted separately before any extra work starts.\n- This proposal is good for 30 days from {date}.\n\n\
         # Acceptance\nSigned: ______________________  Date: ____________",
    ),
    (
        "Small job",
        "# The job\n{title} for {customer}.\n\n# Price\n{pricing}\n\n# Details\n- Materials and labor included.\n- Payment due on completion.\n\n\
         Good for 30 days from {date}. Reply to this email to go ahead.",
    ),
    (
        "Ongoing service agreement",
        "# Services\n{business} will provide the following on a regular schedule:\n- \n- \n\n# Schedule\nVisits every _ weeks, starting {date}.\n\n\
         # Fees\n{pricing}\n\nInvoiced monthly, due in 30 days.\n\n# Cancellation\nEither side may end this agreement with 30 days' written notice.\n\n\
         # Acceptance\nSigned: ______________________  Date: ____________",
    ),
];

impl Books {
    /// Adds the starter templates once; after that the user's edits and deletions stand.
    pub(crate) fn seed_templates(&self) -> Result<()> {
        let seeded: i64 = self.db.query_row("SELECT templates_seeded FROM settings WHERE id = 1", [], |r| r.get(0))?;
        if seeded == 0 {
            for (name, body) in STARTERS {
                self.db.execute("INSERT OR IGNORE INTO proposal_templates(name, body) VALUES (?1, ?2)", params![name, body])?;
            }
            self.db.execute("UPDATE settings SET templates_seeded = 1 WHERE id = 1", [])?;
        }
        Ok(())
    }

    pub fn proposals(&self) -> Result<Vec<Proposal>> {
        let mut st = self.db.prepare(
            "SELECT p.id, p.title, p.customer_id, COALESCE(c.name, ''), p.job_id, p.quote_id, p.date, p.status, p.body
             FROM proposals p LEFT JOIN customers c ON c.id = p.customer_id ORDER BY p.date DESC, p.id DESC",
        )?;
        let rows = st.query_map([], |r| {
            Ok(Proposal {
                id: r.get(0)?,
                title: r.get(1)?,
                customer_id: r.get(2)?,
                customer: r.get(3)?,
                job_id: r.get(4)?,
                quote_id: r.get(5)?,
                date: r.get(6)?,
                status: r.get(7)?,
                body: r.get(8)?,
            })
        })?;
        rows.collect()
    }

    /// Inserts (id 0) or updates a proposal; returns its id.
    pub fn save_proposal(&self, p: &Proposal) -> Result<i64> {
        if p.title.trim().is_empty() || !STATUSES.contains(&p.status.as_str()) {
            return Err(rusqlite::Error::InvalidParameterName("give the proposal a title".into()));
        }
        if p.id == 0 {
            self.db.execute(
                "INSERT INTO proposals(title, customer_id, job_id, quote_id, date, status, body) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![p.title.trim(), p.customer_id, p.job_id, p.quote_id, p.date, p.status, p.body],
            )?;
            return Ok(self.db.last_insert_rowid());
        }
        self.db.execute(
            "UPDATE proposals SET title = ?1, customer_id = ?2, job_id = ?3, quote_id = ?4, date = ?5, status = ?6, body = ?7 WHERE id = ?8",
            params![p.title.trim(), p.customer_id, p.job_id, p.quote_id, p.date, p.status, p.body, p.id],
        )?;
        Ok(p.id)
    }

    pub fn delete_proposal(&self, id: i64) -> Result<()> {
        self.db.execute("DELETE FROM proposals WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn templates(&self) -> Result<Vec<(i64, String, String)>> {
        let mut st = self.db.prepare("SELECT id, name, body FROM proposal_templates ORDER BY name")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect()
    }

    /// Saves a template, replacing one with the same name.
    pub fn save_template(&self, name: &str, body: &str) -> Result<()> {
        if name.trim().is_empty() || body.trim().is_empty() {
            return Err(rusqlite::Error::InvalidParameterName("name the template and write something in it".into()));
        }
        self.db.execute(
            "INSERT INTO proposal_templates(name, body) VALUES (?1, ?2) ON CONFLICT(name) DO UPDATE SET body = ?2",
            params![name.trim(), body],
        )?;
        Ok(())
    }

    pub fn delete_template(&self, id: i64) -> Result<()> {
        self.db.execute("DELETE FROM proposal_templates WHERE id = ?1", [id])?;
        Ok(())
    }

    fn proposal(&self, id: i64) -> Result<Proposal> {
        self.proposals()?.into_iter().find(|p| p.id == id).ok_or_else(|| rusqlite::Error::InvalidParameterName("no such proposal".into()))
    }

    /// The linked quote's own table (line items, tax, total), so the numbers always match the quote,
    /// plus its total including tax.
    fn pricing(&self, quote: Option<i64>) -> Result<(Block, i64)> {
        let Some(q) = quote else { return Ok((Block::note("(Link a quote to show pricing here.)"), 0)) };
        let items = self.items(Doc::Quote, q)?;
        let tax: i64 = self.db.query_row("SELECT tax_bp FROM quotes WHERE id = ?1", [q], |r| r.get(0)).optional()?.unwrap_or(0);
        let flags = self.item_taxable(Doc::Quote, q)?;
        let sub: i64 = items.iter().map(|i| i.1).sum();
        let taxable: i64 = items.iter().zip(flags.iter().chain(std::iter::repeat(&true))).filter(|(_, t)| **t).map(|(i, _)| i.1).sum();
        Ok((self.doc_table(Doc::Quote, q)?, sub + crate::books::tax_cents(taxable, tax)))
    }

    /// What a printed proposal shows: business header, title, then the body with fill-ins and
    /// markup (# headings, - bullets) applied.
    pub fn proposal_blocks(&self, id: i64) -> Result<Vec<Block>> {
        let p = self.proposal(id)?;
        let s = self.settings()?;
        let (pricing, total) = self.pricing(p.quote_id)?;
        let job = match p.job_id {
            Some(j) => self.db.query_row("SELECT name FROM jobs WHERE id = ?1", [j], |r| r.get::<_, String>(0))?,
            None => String::new(),
        };
        let fill = |t: &str| {
            t.replace("{customer}", &p.customer)
                .replace("{business}", &s.name)
                .replace("{date}", &p.date)
                .replace("{title}", &p.title)
                .replace("{job}", &job)
                .replace("{total}", &fmt_cents(total))
        };
        let mut blocks = vec![self.letterhead()?, Block::Title(p.title.clone()), Block::note(format!("Prepared for {} · {}", p.customer, p.date)), Block::Space(8.0)];
        let mut used_pricing = false;
        for line in p.body.lines() {
            let l = line.trim();
            if l == "{pricing}" {
                blocks.push(pricing.clone());
                used_pricing = true;
            } else if let Some(h) = l.strip_prefix("# ") {
                blocks.push(Block::Heading(fill(h)));
            } else if let Some(b) = l.strip_prefix("- ") {
                blocks.push(Block::Bullet(fill(b)));
            } else if !l.is_empty() {
                blocks.push(Block::text(fill(l)));
            }
        }
        if p.quote_id.is_some() && !used_pricing {
            blocks.push(Block::Heading("Pricing".into()));
            blocks.push(pricing);
        }
        Ok(blocks)
    }

    pub fn proposal_pdf(&self, id: i64) -> std::result::Result<Vec<u8>, String> {
        let title = self.proposal(id).map_err(|e| e.to_string())?.title;
        crate::pdf::render(&title, &self.proposal_blocks(id).map_err(|e| e.to_string())?)
    }

    /// (to, subject, body) for emailing a proposal.
    pub fn proposal_email(&self, id: i64) -> Result<(String, String, String)> {
        let p = self.proposal(id)?;
        let email: String = match p.customer_id {
            Some(c) => self.db.query_row("SELECT email FROM customers WHERE id = ?1", [c], |r| r.get(0))?,
            None => String::new(),
        };
        let biz = self.settings()?.name;
        let body = format!(
            "Hi {},\n\nAttached is our proposal for {}. I'm happy to walk through it or adjust anything.\n\nThank you!\n{biz}\n",
            if p.customer.is_empty() { "there" } else { &p.customer },
            p.title
        );
        Ok((email, format!("Proposal: {}", p.title), body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::books::tests::doc;

    #[test]
    fn proposals_templates_and_rendering() {
        let b = Books::open(":memory:").unwrap();
        assert_eq!(b.templates().unwrap().len(), 3, "starter templates");
        let cust = b.add_customer("Riverside <Cafe>", "owner@cafe.test").unwrap();
        let q = b.create_quote(&doc(cust, "2026-06-01", &[("Counter".into(), 240_000)], None)).unwrap();
        let body = b.templates().unwrap().into_iter().find(|t| t.1 == "Project proposal").unwrap().2;
        let mut p = Proposal { title: "Walnut counter".into(), customer_id: Some(cust), quote_id: Some(q), date: "2026-06-02".into(), status: "draft".into(), body, ..Default::default() };
        assert!(b.save_proposal(&Proposal { title: " ".into(), ..p.clone() }).is_err());
        p.id = b.save_proposal(&p).unwrap();
        let blocks = b.proposal_blocks(p.id).unwrap();
        let text = crate::pdf::plain_text(&blocks);
        assert!(text.contains("Riverside <Cafe>") && text.contains("$2,400.00"));
        assert!(blocks.iter().any(|b| matches!(b, Block::Heading(h) if h == "Scope of work")) && blocks.iter().any(|b| matches!(b, Block::Bullet(_))));
        assert!(!text.contains("{pricing}") && !text.contains("{customer}"));
        assert!(b.proposal_pdf(p.id).unwrap().starts_with(b"%PDF"));

        // Declined proposals are kept.
        p.status = "declined".into();
        b.save_proposal(&p).unwrap();
        assert_eq!(b.proposals().unwrap()[0].status, "declined");
        assert_eq!(b.proposal_email(p.id).unwrap().0, "owner@cafe.test");

        b.save_template("Mine", "# Hello {customer}").unwrap();
        b.save_template("Mine", "# Hi {customer}").unwrap();
        assert_eq!(b.templates().unwrap().iter().filter(|t| t.1 == "Mine").count(), 1, "same name replaces");
    }
}
