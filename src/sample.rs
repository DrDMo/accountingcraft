//! Sample books: a made-up woodworking business with a year of activity, so people can try
//! the app (and reviewers can see every screen) without typing in their own numbers.

use crate::books::{Books, Business, DocInput, Item};
use chrono::{Datelike, Months, NaiveDate};
use rusqlite::Result;

/// Fills fresh books with the sample business: a year of activity, dated relative to `today`.
pub fn fill(b: &Books, today: NaiveDate) -> Result<()> {
    let accounts = b.accounts()?;
    let acct = |name: &str| accounts.iter().find(|a| a.name == name || a.system.as_deref() == Some(name)).map_or(0, |a| a.id);
    // Day `d` of the month `back` months ago, clamped so it never lands after today.
    let day = |back: u32, d: u32| -> String {
        let m = today.checked_sub_months(Months::new(back)).unwrap_or(today);
        let date = NaiveDate::from_ymd_opt(m.year(), m.month(), d.min(28)).unwrap_or(m);
        date.min(today).to_string()
    };
    b.save_business(&Business {
        name: "Riverside Woodworks (sample)".into(),
        contact: "Sam Rivera".into(),
        street: "12 Mill Road".into(),
        city: "Portland".into(),
        state: "OR".into(),
        zip: "97201".into(),
        phone: "503-555-0100".into(),
        email: "hello@riverside.example".into(),
        ..Default::default()
    })?;
    let (chk, card) = (acct("Checking"), acct("Credit Card"));
    b.post_simple(&day(12, 1), "Owner investment", chk, acct("Owner's Contributions"), 800_000)?;

    let acme = b.add_customer("Acme Coffee", "orders@acme.example")?;
    let dental = b.add_customer("Harbor Dental", "office@harbordental.example")?;
    let juniper = b.add_customer("Juniper Studio", "hi@juniper.example")?;
    b.add_vendor("Cedar Lumber Yard", "sales@cedarlumber.example")?;
    let job = b.add_job("Acme café counters", Some(acme))?;

    for back in (0..12u32).rev() {
        let wobble = i64::from(back * 37 % 9); // uneven months look real
        b.post_simple(&day(back, 1), "Shop rent", chk, acct("Rent or Lease"), -120_000)?;
        b.post_simple(&day(back, 5), "Phone and internet", card, acct("Phone & Internet"), -8_500)?;
        b.post_simple(&day(back, 8), "Design software", card, acct("Software & Subscriptions"), -4_900)?;
        b.post_simple(&day(back, 12), "Cedar Lumber Yard", chk, acct("Job Materials"), -(38_000 + 6_000 * wobble))?;
        b.post_simple(&day(back, 15), "Shelving install, walk-in client", chk, acct("Service Income"), 210_000 + 15_000 * wobble)?;
        b.post_simple(&day(back, 20), "Fuel", card, acct("Car & Truck"), -6_200)?;
        b.post_simple(&day(back, 25), "Credit card payment", chk, card, -18_000)?;
    }
    b.post_simple(&day(0, 3), "Lunch with client", card, acct("Meals"), -4_250)?;
    b.post_simple(&day(0, 6), "SQ *HARDWARE 4471", chk, acct("uncat_out"), -3_918)?; // left for the person to categorize

    let doc = |customer_id: i64, date: String, due_date: String, items: &[(&str, i64)], job: Option<i64>| DocInput {
        customer_id,
        date,
        items: items.iter().map(|(d, c)| (d.to_string(), *c)).collect::<Vec<Item>>(),
        job,
        tax_bp: 0,
        due_date,
        taxable: vec![],
        customer_po: String::new(),
    };
    let paid = b.create_invoice(&doc(acme, day(3, 2), day(2, 2), &[("Walnut counter, 8 ft, oil finish", 240_000), ("Install", 35_000)], Some(job)))?;
    b.record_payment(paid, &day(2, 1), chk, 275_000)?;
    b.create_invoice(&doc(dental, day(2, 10), day(1, 10), &[("Reception desk, white oak", 310_000)], None))?; // overdue
    b.create_invoice(&doc(juniper, day(0, 2), (today + chrono::TimeDelta::days(30)).to_string(), &[("Floating shelves (6)", 54_000), ("Delivery", 7_500)], None))?;
    b.create_quote(&doc(acme, day(0, 4), String::new(), &[("Second-location counter", 260_000), ("Install", 35_000)], Some(job)))?;

    b.add_time(job, &day(1, 9), 360, 7_500, "Template and cut tops")?;
    b.add_time(job, &day(0, 2), 240, 7_500, "Finish and fit")?;
    b.add_trip(&day(1, 9), "Acme site visit", 184, Some(job))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn sample_books_fill() {
        let b = crate::books::Books::open(":memory:").unwrap();
        let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        super::fill(&b, today).unwrap();
        assert_eq!(b.customers().unwrap().len(), 3);
        assert_eq!(b.invoices().unwrap().len(), 3);
        assert!(b.entries().unwrap().iter().all(|e| e.date.as_str() <= "2026-03-01"), "nothing dated in the future");
    }
}
