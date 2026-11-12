// Release builds open without a console window behind the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Craft house rule: non-test code never panics.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod books;
mod extras;
mod feed;
mod help;
mod icon;
mod lock;
mod mail;
mod package;
mod pdf;
mod proposals;
mod purchasing;
mod reports;
mod sample;
mod ui;

use books::{fmt_cents, nice_dates, net_income, parse_cents, parse_rate_bp, Account, Books, DocInput, Entry, Invoice, Item, JobTotals, Party, Quote, Settings, KINDS};
use eframe::egui::{self, RichText};
use extras::{Asset, Recurring, Rule, TimeEntry, Trip};
use chrono::{Datelike, NaiveDate as Date};
use ui::{ACCENT, MONEY_IN, MONEY_OUT};

#[derive(PartialEq, Clone, Copy)]
enum Screen {
    Home,
    Transactions,
    Reports,
    Invoices,
    Import,
    Jobs,
    People,
    Reconcile,
    Mileage,
    Assets,
    Purchases,
    Proposals,
    Accounts,
    Business,
    Help,
}

/// Screen names used by the help topics and tutorials.
const SCREENS: [(Screen, &str); 15] = [
    (Screen::Home, "home"),
    (Screen::Transactions, "transactions"),
    (Screen::Invoices, "invoices"),
    (Screen::Jobs, "jobs"),
    (Screen::Purchases, "purchases"),
    (Screen::People, "people"),
    (Screen::Proposals, "proposals"),
    (Screen::Reports, "reports"),
    (Screen::Reconcile, "reconcile"),
    (Screen::Mileage, "mileage"),
    (Screen::Assets, "assets"),
    (Screen::Import, "import"),
    (Screen::Accounts, "accounts"),
    (Screen::Business, "business"),
    (Screen::Help, "help"),
];

fn screen_key(s: Screen) -> &'static str {
    SCREENS.iter().find(|x| x.0 == s).map_or("help", |x| x.1)
}

fn screen_of(key: &str) -> Screen {
    SCREENS.iter().find(|x| x.1 == key).map_or(Screen::Home, |x| x.0)
}

#[derive(PartialEq, Clone, Copy)]
enum Direction {
    Out,
    In,
    Transfer,
}

/// The transaction being edited in the Edit dialog.
struct EditTx {
    id: i64,
    date: Date,
    memo: String,
    amount: String,
    money_in: bool,
    bank: i64,
    receipt: bool,
}

struct App {
    /// Who is signed in, their role and the books key (for managing logins).
    user: String,
    role: lock::Role,
    key: lock::Key,
    /// Set to lock the books (drop back to the sign-in screen) or switch to another file.
    lock_now: bool,
    switch_to: Option<String>,
    // Logins card
    u_name: String,
    u_pw: String,
    u_role: lock::Role,
    u_my_pw: String,
    u_recovery: Option<String>,
    /// Changing the encryption key: the owner's password, then the temporary passwords to hand out.
    u_rot_pw: String,
    u_temps: Vec<(String, String)>,
    /// Help pop-up showing this screen's topic.
    help_topic: Option<&'static str>,
    /// Running tutorial: (which, step).
    tour: Option<(usize, usize)>,
    books: Books,
    path: String,
    screen: Screen,
    status: String,
    dirty: bool,
    logo: Option<egui::TextureHandle>,
    accounts: Vec<Account>,
    entries: Vec<Entry>,
    invoices: Vec<Invoice>,
    quotes: Vec<Quote>,
    customers: Vec<Party>,
    vendors: Vec<Party>,
    jobs: Vec<JobTotals>,
    settings: Settings,
    feeds: Vec<(String, String, Option<i64>)>,
    rules: Vec<Rule>,
    recurring: Vec<Recurring>,
    unbilled: Vec<TimeEntry>,
    trips: Vec<Trip>,
    monthly: Vec<(String, i64, i64)>,
    // Transactions
    t_date: Date,
    t_memo: String,
    t_amount: String,
    t_dir: Direction,
    t_bank: i64,
    t_cat: i64,
    t_to: i64,
    t_job: Option<i64>,
    t_vendor: Option<i64>,
    t_repeat: &'static str,
    reg_job: Option<i64>,
    reg_search: String,
    reg_uncat: bool,
    confirm_delete: Option<i64>,
    edit: Option<EditTx>,
    // Reports
    r_kind: reports::Kind,
    r_detail: bool,
    /// The accountant package panel is showing.
    r_pkg: bool,
    r_from: Date,
    r_to: Date,
    r_account: Option<i64>,
    r_job: Option<i64>,
    // Customers, vendors, invoices
    c_name: String,
    c_email: String,
    v_name: String,
    v_email: String,
    i_cust: i64,
    i_date: Date,
    i_items: Vec<(String, String, bool)>, // (description, amount text, taxable)
    i_bank: i64,
    i_job: Option<i64>,
    i_tax: String,
    i_terms: i64,
    i_repeat: &'static str,
    i_po: String,
    /// Payment dialog: (true for a deposit from customer id, else a payment on invoice id; amount; date).
    pay_dlg: Option<(bool, i64, String, Date)>,
    // Jobs and time
    j_name: String,
    j_cust: Option<i64>,
    j_new_cust: bool,
    tm_date: Date,
    tm_hours: String,
    tm_rate: String,
    tm_note: String,
    // Import
    m_path: String,
    m_bank: i64,
    m_header: bool,
    m_cols: [usize; 3], // 1-based date, description, amount
    f_token: String,
    rl_pattern: String,
    rl_account: i64,
    rl_vendor: Option<i64>,
    rl_job: Option<i64>,
    people_q: String,
    /// The customer (false) or vendor (true) being added or edited.
    party_edit: Option<(bool, Party)>,
    /// Quotes & Invoices tab: 1 quotes (shown first), 0 invoices, 2 new.
    inv_tab: u8,
    inv_q: String,
    /// Invoice filter: 0 all, 1 unpaid, 2 overdue, 3 paid.
    inv_filter: u8,
    reg_bulk_job: Option<i64>,
    job_open: Option<i64>,
    // Proposals
    proposals: Vec<proposals::Proposal>,
    templates: Vec<(i64, String, String)>,
    prop: Option<proposals::Proposal>,
    prop_q: String,
    prop_filter: &'static str,
    tpl_pick: Option<i64>,
    tpl_name: String,
    prop_new_cust: bool,
    prop_new_job: bool,
    inv_new_cust: bool,
    inv_new_job: bool,
    /// The New form makes a quote (true) or an invoice (false).
    i_quote: bool,
    t_new_vendor: bool,
    t_new_cat: bool,
    t_cat_name: String,
    t_new_job: bool,
    t_new_jobcust: bool,
    // Purchase orders
    pos: Vec<purchasing::Po>,
    po_vendor: Option<i64>,
    po_new_vendor: bool,
    po_new_cat: bool,
    po_date: Date,
    po_job: Option<i64>,
    po_items: Vec<(String, String, i64)>, // (description, amount text, category account)
    po_notes: String,
    /// Receive (true) or pay (false) dialog: (receive?, po id, date, paid from).
    po_dlg: Option<(bool, i64, Date, Option<i64>)>,
    // Assets
    assets: Vec<Asset>,
    as_new: Asset,
    as_cost: String,
    as_date: Date,
    as_paid_from: Option<i64>,
    as_new_cat: bool,
    as_cat_intangible: bool,
    as_cat_name: String,
    // Reconcile
    rc_account: i64,
    rc_date: Date,
    rc_ending: String,
    // Mileage
    mi_date: Date,
    mi_purpose: String,
    mi_miles: String,
    mi_job: Option<i64>,
    // Accounts
    a_number: String,
    a_name: String,
    a_kind: &'static str,
    a_desc: String,
    // Business
    biz: books::Business,
    p_lock_on: bool,
    p_lock: Date,
    p_pay_link: String,
    p_mileage: String,
    p_hourly: String,
    p_tax_pct: String,
}

fn today() -> Date {
    chrono::Local::now().date_naive()
}

fn add_days(d: Date, n: i64) -> Date {
    chrono::TimeDelta::try_days(n).and_then(|t| d.checked_add_signed(t)).unwrap_or(d)
}

/// Money as plain text for an input box ("1234.50"), the way parse_cents reads it back.
fn plain(c: i64) -> String {
    format!("{}{}.{:02}", if c < 0 { "-" } else { "" }, c.abs() / 100, c.abs() % 100)
}

/// A calendar button; `salt` keeps several pickers on one screen apart.
/// egui's picker speaks jiff; the app keeps dates in chrono (Craft house rule), so convert here.
fn date_picker(ui: &mut egui::Ui, salt: &str, d: &mut Date) {
    let (Ok(y), Ok(m), Ok(day)) = (i16::try_from(d.year()), i8::try_from(d.month()), i8::try_from(d.day())) else { return };
    let Ok(mut j) = jiff::civil::Date::new(y, m, day) else { return };
    ui.add(egui_extras::DatePickerButton::new(&mut j).id_salt(salt).format("%b %-d, %Y").calendar_week(false));
    if let Some(n) = Date::from_ymd_opt(j.year().into(), j.month().unsigned_abs().into(), j.day().unsigned_abs().into()) {
        *d = n;
    }
}

/// Where the app remembers which books file was open last.
fn last_books_file() -> Option<std::path::PathBuf> {
    // Windows: %APPDATA%; macOS: ~/Library/Application Support; Linux: $XDG_CONFIG_HOME or ~/.config.
    let base = std::env::var_os("APPDATA").or_else(|| std::env::var_os("XDG_CONFIG_HOME")).map(std::path::PathBuf::from).or_else(|| {
        let home = std::path::PathBuf::from(std::env::var_os("HOME")?);
        Some(if cfg!(target_os = "macos") { home.join("Library/Application Support") } else { home.join(".config") })
    })?;
    Some(base.join("AccountingCraft").join("last-books.txt"))
}

/// Where the chosen Appearance (System, Dark or Light) is kept between runs.
fn theme_file() -> Option<std::path::PathBuf> {
    last_books_file().map(|f| f.with_file_name("appearance.txt"))
}

fn saved_theme() -> egui::ThemePreference {
    match theme_file().and_then(|f| std::fs::read_to_string(f).ok()).as_deref().map(str::trim) {
        Some("Dark") => egui::ThemePreference::Dark,
        Some("Light") => egui::ThemePreference::Light,
        _ => egui::ThemePreference::System,
    }
}

fn remember_books(path: &str) {
    if let Some(f) = last_books_file() {
        let _ = f.parent().map(std::fs::create_dir_all);
        // Absolute, so the shortcut finds it whatever folder it starts in.
        let abs = std::fs::canonicalize(path).map(|p| p.display().to_string()).unwrap_or(path.to_string());
        let _ = std::fs::write(f, abs.trim_start_matches(r"\\?\"));
    }
}

impl App {
    fn new(s: lock::Session, path: String) -> Self {
        let t = today();
        let books = s.books;
        let mut app = App {
            user: s.user,
            role: s.role,
            key: s.key,
            lock_now: false,
            switch_to: None,
            u_name: String::new(),
            u_pw: String::new(),
            u_role: lock::Role::Bookkeeper,
            u_my_pw: String::new(),
            u_recovery: None,
            u_rot_pw: String::new(),
            u_temps: vec![],
            help_topic: None,
            tour: None,
            books,
            path,
            screen: Screen::Home,
            status: String::new(),
            dirty: true,
            logo: None,
            accounts: vec![],
            entries: vec![],
            invoices: vec![],
            quotes: vec![],
            customers: vec![],
            vendors: vec![],
            jobs: vec![],
            settings: Settings::default(),
            feeds: vec![],
            rules: vec![],
            recurring: vec![],
            unbilled: vec![],
            trips: vec![],
            monthly: vec![],
            t_date: t,
            t_memo: String::new(),
            t_amount: String::new(),
            t_dir: Direction::Out,
            t_bank: 0,
            t_cat: 0,
            t_to: 0,
            t_job: None,
            t_vendor: None,
            t_repeat: "",
            reg_job: None,
            reg_search: String::new(),
            reg_uncat: false,
            confirm_delete: None,
            edit: None,
            r_kind: reports::Kind::ProfitLoss,
            r_detail: false,
            r_pkg: false,
            r_from: Date::from_ymd_opt(t.year(), 1, 1).unwrap_or(t),
            r_to: Date::from_ymd_opt(t.year(), 12, 31).unwrap_or(t),
            r_account: None,
            r_job: None,
            c_name: String::new(),
            c_email: String::new(),
            v_name: String::new(),
            v_email: String::new(),
            i_cust: 0,
            i_date: t,
            i_items: vec![(String::new(), String::new(), true)],
            i_bank: 0,
            i_job: None,
            i_tax: String::new(),
            i_terms: 30,
            i_repeat: "",
            i_po: String::new(),
            pay_dlg: None,
            j_name: String::new(),
            j_cust: None,
            j_new_cust: false,
            tm_date: t,
            tm_hours: String::new(),
            tm_rate: String::new(),
            tm_note: String::new(),
            m_path: String::new(),
            m_bank: 0,
            m_header: true,
            m_cols: [1, 2, 3],
            f_token: String::new(),
            rl_pattern: String::new(),
            rl_account: 0,
            rl_vendor: None,
            rl_job: None,
            people_q: String::new(),
            party_edit: None,
            inv_tab: 1,
            inv_q: String::new(),
            inv_filter: 0,
            reg_bulk_job: None,
            job_open: None,
            proposals: vec![],
            templates: vec![],
            prop: None,
            prop_q: String::new(),
            prop_filter: "",
            tpl_pick: None,
            tpl_name: String::new(),
            prop_new_cust: false,
            prop_new_job: false,
            inv_new_cust: false,
            inv_new_job: false,
            i_quote: true,
            t_new_vendor: false,
            t_new_cat: false,
            t_cat_name: String::new(),
            t_new_job: false,
            t_new_jobcust: false,
            pos: vec![],
            po_vendor: None,
            po_new_vendor: false,
            po_new_cat: false,
            po_date: t,
            po_job: None,
            po_items: vec![(String::new(), String::new(), 0)],
            po_notes: String::new(),
            po_dlg: None,
            assets: vec![],
            as_new: Asset { life_years: 5, ..Default::default() },
            as_cost: String::new(),
            as_date: t,
            as_paid_from: None,
            as_new_cat: false,
            as_cat_intangible: false,
            as_cat_name: String::new(),
            rc_account: 0,
            rc_date: t,
            rc_ending: String::new(),
            mi_date: t,
            mi_purpose: String::new(),
            mi_miles: String::new(),
            mi_job: None,
            a_number: String::new(),
            a_name: String::new(),
            a_kind: "expense",
            a_desc: String::new(),
            biz: Default::default(),
            p_lock_on: false,
            p_lock: t,
            p_pay_link: String::new(),
            p_mileage: String::new(),
            p_hourly: String::new(),
            p_tax_pct: String::new(),
        };
        // Post anything recurring that came due while the app was closed.
        match if app.role == lock::Role::Viewer { Ok(0) } else { app.books.run_recurring(&t.to_string()) } {
            Ok(0) => {}
            Ok(n) => app.status = format!("Posted {n} recurring item(s) that came due."),
            Err(e) => app.status = format!("Error: {e}"),
        }
        app.refresh();
        let first_bank = app.accounts.iter().find(|a| a.kind == "asset").map_or(0, |a| a.id);
        (app.t_bank, app.i_bank, app.m_bank, app.rc_account) = (first_bank, first_bank, first_bank, first_bank);
        app.load_prefs();
        app
    }

    /// Copies saved settings into the Business screen's edit boxes.
    fn load_prefs(&mut self) {
        let s = &self.settings;
        self.biz = s.biz.clone();
        self.p_lock_on = !s.lock_date.is_empty();
        self.p_lock = s.lock_date.parse().unwrap_or(today());
        self.p_pay_link = s.pay_link.clone();
        self.p_mileage = plain(s.mileage_rate);
        self.p_hourly = plain(s.hourly_rate);
        self.p_tax_pct = s.income_tax_pct.to_string();
        if s.hourly_rate > 0 {
            self.tm_rate = plain(s.hourly_rate);
        }
    }

    fn refresh(&mut self) {
        let year = today().year();
        let first = today().checked_sub_months(chrono::Months::new(11)).unwrap_or(today());
        let r = (|| -> rusqlite::Result<()> {
            self.accounts = self.books.accounts()?;
            self.entries = self.books.entries()?;
            self.invoices = self.books.invoices()?;
            self.quotes = self.books.quotes()?;
            self.customers = self.books.customers()?;
            self.vendors = self.books.vendors()?;
            self.jobs = self.books.jobs()?;
            self.settings = self.books.settings()?;
            self.feeds = self.books.feed_accounts()?;
            self.rules = self.books.rules()?;
            self.assets = self.books.assets()?;
            self.pos = self.books.pos()?;
            self.proposals = self.books.proposals()?;
            self.templates = self.books.templates()?;
            self.recurring = self.books.recurring()?;
            self.unbilled = self.books.unbilled_time()?;
            self.trips = self.books.trips(&format!("{year}-01-01"), &format!("{year}-12-31"))?;
            self.monthly = self.books.monthly_totals(&first.format("%Y-%m").to_string(), 12)?;
            Ok(())
        })();
        if let Err(e) = r {
            self.status = format!("Error loading: {e}");
        }
        self.dirty = false;
    }

    /// Shows a message for a write and reloads cached data.
    fn done<T>(&mut self, r: Result<T, impl std::fmt::Display>, ok: &str) -> bool {
        self.dirty = true;
        match r {
            Ok(_) => {
                self.status = ok.to_string();
                true
            }
            Err(e) => {
                let e = e.to_string();
                self.status = if e.contains("readonly") { "Error: you have view-only access to these books.".into() } else { format!("Error: {e}") };
                false
            }
        }
    }

    /// Switches to another books file (or a new one), backing it up first.
    fn open_books(&mut self, path: String) {
        self.switch_to = Some(path); // Root signs in to it
    }

    /// Writes a PDF to the temp folder and opens it in the default PDF viewer, to print or save.
    fn open_pdf(&mut self, name: &str, pdf: Result<Vec<u8>, String>) {
        let r = pdf.and_then(|pdf| {
            let path = std::env::temp_dir().join(format!("AccountingCraft-{name}.pdf"));
            std::fs::write(&path, pdf).map_err(|e| format!("couldn't write the PDF (is it still open in another program?): {e}"))?;
            open_external(&path.display().to_string())
        });
        self.done(r, "Opened the PDF. Print it or save a copy from there.");
    }

    fn open_doc(&mut self, doc: books::Doc, id: i64) {
        let kind = if doc == books::Doc::Invoice { "invoice" } else { "quote" };
        let pdf = self.books.document_pdf(doc, id);
        self.open_pdf(&format!("{kind}-{id}"), pdf);
    }

    /// Emails an invoice or quote with its PDF attached.
    fn email_doc(&mut self, doc: books::Doc, id: i64, reminder: bool) {
        let kind = if doc == books::Doc::Invoice { "Invoice" } else { "Quote" };
        let r = self.books.document_email(doc, id, reminder).map_err(|e| e.to_string()).and_then(|m| Ok((m, self.books.document_pdf(doc, id)?)));
        match r {
            Ok(((to, subject, body), pdf)) => self.send_pdf_email(to, subject, body, pdf, &format!("{kind} {id}"), "customer", "Customers & Vendors"),
            Err(e) => self.status = format!("Error: {e}"),
        }
    }

    /// Saves the PDF and opens an email with it attached, on a helper thread (some mail programs
    /// hold the call open while composing).
    #[allow(clippy::too_many_arguments)]
    fn send_pdf_email(&mut self, to: String, subject: String, body: String, pdf: Vec<u8>, file_stem: &str, who: &str, where_: &str) {
        if to.is_empty() {
            self.status = format!("Error: this {who} has no email address yet; add it on {where_}");
            return;
        }
        let dir = std::env::temp_dir().join("AccountingCraft-mail");
        let path = dir.join(format!("{file_stem}.pdf"));
        if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, pdf)) {
            self.status = format!("Error: {e}");
            return;
        }
        std::thread::spawn(move || {
            if mail::send_with_attachment(&to, &subject, &body, &path).is_err() {
                // No mail program takes attachments this way (e.g. webmail): start the email and show the PDF to drag in.
                let _ = open_external(&format!("mailto:{}?subject={}&body={}", url_enc(&to), url_enc(&subject), url_enc(&body)));
                reveal(&path);
            }
        });
        self.status = "Opening your email with the PDF attached… If it isn't attached, drag it in from the folder that opens.".into();
    }

    fn open_receipt(&mut self, entry: i64) {
        let r = self.books.attachment(entry).map_err(|e| e.to_string()).and_then(|a| {
            let (name, data) = a.ok_or("no receipt on this transaction")?;
            let safe: String = name.chars().map(|c| if c.is_alphanumeric() || ".-_".contains(c) { c } else { '_' }).collect();
            let path = std::env::temp_dir().join(format!("AccountingCraft-receipt-{entry}-{safe}"));
            std::fs::write(&path, data).map_err(|e| e.to_string())?;
            open_external(&path.display().to_string())
        });
        self.done(r, "Opened the receipt.");
    }

    fn uncategorized_ids(&self) -> Vec<i64> {
        self.accounts.iter().filter(|a| matches!(a.system.as_deref(), Some("uncat_in" | "uncat_out"))).map(|a| a.id).collect()
    }

    fn account(&self, id: i64) -> Option<&Account> {
        self.accounts.iter().find(|a| a.id == id)
    }
}

fn url_enc(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.~@".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

/// Shows a file selected in the system file manager (Linux opens its folder).
fn reveal(path: &std::path::Path) {
    #[cfg(windows)]
    let r = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg("-R").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(path.parent().unwrap_or(path)).spawn();
    let _ = r;
}

/// Hands a URL or file (mailto:, a path) to the OS. eframe's open_url always goes to the web browser.
fn open_external(target: &str) -> Result<(), String> {
    #[cfg(windows)]
    let mut c = {
        let mut c = std::process::Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(target_os = "macos")]
    let mut c = std::process::Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut c = std::process::Command::new("xdg-open");
    c.arg(target).spawn().map(|_| ()).map_err(|e| format!("couldn't open it: {e}"))
}

/// Add form plus an editable list. Customers get an address column, vendors a 1099 checkbox.
/// Company, DBA, contact, address, phones, email and website: shared by your business and by customers and vendors.
fn contact_fields(ui: &mut egui::Ui, salt: &str, b: &mut books::Business) {
    egui::Grid::new(salt).num_columns(4).spacing([16.0, 10.0]).show(ui, |ui| {
        ui::label(ui, "Company name");
        field(ui, &mut b.name, 300.0, "Legal name, e.g. Smith Woodworks LLC");
        ui::label(ui, "DBA");
        field(ui, &mut b.dba, 300.0, "Trade names, comma-separated");
        ui.end_row();
        ui::label(ui, "Contact name");
        field(ui, &mut b.contact, 300.0, "The person you deal with");
        ui::label(ui, "Website");
        field(ui, &mut b.website, 300.0, "e.g. smithwoodworks.com");
        ui.end_row();
        ui::label(ui, "Street");
        field(ui, &mut b.street, 300.0, "123 Main St");
        ui::label(ui, "Suite / unit");
        field(ui, &mut b.street2, 300.0, "optional");
        ui.end_row();
        ui::label(ui, "City");
        field(ui, &mut b.city, 300.0, "");
        ui::label(ui, "State / ZIP");
        ui.horizontal(|ui| {
            field(ui, &mut b.state, 80.0, "TX");
            field(ui, &mut b.zip, 120.0, "ZIP");
        });
        ui.end_row();
        ui::label(ui, "Phone");
        field(ui, &mut b.phone, 300.0, "Main number");
        ui::label(ui, "Other phone");
        field(ui, &mut b.phone2, 300.0, "Mobile or fax (optional)");
        ui.end_row();
        ui::label(ui, "Email");
        field(ui, &mut b.email, 300.0, "name@example.com");
        ui.end_row();
    });
}

/// True when every word of `query` appears somewhere in `text`, ignoring case and order
/// ("cafe river" finds "Riverside Cafe").
fn matches(text: &str, query: &str) -> bool {
    let t = text.to_lowercase();
    query.split_whitespace().all(|w| t.contains(&w.to_lowercase()))
}

/// A search box at the top of a long dropdown. Its text lives in egui memory under the popup's id.
fn popup_search(ui: &mut egui::Ui) -> String {
    let id = ui.scope_id().with("search");
    let mut q: String = ui.data_mut(|d| d.get_temp(id).unwrap_or_default());
    let r = ui.add(egui::TextEdit::singleline(&mut q).hint_text("Type to search…").desired_width(f32::INFINITY));
    if ui.memory(|m| m.focused().is_none()) {
        r.request_focus(); // typing goes straight into the search when the list opens
    }
    ui.data_mut(|d| d.insert_temp(id, q.clone()));
    q
}

/// Account picker limited to the given kinds. Returns true when the selection changed.
fn account_combo(ui: &mut egui::Ui, salt: impl egui::AsIdSalt, accounts: &[Account], kinds: &[&str], sel: &mut i64) -> bool {
    let before = *sel;
    let label = |a: &Account| if a.number.is_empty() { a.name.clone() } else { format!("{} · {}", a.number, a.name) };
    let text = accounts.iter().find(|a| a.id == *sel).map_or("Choose…".to_string(), label);
    egui::ComboBox::from_id_salt(salt).selected_text(text).width(ui.spacing().combo_width).wrap_mode(egui::TextWrapMode::Truncate).show_ui(ui, |ui| {
        // Turned-off accounts stay hidden unless already selected.
        let cur = *sel;
        let shown: Vec<&Account> = accounts.iter().filter(|a| kinds.contains(&a.kind.as_str()) && (a.active || a.id == cur)).collect();
        let q = if shown.len() > 8 { popup_search(ui) } else { String::new() };
        for a in shown.into_iter().filter(|a| matches(&format!("{} {} {}", a.number, a.name, a.kind), &q)) {
            ui.selectable_value(sel, a.id, label(a));
        }
    });
    *sel != before
}

/// Optional picker (job, vendor, customer). Returns true when the selection changed.
fn pick<'a>(ui: &mut egui::Ui, salt: impl egui::AsIdSalt, items: impl Iterator<Item = (i64, &'a str)> + Clone, sel: &mut Option<i64>, none: &str) -> bool {
    let before = *sel;
    let text = items.clone().find(|i| Some(i.0) == *sel).map_or(none, |i| i.1).to_string();
    egui::ComboBox::from_id_salt(salt).selected_text(text).width(ui.spacing().combo_width * 0.72).wrap_mode(egui::TextWrapMode::Truncate).show_ui(ui, |ui| {
        let q = if items.clone().count() > 8 { popup_search(ui) } else { String::new() };
        ui.selectable_value(sel, None, none);
        for (id, name) in items.filter(|i| matches(i.1, &q)) {
            ui.selectable_value(sel, Some(id), name);
        }
    });
    *sel != before
}

fn repeat_combo(ui: &mut egui::Ui, salt: &str, sel: &mut &'static str) {
    let opts = [("", "Doesn't repeat"), ("week", "Every week"), ("month", "Every month"), ("year", "Every year")];
    let text = opts.iter().find(|o| o.0 == *sel).map_or("", |o| o.1);
    egui::ComboBox::from_id_salt(salt).selected_text(text).width(150.0).show_ui(ui, |ui| {
        for (k, l) in opts {
            ui.selectable_value(sel, k, l);
        }
    });
}

const TERMS: [(i64, &str); 4] = [(0, "Due on receipt"), (15, "Net 15"), (30, "Net 30"), (60, "Net 60")];
const BANKS: &[&str] = &["asset", "liability"];

/// Signed amount in green (in) or red (out).
fn money(cents: i64) -> RichText {
    let text = if cents > 0 { format!("+{}", fmt_cents(cents)) } else { fmt_cents(cents) };
    RichText::new(text).color(if cents > 0 { MONEY_IN } else { MONEY_OUT })
}

/// Right-aligns money in a fixed-width table cell so the decimal points line up.
fn amount_cell(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) {
    let size = egui::vec2(120.0, ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(size, egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.set_min_size(size); // claim the whole cell, or the next column starts over the text
        ui.label(text)
    });
}

/// Column header for a money column, right-aligned to match `amount_cell`.
fn th_num(ui: &mut egui::Ui, text: &str) {
    let size = egui::vec2(120.0, ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(size, egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.set_min_size(size);
        ui::th(ui, text)
    });
}

/// Single-line text box at a fixed width. Inside a Grid, a plain TextEdit shrinks to the column's
/// previous width; a fixed size makes the column grow to fit instead.
fn field(ui: &mut egui::Ui, text: &mut String, width: f32, hint: &str) -> egui::Response {
    ui.add_sized([width, 32.0], egui::TextEdit::singleline(text).hint_text(hint))
}

/// Income (green) and expense (red) bars per month, with the amounts on hover.
fn month_chart(ui: &mut egui::Ui, data: &[(String, i64, i64)]) {
    let p = ui::pal(ui);
    let h = 170.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h + 24.0), egui::Sense::hover());
    let max = data.iter().map(|d| d.1.max(d.2)).max().unwrap_or(0).max(1) as f32;
    let slot = rect.width() / data.len().max(1) as f32;
    let bw = (slot * 0.3).min(22.0);
    let base = rect.top() + h;
    let painter = ui.painter();
    painter.line_segment([egui::pos2(rect.left(), base), egui::pos2(rect.right(), base)], egui::Stroke::new(1.0, p.border));
    for (i, (m, inc, exp)) in data.iter().enumerate() {
        let cx = rect.left() + slot * (i as f32 + 0.5);
        for (j, (v, c)) in [(*inc, MONEY_IN), (*exp, MONEY_OUT)].into_iter().enumerate() {
            let bh = (v.max(0) as f32 / max * (h - 10.0)).max(if v > 0 { 2.0 } else { 0.0 });
            let x0 = cx - bw - 1.0 + j as f32 * (bw + 2.0);
            painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x0, base - bh), egui::pos2(x0 + bw, base)), 3.0, c);
        }
        let label = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
            [m.get(5..7).and_then(|x| x.parse::<usize>().ok()).unwrap_or(1).clamp(1, 12) - 1];
        painter.text(egui::pos2(cx, base + 6.0), egui::Align2::CENTER_TOP, label, egui::FontId::proportional(12.5), p.weak);
    }
    if let Some(pos) = resp.hover_pos() {
        let i = (((pos.x - rect.left()) / slot) as usize).min(data.len().saturating_sub(1));
        if let Some((m, inc, exp)) = data.get(i) {
            resp.on_hover_text(format!("{m}\nIncome {}\nExpenses {}\nProfit {}", fmt_cents(*inc), fmt_cents(*exp), fmt_cents(inc - exp)));
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.dirty {
            self.refresh();
        }
        if self.screen == Screen::Business && self.role != lock::Role::Owner {
            self.screen = Screen::Home;
        }
        let p = ui::pal(ui);
        let logo = self
            .logo
            .get_or_insert_with(|| {
                let img = egui::ColorImage::from_rgba_unmultiplied([64, 64], &icon::abacus(64));
                ui.ctx().load_texture("logo", img, egui::TextureOptions::LINEAR)
            })
            .id();

        if !self.status.is_empty() {
            egui::Panel::bottom("status")
                .frame(egui::Frame::new().fill(p.side).inner_margin(egui::Margin::symmetric(20, 8)).stroke(egui::Stroke::new(1.0, p.border)))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let err = self.status.starts_with("Error");
                        ui.label(RichText::new("●").color(if err { MONEY_OUT } else { MONEY_IN }));
                        ui.label(&self.status);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("✕").on_hover_text("Dismiss").clicked() {
                                self.status.clear();
                            }
                        });
                    });
                });
        }
        let mut open_path = None;
        egui::Panel::left("nav")
            .resizable(false)
            .exact_size(230.0)
            .frame(egui::Frame::new().fill(p.side).inner_margin(egui::Margin::symmetric(14, 18)).stroke(egui::Stroke::new(1.0, p.border)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Image::new((logo, egui::vec2(30.0, 30.0))));
                    ui.label(RichText::new("AccountingCraft").font(ui::semibold(18.0)));
                });
                ui.add_space(14.0);
                egui::Panel::bottom("nav-foot").frame(egui::Frame::new().inner_margin(egui::Margin { top: 10, ..Default::default() })).show_separator_line(false).show(ui, |ui| {
                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        egui::widgets::global_theme_preference_buttons(ui);
                        ui::label(ui, "Appearance");
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui.small_button("Open…").on_hover_text("Open another books file").clicked() {
                                open_path = rfd::FileDialog::new().add_filter("Books", &["db"]).pick_file();
                            }
                            if ui.small_button("New…").on_hover_text("Start a new books file").clicked() {
                                open_path = rfd::FileDialog::new().set_file_name("my-business.db").add_filter("Books", &["db"]).save_file();
                            }
                        });
                        let name = std::path::Path::new(&self.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                        ui.label(RichText::new(format!("Books: {name}")).small().color(p.weak)).on_hover_text(&self.path);
                        ui.horizontal(|ui| {
                            if ui.small_button("🔒 Lock").on_hover_text("Lock the books now. They lock by themselves after 15 minutes idle.").clicked() {
                                self.lock_now = true;
                            }
                            ui.label(RichText::new(format!("{} · {}", self.user, self.role.label())).small().color(p.weak));
                        });
                    });
                });
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    // Groups ("" = a top-level item). Banking and Setup start closed; open/closed is remembered while the app runs.
                    const NAV: [(&str, &[(Screen, &str)]); 7] = [
                        ("", &[(Screen::Home, "Home")]),
                        ("Sales", &[(Screen::Invoices, "Quotes & Invoices"), (Screen::Jobs, "Jobs & Time"), (Screen::Proposals, "Proposals")]),
                        ("Spending", &[(Screen::Purchases, "Purchase Orders"), (Screen::Mileage, "Mileage"), (Screen::Assets, "Assets")]),
                        ("Banking", &[(Screen::Transactions, "Transactions"), (Screen::Import, "Import & Rules"), (Screen::Reconcile, "Reconcile")]),
                        ("", &[(Screen::Reports, "Reports")]),
                        ("Setup", &[(Screen::People, "Customers & Vendors"), (Screen::Accounts, "Chart of Accounts"), (Screen::Business, "Business & Settings")]),
                        ("", &[(Screen::Help, "Help & Tutorials")]),
                    ];
                    for (group, items) in NAV {
                        let items: Vec<_> = items.iter().filter(|(s, _)| *s != Screen::Business || self.role == lock::Role::Owner).collect();
                        let mut indent = 0.0;
                        if !group.is_empty() {
                            let id = ui.scope_id().with(("nav-open", group));
                            let here = items.iter().any(|(s, _)| *s == self.screen);
                            let open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(!matches!(group, "Banking" | "Setup"));
                            if ui::nav_group(ui, group, open, here && !open) {
                                ui.data_mut(|d| d.insert_temp(id, !open));
                            }
                            if !open {
                                continue;
                            }
                            indent = 14.0;
                        }
                        for (s, label) in items {
                            if ui::nav_item(ui, label, self.screen == *s, indent) {
                                self.screen = *s;
                            }
                        }
                    }
                });
            });
        if let Some(path) = open_path {
            self.open_books(path.display().to_string());
            return;
        }
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin::symmetric(32, 26)))
            .show(ui, |ui| {
                if ui.input(|i| i.key_pressed(egui::Key::F1)) {
                    self.help_topic = Some(screen_key(self.screen));
                }
                if self.screen != Screen::Help {
                    // Floats over the top right of the content without pushing the page down.
                    let pos = ui.max_rect().right_top() + egui::vec2(-100.0, 0.0);
                    egui::Area::new(egui::Id::unique("helpbtn")).fixed_pos(pos).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
                        if ui.button("? Help").on_hover_text("Help for this screen (F1)").clicked() {
                            self.help_topic = Some(screen_key(self.screen));
                        }
                    });
                }
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| match self.screen {
                    Screen::Home => self.home(ui),
                    Screen::Transactions => self.transactions(ui),
                    Screen::Reports => self.reports(ui),
                    Screen::Invoices => self.invoices_ui(ui),
                    Screen::Jobs => self.jobs_ui(ui),
                    Screen::People => self.people(ui),
                    Screen::Reconcile => self.reconcile(ui),
                    Screen::Mileage => self.mileage(ui),
                    Screen::Assets => self.assets_ui(ui),
                    Screen::Purchases => self.purchases(ui),
                    Screen::Proposals => self.proposals_ui(ui),
                    Screen::Help => self.help_ui(ui),
                    Screen::Import => self.import(ui),
                    Screen::Accounts => self.accounts_ui(ui),
                    Screen::Business => self.business(ui),
                });
            });
        self.overlays(&ctx);
    }
}

impl App {
    /// The help pop-up and the tutorial panel, drawn over whatever screen is showing.
    fn overlays(&mut self, ctx: &egui::Context) {
        if let Some(key) = self.help_topic {
            let t = help::topic(key);
            let mut open = true;
            let mut start = None;
            egui::Window::new(format!("Help: {}", t.title))
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .default_width(560.0)
                .default_height(520.0)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.label(RichText::new(t.intro).size(16.0));
                        for (heading, lines) in t.sections {
                            ui.add_space(10.0);
                            ui.label(RichText::new(*heading).font(ui::semibold(16.0)).color(ACCENT));
                            for l in *lines {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label("•");
                                    ui.label(*l);
                                });
                            }
                        }
                        let tours = help::tutorials_for(key);
                        if !tours.is_empty() {
                            ui.add_space(12.0);
                            ui.label(RichText::new("Quick tutorials").font(ui::semibold(16.0)).color(ACCENT));
                            for i in tours {
                                if ui.link(format!("→ {}", help::TUTORIALS[i].title)).clicked() {
                                    start = Some(i);
                                }
                            }
                        }
                    });
                });
            if !open {
                self.help_topic = None;
            }
            if let Some(i) = start {
                self.start_tour(i);
            }
        }
        if let Some((ti, si)) = self.tour {
            let t = &help::TUTORIALS[ti];
            let step = &t.steps[si];
            let (mut back, mut next, mut quit) = (false, false, false);
            let p = ui::pal_ctx(ctx);
            egui::Window::new("tour")
                .title_bar(false)
                .resizable(false)
                .anchor(egui::Align2::RIGHT_BOTTOM, [-24.0, -24.0])
                .fixed_size([380.0, 0.0])
                .frame(egui::Frame::new().fill(p.card).stroke(egui::Stroke::new(2.0, ACCENT)).corner_radius(12.0).inner_margin(18.0))
                .show(ctx, |ui| {
                    ui.label(RichText::new(format!("{} · step {} of {}", t.title, si + 1, t.steps.len())).small().color(p.weak));
                    ui.label(RichText::new(step.title).font(ui::semibold(18.0)));
                    ui.label(step.text);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        back = si > 0 && ui.button("← Back").clicked();
                        let last = si + 1 == t.steps.len();
                        next = ui::primary(ui, if last { "Done ✔" } else { "Next →" });
                        ui.add_space(16.0);
                        quit = ui.small_button("Exit tour").clicked();
                    });
                });
            if quit || (next && si + 1 == t.steps.len()) {
                self.tour = None;
                if !quit {
                    self.status = format!("Finished \"{}\". Press F1 on any screen for help.", t.title);
                }
            } else if next || back {
                let si = if next { si + 1 } else { si - 1 };
                self.tour = Some((ti, si));
                self.screen = screen_of(t.steps[si].screen);
            }
        }
    }

    fn start_tour(&mut self, i: usize) {
        self.tour = Some((i, 0));
        self.help_topic = None;
        self.screen = screen_of(help::TUTORIALS[i].steps[0].screen);
    }

    fn help_ui(&mut self, ui: &mut egui::Ui) {
        ui::page(ui, "Help & Tutorials", "Short guided tours, and help for every screen. Press F1 anywhere for help on that screen.");
        let mut start = None;
        ui::card(ui, "Quick tutorials", |ui| {
            egui::Grid::new("tours").num_columns(3).spacing([20.0, 12.0]).show(ui, |ui| {
                for (i, t) in help::TUTORIALS.iter().enumerate() {
                    ui.label(RichText::new(t.title).font(ui::semibold(16.0)));
                    ui::label(ui, &format!("{} ({} steps)", t.blurb, t.steps.len()));
                    if ui::primary(ui, "Start") {
                        start = Some(i);
                    }
                    ui.end_row();
                }
            });
        });
        let mut show = None;
        ui::card(ui, "Help for each screen", |ui| {
            ui.horizontal_wrapped(|ui| {
                for (_, key) in SCREENS.iter().filter(|s| s.0 != Screen::Help) {
                    if ui.button(help::topic(key).title).clicked() {
                        show = Some(*key);
                    }
                }
            });
        });
        ui::card(ui, "About AccountingCraft", |ui| {
            ui::label(ui, &format!("Version {}. Free and open source under the MIT or Apache 2.0 license, at your choice.", env!("CARGO_PKG_VERSION")));
            ui.hyperlink_to("Source code, releases and issues", env!("CARGO_PKG_REPOSITORY"));
        });
        if let Some(i) = start {
            self.start_tour(i);
        }
        if show.is_some() {
            self.help_topic = show;
        }
    }

    fn home(&mut self, ui: &mut egui::Ui) {
        let t = today();
        let (now, month, year) = (t.to_string(), t.with_day(1).unwrap_or(t).to_string(), format!("{}-01-01", t.year()));
        let r = (|| Ok::<_, rusqlite::Error>((
            self.books.balance_sheet(&now)?,
            self.books.profit_and_loss(&month, &now)?,
            self.books.profit_and_loss(&year, &now)?,
        )))();
        let (bs, pl_m, pl_y) = match r {
            Ok(x) => x,
            Err(e) => {
                ui.label(format!("Error: {e}"));
                return;
            }
        };
        let sum = |f: &dyn Fn(&(String, String, i64)) -> bool| bs.rows.iter().filter(|r| f(r)).map(|r| r.2).sum::<i64>();
        let ar_name = self.accounts.iter().find(|a| a.system.as_deref() == Some("ar")).map_or("", |a| a.name.as_str());
        let receivable = sum(&|r| r.1 == ar_name);
        let cash = sum(&|r| r.0 == "asset") - receivable;
        let owe = sum(&|r| r.0 == "liability");
        let unpaid: Vec<&Invoice> = self.invoices.iter().filter(|i| !i.paid).collect();
        let overdue = unpaid.iter().filter(|i| i.overdue(&now)).count();

        let title = if self.settings.name.is_empty() { "Welcome back" } else { &self.settings.name };
        ui::page(ui, title, "Here's where your business stands today.");
        let n_unpaid = unpaid.len();
        let note = match overdue {
            0 => format!("{n_unpaid} unpaid invoice{}", if n_unpaid == 1 { "" } else { "s" }),
            n => format!("{n} overdue"),
        };
        ui::stat_tiles(
            ui,
            &[
                ("Cash in the bank", fmt_cents(cash), String::new()),
                ("Customers owe you", fmt_cents(receivable), note),
                ("You owe", fmt_cents(owe), "Cards, loans, deposits held".into()),
                ("Profit this month", fmt_cents(net_income(&pl_m)), String::new()),
                ("Profit this year", fmt_cents(net_income(&pl_y)), String::new()),
            ],
        );

        let uncat = self.uncategorized_ids();
        let n_uncat = self.entries.iter().filter(|e| e.lines.iter().any(|l| uncat.contains(&l.1))).count();
        let mut tour = false;
        ui::card(ui, "To do", |ui| {
            let mut todo = 0;
            if self.entries.is_empty() {
                todo += 1;
                if ui.link(RichText::new("New here? Take the 5-minute setup tour →").font(ui::semibold(15.0))).clicked() {
                    tour = true;
                }
                ui.horizontal(|ui| {
                    ui.label("No transactions yet.");
                    if ui.link("Record one").clicked() {
                        self.screen = Screen::Transactions;
                    }
                    ui.label("or");
                    if ui.link("import from your bank").clicked() {
                        self.screen = Screen::Import;
                    }
                });
            }
            if n_uncat > 0 {
                todo += 1;
                if ui.link(format!("{n_uncat} transaction(s) need a category")).clicked() {
                    (self.screen, self.reg_uncat) = (Screen::Transactions, true);
                }
            }
            if overdue > 0 {
                todo += 1;
                if ui.link(RichText::new(format!("{overdue} invoice(s) overdue: send a reminder")).color(MONEY_OUT)).clicked() {
                    self.screen = Screen::Invoices;
                }
            } else if !unpaid.is_empty() {
                todo += 1;
                let total: i64 = unpaid.iter().map(|i| i.balance()).sum();
                if ui.link(format!("{} unpaid invoice(s), {} in all", unpaid.len(), fmt_cents(total))).clicked() {
                    self.screen = Screen::Invoices;
                }
            }
            let owed: Vec<&purchasing::Po> = self.pos.iter().filter(|p| p.owed).collect();
            if !owed.is_empty() {
                todo += 1;
                let total: i64 = owed.iter().map(|p| p.total).sum();
                if ui.link(format!("{} vendor bill(s) to pay, {} in all", owed.len(), fmt_cents(total))).clicked() {
                    self.screen = Screen::Purchases;
                }
            }
            if !self.unbilled.is_empty() {
                todo += 1;
                let total: i64 = self.unbilled.iter().map(|t| t.cents()).sum();
                if ui.link(format!("{} of unbilled time", fmt_cents(total))).clicked() {
                    self.screen = Screen::Jobs;
                }
            }
            if self.settings.name.is_empty() || self.settings.logo.is_none() {
                todo += 1;
                if ui.link("Add your business name and logo for invoices").clicked() {
                    self.screen = Screen::Business;
                }
            }
            if todo == 0 {
                ui.label(RichText::new("All caught up ✔").color(MONEY_IN));
            }
        });
        if tour {
            self.start_tour(0);
        }
        let monthly = self.monthly.clone();
        ui::card(ui, "Last 12 months", |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("■ Income").color(MONEY_IN));
                ui.label(RichText::new("■ Expenses").color(MONEY_OUT));
            });
            month_chart(ui, &monthly);
        });
    }

    fn transactions(&mut self, ui: &mut egui::Ui) {
        ui::page(ui, "Transactions", "Record money in or out, then categorize and tag it in the register.");
        ui::card(ui, "Record money in, out, or a transfer", |ui| {
            egui::Grid::new("tform").num_columns(4).spacing([16.0, 10.0]).show(ui, |ui| {
                ui::label(ui, "Type");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.t_dir, Direction::Out, "Money out");
                    ui.selectable_value(&mut self.t_dir, Direction::In, "Money in");
                    ui.selectable_value(&mut self.t_dir, Direction::Transfer, "Transfer")
                        .on_hover_text("Move money between your accounts, or pay a credit card");
                });
                ui::label(ui, "Date");
                date_picker(ui, "tdate", &mut self.t_date);
                ui.end_row();
                ui::label(ui, "Description");
                field(ui, &mut self.t_memo, 260.0, "e.g. Lumber for deck");
                ui::label(ui, "Amount");
                field(ui, &mut self.t_amount, 140.0, "0.00");
                ui.end_row();
                if self.t_dir == Direction::Transfer {
                    ui::label(ui, "From");
                    account_combo(ui, "tbank", &self.accounts, BANKS, &mut self.t_bank);
                    ui::label(ui, "To");
                    account_combo(ui, "tto", &self.accounts, BANKS, &mut self.t_to);
                    ui.end_row();
                } else {
                    ui::label(ui, "Bank / card");
                    account_combo(ui, "tbank", &self.accounts, BANKS, &mut self.t_bank);
                    ui::label(ui, "Category");
                    ui.horizontal(|ui| {
                        account_combo(ui, "tcat", &self.accounts, &["income", "expense", "equity", "liability"], &mut self.t_cat);
                        ui.toggle_value(&mut self.t_new_cat, "+ New category");
                    });
                    ui.end_row();
                    ui::label(ui, "Vendor");
                    ui.horizontal(|ui| {
                        pick(ui, "tvendor", self.vendors.iter().map(|v| (v.id, v.name.as_str())), &mut self.t_vendor, "(none)");
                        ui.toggle_value(&mut self.t_new_vendor, "+ New vendor");
                    });
                    ui::label(ui, "Job");
                    ui.horizontal(|ui| {
                        pick(ui, "tjob", self.jobs.iter().map(|j| (j.id, j.name.as_str())), &mut self.t_job, "(none)");
                        ui.toggle_value(&mut self.t_new_job, "+ New job");
                    });
                    ui.end_row();
                }
                ui::label(ui, "Repeat");
                repeat_combo(ui, "trepeat", &mut self.t_repeat);
                ui.end_row();
            });
            let transfer = self.t_dir == Direction::Transfer;
            if self.t_new_cat && !transfer {
                let kind = if self.t_dir == Direction::In { "income" } else { "expense" };
                let add = ui
                    .horizontal(|ui| {
                        field(ui, &mut self.t_cat_name, 260.0, if kind == "income" { "Income category name" } else { "Expense category name" });
                        ui::primary(ui, "Add category")
                    })
                    .inner;
                if add {
                    let r = self.books.add_account("", &self.t_cat_name, kind, "");
                    if let Ok(id) = &r {
                        self.t_cat = *id;
                    }
                    if self.done(r, "Category added and picked. Number it later in Chart of Accounts if you like.") {
                        (self.t_cat_name, self.t_new_cat) = (String::new(), false);
                    }
                }
            }
            if self.t_new_vendor && !transfer {
                let add = ui
                    .horizontal(|ui| {
                        field(ui, &mut self.v_name, 260.0, "Vendor name");
                        field(ui, &mut self.v_email, 240.0, "Email (optional)");
                        ui::primary(ui, "Add vendor")
                    })
                    .inner;
                if add {
                    let r = self.books.add_vendor(&self.v_name, &self.v_email);
                    if let Ok(id) = &r {
                        self.t_vendor = Some(*id);
                    }
                    if self.done(r, "Vendor added and picked.") {
                        (self.v_name, self.v_email, self.t_new_vendor) = (String::new(), String::new(), false);
                    }
                }
            }
            if self.t_new_job && !transfer {
                let add = ui
                    .horizontal(|ui| {
                        field(ui, &mut self.j_name, 260.0, "Job name");
                        pick(ui, "tjobcust", self.customers.iter().map(|c| (c.id, c.name.as_str())), &mut self.j_cust, "Customer (optional)");
                        ui.toggle_value(&mut self.t_new_jobcust, "+ New customer");
                        ui::primary(ui, "Add job")
                    })
                    .inner;
                if self.t_new_jobcust {
                    let add_c = ui
                        .horizontal(|ui| {
                            field(ui, &mut self.c_name, 260.0, "Customer name");
                            field(ui, &mut self.c_email, 240.0, "Email (optional)");
                            ui::primary(ui, "Add customer")
                        })
                        .inner;
                    if add_c {
                        let r = self.books.add_customer(&self.c_name, &self.c_email);
                        if let Ok(id) = &r {
                            self.j_cust = Some(*id);
                        }
                        if self.done(r, "Customer added and picked for the new job.") {
                            (self.c_name, self.c_email, self.t_new_jobcust) = (String::new(), String::new(), false);
                        }
                    }
                }
                if add {
                    let r = self.books.add_job(&self.j_name, self.j_cust);
                    if let Ok(id) = &r {
                        self.t_job = Some(*id);
                    }
                    if self.done(r, "Job added and picked.") {
                        (self.j_name, self.t_new_job) = (String::new(), false);
                    }
                }
            }
            ui.add_space(6.0);
            if ui::primary(ui, "Record transaction") {
                self.record();
            }
        });
        self.register(ui);
        let mut stop = None;
        if !self.recurring.is_empty() {
            ui::card(ui, "Recurring", |ui| {
                ui::label(ui, "Posted automatically on their dates when you open the app.");
                egui::Grid::new("recur").striped(true).num_columns(4).spacing([24.0, 6.0]).show(ui, |ui| {
                    for h in ["NEXT", "EVERY", "WHAT", ""] {
                        ui::th(ui, h);
                    }
                    ui.end_row();
                    for r in &self.recurring {
                        ui.label(nice_dates(&r.next_date));
                        ui.label(&r.every);
                        ui.label(&r.summary);
                        if ui.small_button("Stop").clicked() {
                            stop = Some(r.id);
                        }
                        ui.end_row();
                    }
                });
            });
        }
        if let Some(id) = stop {
            let r = self.books.delete_recurring(id);
            self.done(r, "Stopped. Past occurrences stay in your books.");
        }
    }

    fn record(&mut self) {
        let transfer = self.t_dir == Direction::Transfer;
        let (bank, cat) = if transfer { (self.t_to, self.t_bank) } else { (self.t_bank, self.t_cat) };
        let r = match parse_cents(&self.t_amount) {
            None | Some(0) => Err("enter an amount like 12.34".to_string()),
            _ if bank == 0 || cat == 0 || bank == cat => Err(if transfer {
                "choose two different accounts".to_string()
            } else {
                "choose a bank/card and a different category".to_string()
            }),
            Some(c) => {
                // A transfer posts money arriving in "To" (first line) and leaving "From".
                let c = if self.t_dir == Direction::Out { -c.abs() } else { c.abs() };
                let (job, vendor) = if transfer { (None, None) } else { (self.t_job, self.t_vendor) };
                let d = self.t_date.to_string();
                let memo = if transfer && self.t_memo.trim().is_empty() { "Transfer".to_string() } else { self.t_memo.trim().to_string() };
                self.books
                    .post_simple(&d, &memo, bank, cat, c)
                    .and_then(|e| self.books.set_tags(e, job, vendor))
                    .and_then(|_| match self.t_repeat {
                        "" => Ok(()),
                        every => self.books.add_recurring_txn(every, &d, &memo, bank, cat, c, job, vendor),
                    })
                    .map_err(|e| e.to_string())
            }
        };
        let msg = if self.t_repeat.is_empty() { "Recorded." } else { "Recorded, and set to repeat." };
        if self.done(r, msg) {
            self.t_memo.clear();
            self.t_amount.clear();
            self.t_repeat = "";
        }
    }

    fn register(&mut self, ui: &mut egui::Ui) {
        let uncat = self.uncategorized_ids();
        let q = self.reg_search.trim().to_lowercase();
        let mut changes = vec![];
        let mut tags = vec![];
        let (mut edit, mut receipt, mut bulk) = (None, None, false);
        let mut shown_ids = vec![];
        ui::card(ui, "Register", |ui| {
            if self.entries.is_empty() {
                ui::label(ui, "No transactions yet. Record one above, or bring them in from Import & Rules.");
                return;
            }
            ui.horizontal(|ui| {
                field(ui, &mut self.reg_search, 240.0, "Search descriptions");
                ui.checkbox(&mut self.reg_uncat, "Only uncategorized");
                ui::label(ui, "Job");
                pick(ui, "regjob", self.jobs.iter().map(|j| (j.id, j.name.as_str())), &mut self.reg_job, "(all)");
            });
            ui.horizontal(|ui| {
                ui::label(ui, "Put everything shown on job");
                pick(ui, "bulkjob", self.jobs.iter().map(|j| (j.id, j.name.as_str())), &mut self.reg_bulk_job, "Choose a job");
                if self.reg_bulk_job.is_some() && ui.button("Apply").on_hover_text("Tags every row matching the search and filters above").clicked() {
                    bulk = true;
                }
            });
            ui.add_space(6.0);
            let name = |id: i64| self.accounts.iter().find(|a| a.id == id).map_or("?", |a| a.name.as_str());
            let mut shown = 0;
            // Wide table: scroll sideways inside the card instead of overflowing it.
            egui::ScrollArea::horizontal().id_salt("register").show(ui, |ui| {
                ui.spacing_mut().combo_width = 180.0; // narrower pickers so the row fits beside its buttons
                egui::Grid::new("register").striped(true).num_columns(9).spacing([10.0, 8.0]).show(ui, |ui| {
                    for h in ["DATE", "DESCRIPTION", "ACCOUNT", "CATEGORY", "JOB", "VENDOR"] {
                        ui::th(ui, h);
                    }
                    th_num(ui, "AMOUNT");
                    ui.label("");
                    ui.label("");
                    ui.end_row();
                    for e in &self.entries {
                        // The first line is the bank/card (or A/R) side, as every posting path writes it first.
                        let Some(&(_, acct, cents)) = e.lines.first() else { continue };
                        if !e.memo.to_lowercase().contains(&q)
                            || (self.reg_uncat && !e.lines.iter().any(|l| uncat.contains(&l.1)))
                            || (self.reg_job.is_some() && e.job != self.reg_job)
                        {
                            continue;
                        }
                        shown += 1;
                        shown_ids.push((e.id, e.vendor));
                        ui.label(nice_dates(&e.date));
                        ui.horizontal(|ui| {
                            ui.label(&e.memo);
                            if e.receipt && ui.small_button("📎").on_hover_text("Open the receipt").clicked() {
                                receipt = Some(e.id);
                            }
                        });
                        ui::label(ui, name(acct));
                        ui.vertical(|ui| {
                            for &(line, a, _) in &e.lines[1..] {
                                let mut sel = a;
                                if account_combo(ui, ("line", line), &self.accounts, &KINDS, &mut sel) {
                                    changes.push((line, sel));
                                }
                            }
                        });
                        let (mut job, mut vendor) = (e.job, e.vendor);
                        let jc = pick(ui, ("ejob", e.id), self.jobs.iter().map(|j| (j.id, j.name.as_str())), &mut job, "—");
                        let vc = pick(ui, ("evend", e.id), self.vendors.iter().map(|v| (v.id, v.name.as_str())), &mut vendor, "—");
                        if jc || vc {
                            tags.push((e.id, job, vendor));
                        }
                        amount_cell(ui, money(cents));
                        if e.lines.len() == 2 && !e.invoice {
                            if ui.small_button("Edit").clicked() {
                                edit = Some(e.id);
                            }
                        } else {
                            ui.label("");
                        }
                        if ui.small_button("✕").on_hover_text("Delete").clicked() {
                            self.confirm_delete = Some(e.id);
                        }
                        ui.end_row();
                    }
                });
            });
            if shown == 0 {
                ui::label(ui, "Nothing matches. Clear the search or the filters.");
            }
        });
        for (line, acct) in changes {
            let r = self.books.set_line_account(line, acct);
            self.done(r, "Recategorized.");
        }
        for (id, job, vendor) in tags {
            let r = self.books.set_tags(id, job, vendor);
            self.done(r, "Saved.");
        }
        if bulk {
            // Locked or invoice-owned rows are skipped rather than stopping the rest.
            let n = shown_ids.iter().filter(|(id, vendor)| self.books.set_tags(*id, self.reg_bulk_job, *vendor).is_ok()).count();
            self.done(Ok::<_, String>(()), &format!("Put {n} transaction(s) on the job."));
        }
        if let Some(id) = receipt {
            self.open_receipt(id);
        }
        if let Some(e) = edit.and_then(|id| self.entries.iter().find(|e| e.id == id)) {
            let cents = e.lines[0].2;
            self.edit = Some(EditTx {
                id: e.id,
                date: e.date.parse().unwrap_or(today()),
                memo: e.memo.clone(),
                amount: plain(cents.abs()),
                money_in: cents > 0,
                bank: e.lines[0].1,
                receipt: e.receipt,
            });
        }
        self.edit_dialog(ui);
        if let Some(id) = self.confirm_delete {
            let memo = self.entries.iter().find(|e| e.id == id).map_or(String::new(), |e| format!("{} {}", nice_dates(&e.date), e.memo));
            let m = egui::Modal::new(egui::Id::unique("confirm_delete")).show(ui.ctx(), |ui| {
                ui.label(RichText::new("Delete this transaction?").font(ui::semibold(18.0)));
                ui.label(memo);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let yes = ui.add(egui::Button::new(RichText::new("Delete").color(egui::Color32::WHITE)).fill(MONEY_OUT)).clicked();
                    (yes, ui.button("Cancel").clicked())
                })
                .inner
            });
            let (yes, no) = m.inner;
            if yes {
                let r = self.books.delete_entry(id);
                self.done(r, "Deleted.");
            }
            if yes || no || m.should_close() {
                self.confirm_delete = None;
            }
        }
    }

    fn edit_dialog(&mut self, ui: &mut egui::Ui) {
        let Some(ed) = self.edit.as_mut() else { return };
        let accounts = &self.accounts;
        let (mut save, mut cancel, mut attach, mut open, mut remove) = (false, false, false, false, false);
        let m = egui::Modal::new(egui::Id::unique("edit_tx")).show(ui.ctx(), |ui| {
            ui.label(RichText::new("Edit transaction").font(ui::semibold(18.0)));
            ui.add_space(6.0);
            egui::Grid::new("editgrid").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                ui::label(ui, "Date");
                date_picker(ui, "edate", &mut ed.date);
                ui.end_row();
                ui::label(ui, "Description");
                field(ui, &mut ed.memo, 300.0, "");
                ui.end_row();
                ui::label(ui, "Amount");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut ed.money_in, false, "Out");
                    ui.selectable_value(&mut ed.money_in, true, "In");
                    field(ui, &mut ed.amount, 120.0, "0.00");
                });
                ui.end_row();
                ui::label(ui, "Account");
                account_combo(ui, "ebank", accounts, BANKS, &mut ed.bank);
                ui.end_row();
                ui::label(ui, "Receipt");
                ui.horizontal(|ui| {
                    if ed.receipt {
                        open = ui.button("Open").clicked();
                        attach = ui.button("Replace…").clicked();
                        remove = ui.button("Remove").clicked();
                    } else {
                        attach = ui.button("Attach receipt…").clicked();
                    }
                });
                ui.end_row();
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                save = ui::primary(ui, "Save");
                cancel = ui.button("Cancel").clicked();
            });
        });
        let (id, receipt) = (ed.id, ed.receipt);
        if save {
            let r = match parse_cents(&ed.amount) {
                Some(c) if c != 0 => {
                    let c = if ed.money_in { c.abs() } else { -c.abs() };
                    self.books.update_simple(id, &ed.date.to_string(), &ed.memo, ed.bank, c).map_err(|e| e.to_string())
                }
                _ => Err("enter an amount like 12.34".to_string()),
            };
            if self.done(r, "Saved.") {
                self.edit = None;
            }
        } else if cancel || m.should_close() {
            self.edit = None;
        }
        if attach
            && let Some(p) = rfd::FileDialog::new().add_filter("Receipts", &["pdf", "png", "jpg", "jpeg", "heic", "webp"]).pick_file() {
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or("receipt".into());
                let r = std::fs::read(&p).map_err(|e| e.to_string()).and_then(|b| self.books.attach(id, &name, &b).map_err(|e| e.to_string()));
                if self.done(r, "Receipt attached.")
                    && let Some(ed) = self.edit.as_mut() {
                        ed.receipt = true;
                    }
            }
        if open && receipt {
            self.open_receipt(id);
        }
        if remove {
            let r = self.books.remove_attachment(id);
            if self.done(r, "Receipt removed.")
                && let Some(ed) = self.edit.as_mut() {
                    ed.receipt = false;
                }
        }
    }

    fn reports(&mut self, ui: &mut egui::Ui) {
        ui::page(ui, "Reports", "Pick a report and a date range. Export to a spreadsheet or print / save as PDF.");
        let (mut csv, mut print, mut save_pdf, mut make_pkg) = (false, false, false, false);
        ui::card(ui, "", |ui| {
            ui.horizontal_wrapped(|ui| {
                let name = reports::ALL.iter().find(|k| k.0 == self.r_kind).map_or("", |k| k.1);
                egui::ComboBox::from_id_salt("rkind").selected_text(name).width(260.0).show_ui(ui, |ui| {
                    for (k, n) in reports::ALL {
                        ui.selectable_value(&mut self.r_kind, k, n);
                    }
                });
                if !self.r_kind.as_of() && self.r_kind != reports::Kind::EstimatedTax {
                    ui::label(ui, "From");
                    date_picker(ui, "rfrom", &mut self.r_from);
                }
                ui::label(ui, match self.r_kind {
                    reports::Kind::EstimatedTax => "Year of",
                    k if k.as_of() => "As of",
                    _ => "To",
                });
                date_picker(ui, "rto", &mut self.r_to);
                match self.r_kind {
                    reports::Kind::GeneralLedger => {
                        let items = self.accounts.iter().filter(|a| a.used > 0).map(|a| (a.id, a.name.as_str()));
                        pick(ui, "racct", items, &mut self.r_account, "All accounts");
                    }
                    reports::Kind::JobProfit => {
                        pick(ui, "rjob", self.jobs.iter().map(|j| (j.id, j.name.as_str())), &mut self.r_job, "All jobs");
                    }
                    _ => {}
                }
                ui.add_space(8.0);
                ui.toggle_value(&mut self.r_detail, "Show detail")
                    .on_hover_text("List the transactions, invoices or accounts behind each line");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.toggle_value(&mut self.r_pkg, "Accountant package…")
                        .on_hover_text("Every report, the full journal and QuickBooks Online import files in one folder for your CPA");
                    print = ui.button("Print").clicked();
                    save_pdf = ui.button("Save PDF").clicked();
                    csv = ui.button("Export CSV").clicked();
                });
            });
        });
        if self.r_pkg {
            ui::card(ui, "Accountant package", |ui| {
                ui::label(ui, "One folder for your CPA: all reports as a PDF and CSVs, every journal line, QuickBooks Online import files, and a read-me saying how finished the period is.");
                ui.horizontal_wrapped(|ui| {
                    ui::label(ui, "Period");
                    let y = self.r_to.year();
                    for (label, a, z) in [("Q1", (1, 1), (3, 31)), ("Q2", (4, 1), (6, 30)), ("Q3", (7, 1), (9, 30)), ("Q4", (10, 1), (12, 31)), ("Full year", (1, 1), (12, 31))] {
                        let (f, t) = (Date::from_ymd_opt(y, a.0, a.1).unwrap_or(self.r_from), Date::from_ymd_opt(y, z.0, z.1).unwrap_or(self.r_to));
                        if ui.add(egui::Button::selectable(self.r_from == f && self.r_to == t, format!("{label} {y}"))).clicked() {
                            (self.r_from, self.r_to) = (f, t);
                        }
                    }
                    ui::label(ui, &format!("{} to {} (or set From / To above)", self.r_from, self.r_to));
                });
                match package::issues(&self.books, &self.r_from.to_string(), &self.r_to.to_string()) {
                    Ok(list) if list.is_empty() => drop(ui.label(RichText::new("✔ Ready: categorized, reconciled and locked.").color(MONEY_IN))),
                    Ok(list) => {
                        ui.label(RichText::new("Before you send it (or send it anyway; the read-me lists these):").color(MONEY_OUT));
                        for i in list {
                            ui.label(format!("  •  {i}"));
                        }
                    }
                    Err(e) => drop(ui.label(format!("Error: {e}"))),
                }
                ui.add_space(6.0);
                make_pkg = ui::primary(ui, "Make package…");
            });
        }
        // shortcut: re-runs the report SQL every frame; cache it if large books feel slow here.
        let (from, to) = (self.r_from.to_string(), self.r_to.to_string());
        let table = match reports::run(&self.books, self.r_kind, &from, &to, self.r_account, self.r_job, self.r_detail) {
            Ok(t) => t,
            Err(e) => {
                ui.label(format!("Error: {e}"));
                return;
            }
        };
        ui::card(ui, &nice_dates(&table.title), |ui| {
            if table.rows.len() <= 1 {
                ui::label(ui, "Nothing in this period yet.");
            }
            egui::ScrollArea::horizontal().id_salt("reportscroll").show(ui, |ui| {
                egui::Grid::new(("report", table.title.as_str())).striped(true).num_columns(table.columns.len()).spacing([28.0, 6.0]).show(ui, |ui| {
                    for (i, c) in table.columns.iter().enumerate() {
                        if table.rows.iter().any(|r| matches!(r.cells.get(i), Some(reports::Cell::Money(_)))) {
                            th_num(ui, &c.to_uppercase());
                        } else {
                            ui::th(ui, &c.to_uppercase());
                        }
                    }
                    ui.end_row();
                    for r in &table.rows {
                        for c in &r.cells {
                            match c {
                                reports::Cell::Text(s) if r.strong => drop(ui.label(RichText::new(nice_dates(s)).font(ui::semibold(15.0)))),
                                reports::Cell::Text(s) => drop(ui.label(nice_dates(s))),
                                reports::Cell::Money(m) if r.strong => amount_cell(ui, RichText::new(fmt_cents(*m)).font(ui::semibold(15.0))),
                                reports::Cell::Money(m) => amount_cell(ui, fmt_cents(*m)),
                            }
                        }
                        ui.end_row();
                    }
                });
            });
        });
        if csv {
            let file = format!("{}.csv", table.title.replace([',', '/', ':', '(', ')'], ""));
            if let Some(path) = rfd::FileDialog::new().set_file_name(&file).add_filter("CSV", &["csv"]).save_file() {
                let r = reports::to_csv(&table).and_then(|s| std::fs::write(&path, s).map_err(|e| e.to_string()));
                self.done(r, &format!("Saved {}", path.display()));
            }
        }
        if print {
            let pdf = reports::to_pdf(std::slice::from_ref(&table), &self.settings.name);
            self.open_pdf("report", pdf);
        }
        if save_pdf {
            let file = format!("{}.pdf", table.title.replace([',', '/', ':', '(', ')'], ""));
            if let Some(path) = rfd::FileDialog::new().set_file_name(&file).add_filter("PDF", &["pdf"]).save_file() {
                let r = reports::to_pdf(std::slice::from_ref(&table), &self.settings.name).and_then(|pdf| std::fs::write(&path, pdf).map_err(|e| e.to_string()));
                self.done(r, &format!("Saved {}", path.display()));
            }
        }
        if make_pkg
            && let Some(parent) = rfd::FileDialog::new().set_title("Where should the package folder go?").pick_folder() {
                let r = package::write(&self.books, &parent, &self.r_from.to_string(), &self.r_to.to_string());
                if let Ok(dir) = &r {
                    let _ = open_external(&dir.display().to_string());
                }
                let msg = r.as_ref().map(|d| format!("Package saved to {}", d.display())).unwrap_or_default();
                self.done(r, &msg);
            }
    }

    fn doc_input(&self, items: Vec<Item>, taxable: Vec<bool>) -> Result<DocInput, String> {
        let tax_bp = parse_rate_bp(&self.i_tax).ok_or("sales tax should look like 8.25")?;
        let due = if self.i_terms > 0 { add_days(self.i_date, self.i_terms).to_string() } else { String::new() };
        Ok(DocInput { customer_id: self.i_cust, date: self.i_date.to_string(), items, job: self.i_job, tax_bp, due_date: due, taxable, customer_po: self.i_po.clone() })
    }

    fn invoices_ui(&mut self, ui: &mut egui::Ui) {
        let now = today().to_string();
        ui::page(ui, "Quotes & Invoices", "Quote a price, turn it into an invoice, then record payments as they arrive.");
        let open_quotes = self.quotes.iter().filter(|q| q.invoice_id.is_none()).count();
        let unpaid = self.invoices.iter().filter(|i| !i.paid).count();
        ui.horizontal(|ui| {
            for (tab, label) in [(1u8, format!("Quotes ({open_quotes} open)")), (0, format!("Invoices ({unpaid} unpaid)"))] {
                if ui.add(egui::Button::selectable(self.inv_tab == tab, RichText::new(label).size(16.0))).clicked() {
                    self.inv_tab = tab;
                }
            }
            ui.add_space(12.0);
            if ui::primary(ui, "+ New quote") {
                (self.inv_tab, self.i_quote) = (2, true);
            }
            if ui.button("+ New invoice").clicked() {
                (self.inv_tab, self.i_quote) = (2, false);
            }
        });
        ui.add_space(10.0);
        if self.inv_tab == 2 {
        let form_title = if self.i_quote { "New quote" } else { "New invoice" };
        ui::card(ui, form_title, |ui| {
            // A big two-way switch so it's always obvious which one you're making.
            ui.horizontal(|ui| {
                for (q, label, tip) in [
                    (true, "Quote", "A price for the customer to approve. Nothing goes in the books until you turn it into an invoice."),
                    (false, "Invoice", "A bill for work agreed or done. It goes in the books as money the customer owes you."),
                ] {
                    let on = self.i_quote == q;
                    let text = RichText::new(label).font(ui::semibold(17.0)).color(if on { egui::Color32::WHITE } else { ui::pal(ui).text });
                    let b = egui::Button::new(text).min_size(egui::vec2(150.0, 40.0)).fill(if on { ACCENT } else { ui::pal(ui).field });
                    if ui.add(b).on_hover_text(tip).clicked() {
                        self.i_quote = q;
                    }
                }
                ui.add_space(12.0);
                ui::label(ui, if self.i_quote { "A price to approve. Not in your books yet." } else { "A bill. Goes in your books as money owed to you." });
            });
            ui.add_space(8.0);
            egui::Grid::new("iform").num_columns(4).spacing([16.0, 10.0]).show(ui, |ui| {
                ui::label(ui, "Customer");
                let mut cust = (self.i_cust != 0).then_some(self.i_cust);
                ui.horizontal(|ui| {
                    pick(ui, "icust", self.customers.iter().map(|c| (c.id, c.name.as_str())), &mut cust, "Choose…");
                    ui.toggle_value(&mut self.inv_new_cust, "+ New customer");
                });
                self.i_cust = cust.unwrap_or(0);
                ui::label(ui, "Date");
                date_picker(ui, "idate", &mut self.i_date);
                ui.end_row();
                ui::label(ui, "Job");
                ui.horizontal(|ui| {
                    pick(ui, "ijob", self.jobs.iter().map(|j| (j.id, j.name.as_str())), &mut self.i_job, "No job yet");
                    ui.toggle_value(&mut self.inv_new_job, "+ New job");
                    if self.i_job.is_none() && self.i_quote {
                        ui.label(RichText::new("saving makes one").small().color(ui::pal(ui).weak))
                            .on_hover_text("With no job picked, saving the quote creates a job for it, so you can track its costs.");
                    }
                });
                ui::label(ui, "Customer PO #");
                field(ui, &mut self.i_po, 200.0, "if they gave you one");
                ui.end_row();
                ui::label(ui, "Sales tax %");
                field(ui, &mut self.i_tax, 90.0, "0");
                if !self.i_quote {
                    ui::label(ui, "Terms");
                    let t = TERMS.iter().find(|t| t.0 == self.i_terms).map_or("", |t| t.1);
                    egui::ComboBox::from_id_salt("iterms").selected_text(t).width(200.0).show_ui(ui, |ui| {
                        for (d, l) in TERMS {
                            ui.selectable_value(&mut self.i_terms, d, l);
                        }
                    });
                    ui.end_row();
                    ui::label(ui, "Repeat");
                    repeat_combo(ui, "irepeat", &mut self.i_repeat);
                }
                ui.end_row();
            });
            if self.inv_new_job {
                let add = ui
                    .horizontal(|ui| {
                        field(ui, &mut self.j_name, 300.0, "Job name");
                        ui::label(ui, "for the customer picked above");
                        ui::primary(ui, "Add job")
                    })
                    .inner;
                if add {
                    let cust = (self.i_cust != 0).then_some(self.i_cust);
                    let r = self.books.add_job(&self.j_name, cust);
                    if let Ok(id) = &r {
                        self.i_job = Some(*id);
                    }
                    if self.done(r, "Job added and picked for this invoice or quote.") {
                        (self.j_name, self.inv_new_job) = (String::new(), false);
                    }
                }
            }
            if self.inv_new_cust {
                let add = ui
                    .horizontal(|ui| {
                        field(ui, &mut self.c_name, 260.0, "Customer name");
                        field(ui, &mut self.c_email, 240.0, "Email");
                        ui::primary(ui, "Add customer")
                    })
                    .inner;
                if add {
                    let r = self.books.add_customer(&self.c_name, &self.c_email);
                    if let Ok(id) = &r {
                        self.i_cust = *id;
                    }
                    if self.done(r, "Customer added and picked for this invoice or quote.") {
                        (self.c_name, self.c_email, self.inv_new_cust) = (String::new(), String::new(), false);
                    }
                }
            }
            ui.add_space(8.0);
            let mut remove = None;
            egui::Grid::new("items").num_columns(4).spacing([12.0, 8.0]).show(ui, |ui| {
                ui::th(ui, "DESCRIPTION");
                ui::th(ui, "AMOUNT");
                ui::th(ui, "TAX");
                ui.end_row();
                for (i, (d, a, t)) in self.i_items.iter_mut().enumerate() {
                    field(ui, d, 420.0, "What you did or sold");
                    field(ui, a, 120.0, "0.00");
                    ui.checkbox(t, "").on_hover_text("Untick for lines that aren't taxable, like labor in many states");
                    if ui.small_button("✕").on_hover_text("Remove line").clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
            if let Some(i) = remove {
                self.i_items.remove(i);
            }
            // Blank rows are ignored; any other unparseable amount blocks saving.
            let kept: Vec<&(String, String, bool)> = self.i_items.iter().filter(|(d, a, _)| !(d.trim().is_empty() && a.trim().is_empty())).collect();
            let parsed: Option<Vec<Item>> = kept.iter().map(|(d, a, _)| parse_cents(a).map(|c| (d.trim().to_string(), c))).collect();
            let taxable: Vec<bool> = kept.iter().map(|i| i.2).collect();
            ui.horizontal(|ui| {
                if ui.button("+ Add line").clicked() {
                    self.i_items.push((String::new(), String::new(), true));
                }
                ui.add_space(12.0);
                match (&parsed, parse_rate_bp(&self.i_tax)) {
                    (Some(items), Some(bp)) => {
                        let sub: i64 = items.iter().map(|i| i.1).sum();
                        let taxed: i64 = items.iter().zip(&taxable).filter(|(_, t)| **t).map(|(i, _)| i.1).sum();
                        let tax = books::tax_cents(taxed, bp);
                        let text = if tax == 0 { format!("Total {}", fmt_cents(sub)) } else { format!("Subtotal {}  +  tax {}  =  {}", fmt_cents(sub), fmt_cents(tax), fmt_cents(sub + tax)) };
                        ui.label(RichText::new(text).font(ui::semibold(16.0)))
                    }
                    _ => ui.colored_label(MONEY_OUT, "Check the amounts and tax rate"),
                };
            });
            ui.add_space(6.0);
            let clicked = ui::primary(ui, if self.i_quote { "Save quote" } else { "Create invoice" });
            let (inv, quote) = (clicked && !self.i_quote, clicked && self.i_quote);
            if inv || quote {
                let r = match &parsed {
                    _ if self.i_cust == 0 => Err("choose a customer".to_string()),
                    None => Err("check the amounts".to_string()),
                    Some(items) => self.doc_input(items.clone(), taxable.clone()).and_then(|d| {
                        if quote {
                            return self.books.create_quote(&d).map(|_| ()).map_err(|e| e.to_string());
                        }
                        self.books.create_invoice(&d).map_err(|e| e.to_string())?;
                        match self.i_repeat {
                            "" => Ok(()),
                            every => self.books.add_recurring_invoice(every, &d, self.i_terms).map_err(|e| e.to_string()),
                        }
                    }),
                };
                let msg = if inv { "Invoice created and booked to Accounts Receivable." } else { "Quote saved with its job. It isn't in the books until converted." };
                if self.done(r, msg) {
                    self.i_items = vec![(String::new(), String::new(), true)];
                    self.i_repeat = "";
                    self.i_po.clear();
                    self.inv_tab = if inv { 0 } else { 1 };
                }
            }
        });
        }

        let (mut pay, mut view, mut mail, mut convert) = (None, None, None, None);
        let (mut deposit, mut apply) = (None, None);
        if self.inv_tab == 1 {
        ui::card(ui, "Quotes", |ui| {
            if self.quotes.is_empty() {
                ui::label(ui, "No quotes yet. Use \"+ New quote\" to send a price before the work is agreed.");
                return;
            }
            field(ui, &mut self.inv_q, 320.0, "Search by customer, job or number");
            ui.add_space(4.0);
            egui::Grid::new("quotes").striped(true).num_columns(9).spacing([16.0, 8.0]).show(ui, |ui| {
                for h in ["QUOTE", "CUSTOMER", "JOB", "DATE"] {
                    ui::th(ui, h);
                }
                th_num(ui, "BEFORE TAX");
                ui.end_row();
                for q in self.quotes.iter().filter(|q| matches(&format!("Q{} {} {}", q.id, q.customer, q.job.as_deref().unwrap_or("")), &self.inv_q)) {
                    ui.label(format!("Q{}", q.id));
                    ui.label(&q.customer);
                    ui::label(ui, q.job.as_deref().unwrap_or(""));
                    ui.label(nice_dates(&q.date));
                    amount_cell(ui, fmt_cents(q.cents));
                    if ui.button("View / print").clicked() {
                        view = Some((books::Doc::Quote, q.id));
                    }
                    if ui.button("Email").clicked() {
                        mail = Some((books::Doc::Quote, q.id, false));
                    }
                    if ui.button("Take deposit").on_hover_text("Money paid up front, held until the job is invoiced").clicked() {
                        deposit = Some(q.customer_id);
                    }
                    match q.invoice_id {
                        Some(inv) => drop(ui.label(RichText::new(format!("Invoiced as #{inv}")).color(MONEY_IN))),
                        None => {
                            if ui::primary(ui, "Convert to invoice") {
                                convert = Some(q.id);
                            }
                        }
                    }
                    ui.end_row();
                }
            });
        });
        }
        if self.inv_tab == 0 {
        ui::card(ui, "Invoices", |ui| {
            ui.horizontal_wrapped(|ui| {
                field(ui, &mut self.inv_q, 280.0, "Search by customer, number or description");
                for (f, label) in [(0u8, "All"), (1, "Unpaid"), (2, "Overdue"), (3, "Paid")] {
                    ui.selectable_value(&mut self.inv_filter, f, label);
                }
                ui.add_space(12.0);
                ui::label(ui, "Payments go into");
                account_combo(ui, "ibank", &self.accounts, BANKS, &mut self.i_bank);
            });
            if self.invoices.is_empty() {
                ui::label(ui, "No invoices yet.");
                return;
            }
            egui::ScrollArea::horizontal().id_salt("invscroll").show(ui, |ui| {
                egui::Grid::new("invoices").striped(true).num_columns(10).spacing([14.0, 8.0]).show(ui, |ui| {
                    for h in ["INVOICE", "CUSTOMER", "DATE", "DUE"] {
                        ui::th(ui, h);
                    }
                    th_num(ui, "TOTAL");
                    th_num(ui, "BALANCE");
                    ui.end_row();
                    let shown = self.invoices.iter().filter(|i| {
                        let status = match self.inv_filter {
                            1 => !i.paid,
                            2 => i.overdue(&now),
                            3 => i.paid,
                            _ => true,
                        };
                        status && matches(&format!("#{} {} {}", i.number, i.customer, i.description), &self.inv_q)
                    });
                    for i in shown {
                        ui.label(format!("#{}", i.number));
                        ui.label(&i.customer).on_hover_text(&i.description);
                        ui.label(nice_dates(&i.date));
                        if i.overdue(&now) {
                            ui.label(RichText::new(format!("{} overdue", nice_dates(&i.due_date))).color(MONEY_OUT));
                        } else {
                            ui::label(ui, &if i.due_date.is_empty() { "On receipt".to_string() } else { nice_dates(&i.due_date) });
                        }
                        amount_cell(ui, fmt_cents(i.cents));
                        if i.paid {
                            amount_cell(ui, RichText::new("Paid ✔").color(MONEY_IN));
                        } else {
                            amount_cell(ui, fmt_cents(i.balance()));
                        }
                        if ui.button("View / print").clicked() {
                            view = Some((books::Doc::Invoice, i.id));
                        }
                        if i.overdue(&now) {
                            if ui.button("Send reminder").clicked() {
                                mail = Some((books::Doc::Invoice, i.id, true));
                            }
                        } else if ui.button("Email").clicked() {
                            mail = Some((books::Doc::Invoice, i.id, false));
                        }
                        if !i.paid && ui::primary(ui, "Record payment") {
                            pay = Some((i.id, plain(i.balance())));
                        }
                        let held = if i.paid { 0 } else { self.books.deposit_available(i.customer_id).unwrap_or(0) };
                        if held > 0 && ui.button(format!("Apply {} deposit", fmt_cents(held.min(i.balance())))).clicked() {
                            apply = Some(i.id);
                        }
                        ui.end_row();
                    }
                });
            });
        });
        }
        if let Some((id, amt)) = pay {
            self.pay_dlg = Some((false, id, amt, today()));
        }
        if let Some(cust) = deposit {
            self.pay_dlg = Some((true, cust, String::new(), today()));
        }
        if let Some(id) = apply {
            let r = self.books.apply_deposit(id, &now);
            let msg = format!("Applied {} of the customer's deposit.", r.as_ref().map_or(String::new(), |c| fmt_cents(*c)));
            self.done(r, &msg);
        }
        if let Some(id) = convert {
            let due = if self.i_terms > 0 { add_days(today(), self.i_terms).to_string() } else { String::new() };
            let r = self.books.convert_quote(id, &now, &due);
            self.done(r, "Quote converted to an invoice dated today.");
        }
        if let Some((doc, id)) = view {
            self.open_doc(doc, id);
        }
        if let Some((doc, id, reminder)) = mail {
            self.email_doc(doc, id, reminder);
        }
        self.payment_dialog(ui);
    }

    fn payment_dialog(&mut self, ui: &mut egui::Ui) {
        let Some((is_deposit, id, amount, date)) = self.pay_dlg.as_mut() else { return };
        let is_deposit = *is_deposit;
        let who = self.customers.iter().find(|c| c.id == *id).map_or(String::new(), |c| c.name.clone());
        let accounts = &self.accounts;
        let bank = &mut self.i_bank;
        let (mut save, mut cancel) = (false, false);
        let m = egui::Modal::new(egui::Id::unique("payment")).show(ui.ctx(), |ui| {
            if is_deposit {
                ui.label(RichText::new(format!("Take a deposit from {who}")).font(ui::semibold(18.0)));
                ui::label(ui, "It's held as Customer Deposits (not income yet) until you apply it to their invoice.");
            } else {
                ui.label(RichText::new(format!("Record a payment on invoice #{id}")).font(ui::semibold(18.0)));
                ui::label(ui, "Enter less than the balance for a partial payment.");
            }
            ui.add_space(6.0);
            egui::Grid::new("paygrid").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                ui::label(ui, "Amount");
                field(ui, amount, 140.0, "0.00");
                ui.end_row();
                ui::label(ui, "Date received");
                date_picker(ui, "paydate", date);
                ui.end_row();
                ui::label(ui, "Deposited into");
                account_combo(ui, "paybank", accounts, BANKS, bank);
                ui.end_row();
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                save = ui::primary(ui, if is_deposit { "Record deposit" } else { "Record payment" });
                cancel = ui.button("Cancel").clicked();
            });
        });
        let (id, amount, date) = (*id, amount.clone(), date.to_string());
        if save {
            let r = match parse_cents(&amount) {
                Some(c) if self.i_bank != 0 && is_deposit => self.books.record_deposit(id, &date, self.i_bank, c).map_err(|e| e.to_string()),
                Some(c) if self.i_bank != 0 => self.books.record_payment(id, &date, self.i_bank, c).map_err(|e| e.to_string()),
                Some(_) => Err("choose where the money went".to_string()),
                None => Err("enter an amount like 12.34".to_string()),
            };
            if self.done(r, if is_deposit { "Deposit recorded and held for this customer." } else { "Payment recorded." }) {
                self.pay_dlg = None;
            }
        } else if cancel || m.should_close() {
            self.pay_dlg = None;
        }
    }

    fn import(&mut self, ui: &mut egui::Ui) {
        ui::page(ui, "Import & Rules", "Bring in transactions from your bank. Rules sort them automatically; the rest land in Uncategorized.");
        ui::card(ui, "Bank CSV file", |ui| {
            egui::Grid::new("mform").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                ui::label(ui, "File");
                ui.horizontal(|ui| {
                    field(ui, &mut self.m_path, 360.0, "Choose a .csv file");
                    if ui.button("Browse…").clicked()
                        && let Some(p) = rfd::FileDialog::new().add_filter("CSV", &["csv", "txt"]).pick_file() {
                            self.m_path = p.display().to_string();
                        }
                });
                ui.end_row();
                ui::label(ui, "Into account");
                account_combo(ui, "mbank", &self.accounts, BANKS, &mut self.m_bank);
                ui.end_row();
                ui::label(ui, "Columns");
                ui.horizontal(|ui| {
                    for (i, label) in ["Date #", "Description #", "Amount #"].iter().enumerate() {
                        ui::label(ui, label);
                        ui.add(egui::DragValue::new(&mut self.m_cols[i]).range(1..=50));
                    }
                    ui.checkbox(&mut self.m_header, "First row is a header");
                });
                ui.end_row();
            });
            ui::label(ui, "The amount column must be signed: deposits positive, withdrawals negative. Re-importing the same file is safe.");
            if ui::primary(ui, "Import") {
                let path = self.m_path.trim().trim_matches('"');
                let r = std::fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|text| {
                    let [d, m, a] = self.m_cols.map(|c| c - 1);
                    self.books.import_csv(&text, self.m_bank, self.m_header, d, m, a)
                });
                let msg = match &r {
                    Ok((n, s)) => format!("Imported {n} rows, skipped {s} duplicates."),
                    Err(_) => String::new(),
                };
                self.done(r, &msg);
            }
        });

        let (mut add_rule, mut apply, mut del_rule) = (false, false, None);
        ui::card(ui, "Rules", |ui| {
            ui::label(ui, "When an imported description contains the text, file it under the category (and vendor) automatically.");
            ui.horizontal(|ui| {
                field(ui, &mut self.rl_pattern, 200.0, "Contains, e.g. SHELL");
                account_combo(ui, "rlacct", &self.accounts, &["income", "expense", "equity", "liability"], &mut self.rl_account);
                pick(ui, "rlvend", self.vendors.iter().map(|v| (v.id, v.name.as_str())), &mut self.rl_vendor, "(no vendor)");
                pick(ui, "rljob", self.jobs.iter().map(|j| (j.id, j.name.as_str())), &mut self.rl_job, "(no job)");
                add_rule = ui::primary(ui, "Add rule");
            });
            if !self.rules.is_empty() {
                let name = |id: i64| self.accounts.iter().find(|a| a.id == id).map_or("?".to_string(), |a| a.name.clone());
                let vend = |id: Option<i64>| id.and_then(|id| self.vendors.iter().find(|v| v.id == id)).map_or(String::new(), |v| v.name.clone());
                egui::Grid::new("rules").striped(true).num_columns(5).spacing([24.0, 6.0]).show(ui, |ui| {
                    for h in ["CONTAINS", "CATEGORY", "VENDOR", "JOB", ""] {
                        ui::th(ui, h);
                    }
                    ui.end_row();
                    for r in &self.rules {
                        ui.label(&r.pattern);
                        ui.label(name(r.account));
                        ui::label(ui, &vend(r.vendor));
                        ui::label(ui, r.job.and_then(|id| self.jobs.iter().find(|j| j.id == id)).map_or("", |j| j.name.as_str()));
                        if ui.small_button("✕").clicked() {
                            del_rule = Some(r.id);
                        }
                        ui.end_row();
                    }
                });
                apply = ui.button("Apply rules to uncategorized transactions now").clicked();
            }
        });
        if add_rule {
            let r = self.books.add_rule(&self.rl_pattern, self.rl_account, self.rl_vendor, self.rl_job);
            if self.done(r, "Rule added. New imports will follow it.") {
                self.rl_pattern.clear();
            }
        }
        if let Some(id) = del_rule {
            let r = self.books.delete_rule(id);
            self.done(r, "Rule removed.");
        }
        if apply {
            let r = self.books.apply_rules();
            let msg = format!("Categorized {} transaction(s).", r.as_ref().map_or(0, |n| *n));
            self.done(r, &msg);
        }

        let url = self.settings.simplefin_url.clone();
        let mut maps = vec![];
        ui::card(ui, "Automatic bank feed (SimpleFIN)", |ui| {
            let Some(url) = url else {
                ui.label("Download transactions automatically through SimpleFIN Bridge (about $15/year, read-only).");
                ui.hyperlink_to("Get a setup token at bridge.simplefin.org", "https://bridge.simplefin.org/");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.f_token).password(true).hint_text("Paste setup token").desired_width(360.0));
                    if ui::primary(ui, "Connect") {
                        // shortcut: blocks the UI during the HTTPS call; move to a thread if it feels slow.
                        let r = feed::claim(&self.f_token).and_then(|u| self.books.set_simplefin_url(&u).map_err(|e| e.to_string()));
                        if self.done(r, "Connected. Click Download to fetch your accounts.") {
                            self.f_token.clear();
                        }
                    }
                });
                return;
            };
            ui::label(ui, "Match each bank account to one of yours. Unmatched accounts are skipped.");
            if self.feeds.is_empty() {
                ui::label(ui, "Click Download once to see your bank accounts here.");
            }
            egui::Grid::new("feeds").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                for (fid, name, acct) in &self.feeds {
                    ui.label(name);
                    let mut sel = acct.unwrap_or(0);
                    if account_combo(ui, ("feed", fid), &self.accounts, BANKS, &mut sel) {
                        maps.push((fid.clone(), sel));
                    }
                    ui.end_row();
                }
            });
            if ui::primary(ui, "Download last 60 days") {
                // Re-downloading overlaps safely: each bank transaction id posts once.
                let start = chrono::Utc::now().timestamp() - 60 * 86_400;
                let r = feed::fetch(&url, start).and_then(|json| self.books.import_feed(&json));
                let msg = match &r {
                    Ok((n, s)) => format!("Downloaded {n} new transactions ({s} already in your books)."),
                    Err(_) => String::new(),
                };
                self.done(r, &msg);
            }
        });
        for (fid, sel) in maps {
            let r = self.books.map_feed_account(&fid, sel);
            self.done(r, "Account matched.");
        }
    }

    fn jobs_ui(&mut self, ui: &mut egui::Ui) {
        if let Some(id) = self.job_open.filter(|id| self.jobs.iter().any(|j| j.id == *id)) {
            return self.job_workspace(ui, id);
        }
        ui::page(ui, "Jobs & Time", "Tag invoices, expenses and hours with a job to see what each one earned and cost. Quotes start a job automatically.");
        ui::card(ui, "New job", |ui| {
            ui.horizontal(|ui| {
                field(ui, &mut self.j_name, 260.0, "Job name");
                ui::label(ui, "Customer");
                pick(ui, "jcust", self.customers.iter().map(|c| (c.id, c.name.as_str())), &mut self.j_cust, "(none)");
                ui.toggle_value(&mut self.j_new_cust, "+ New customer");
                if ui::primary(ui, "Add job") {
                    let r = self.books.add_job(&self.j_name, self.j_cust);
                    if self.done(r, "Job added. Pick it when you record expenses, time or invoices.") {
                        self.j_name.clear();
                    }
                }
            });
            if self.j_new_cust {
                let add = ui
                    .horizontal(|ui| {
                        field(ui, &mut self.c_name, 260.0, "Customer name");
                        field(ui, &mut self.c_email, 240.0, "Email");
                        ui.button("Add customer").clicked()
                    })
                    .inner;
                if add {
                    let r = self.books.add_customer(&self.c_name, &self.c_email);
                    if let Ok(id) = &r {
                        self.j_cust = Some(*id);
                    }
                    if self.done(r, "Customer added and picked for this job.") {
                        (self.c_name, self.c_email, self.j_new_cust) = (String::new(), String::new(), false);
                    }
                }
            }
        });

        let (mut open, mut bill) = (None, None);
        ui::card(ui, "All jobs", |ui| {
            if self.jobs.is_empty() {
                ui::label(ui, "No jobs yet.");
                return;
            }
            ui::label(ui, "Click a job to open it: its quotes, invoices, time and costs in one place.");
            egui::ScrollArea::horizontal().id_salt("jobscroll").show(ui, |ui| {
            egui::Grid::new("jobs").striped(true).num_columns(10).spacing([18.0, 8.0]).show(ui, |ui| {
                ui::th(ui, "JOB");
                ui::th(ui, "CUSTOMER");
                for h in ["QUOTED", "INVOICED", "EXPENSES", "PROFIT", "MARGIN", "UNBILLED"] {
                    th_num(ui, h);
                }
                ui.end_row();
                for j in &self.jobs {
                    let open_now = self.job_open == Some(j.id);
                    if ui.link(&j.name).on_hover_text("Open this job").clicked() {
                        self.job_open = Some(j.id);
                    }
                    let _ = open_now;
                    ui::label(ui, j.customer.as_deref().unwrap_or(""));
                    amount_cell(ui, if j.quoted > 0 { fmt_cents(j.quoted) } else { String::new() });
                    amount_cell(ui, fmt_cents(j.income));
                    amount_cell(ui, fmt_cents(j.expenses));
                    let p = j.income - j.expenses;
                    amount_cell(ui, RichText::new(fmt_cents(p)).color(if p >= 0 { MONEY_IN } else { MONEY_OUT }));
                    amount_cell(ui, if j.income > 0 { format!("{:.0}%", p as f64 * 100.0 / j.income as f64) } else { String::new() });
                    let unbilled: i64 = self.unbilled.iter().filter(|t| t.job_id == j.id).map(|t| t.cents()).sum();
                    amount_cell(ui, if unbilled > 0 { fmt_cents(unbilled) } else { String::new() });
                    if unbilled > 0 && ui::primary(ui, "Bill time") {
                        bill = Some(j.id);
                    } else if unbilled == 0 {
                        ui.label("");
                    }
                    if ui.link("Transactions →").clicked() {
                        open = Some(j.id);
                    }
                    ui.end_row();
                }
            });
            });
        });
    }

    /// Everything about one job on one page: totals, its quotes and invoices, time, and costs.
    fn job_workspace(&mut self, ui: &mut egui::Ui, id: i64) {
        let Some(j) = self.jobs.iter().find(|j| j.id == id).cloned() else { return };
        if ui.link("← All jobs").clicked() {
            self.job_open = None;
            return;
        }
        ui.add_space(4.0);
        ui::page(ui, &j.name, &j.customer.clone().map_or("No customer yet".to_string(), |c| format!("For {c}")));
        let unbilled: Vec<TimeEntry> = self.unbilled.iter().filter(|t| t.job_id == id).cloned().collect();
        let unbilled_cents: i64 = unbilled.iter().map(|t| t.cents()).sum();
        let profit = j.income - j.expenses;
        let margin = if j.income > 0 { format!("{:.0}% margin", profit as f64 * 100.0 / j.income as f64) } else { String::new() };
        ui::stat_tiles(
            ui,
            &[
                ("Quoted", fmt_cents(j.quoted), String::new()),
                ("Invoiced", fmt_cents(j.income), String::new()),
                ("Costs", fmt_cents(j.expenses), String::new()),
                ("Profit", fmt_cents(profit), margin),
                ("Unbilled time", fmt_cents(unbilled_cents), format!("{} entries", unbilled.len())),
            ],
        );
        let (mut new_quote, mut new_inv, mut register, mut bill) = (false, false, false, false);
        ui.horizontal(|ui| {
            new_quote = ui::primary(ui, "New quote for this job");
            new_inv = ui.button("New invoice for this job").clicked();
            if unbilled_cents > 0 {
                bill = ui.button(format!("Bill {} of time", fmt_cents(unbilled_cents))).clicked();
            }
            register = ui.button("Show its transactions").clicked();
        });
        ui.add_space(12.0);
        if new_quote || new_inv {
            self.i_cust = j.customer_id.unwrap_or(0);
            self.i_job = Some(id);
            (self.screen, self.inv_tab, self.i_quote) = (Screen::Invoices, 2, new_quote);
            if j.customer_id.is_none() {
                self.status = "This job has no customer; choose one on the form.".into();
            }
            return;
        }
        if register {
            (self.screen, self.reg_job, self.reg_uncat) = (Screen::Transactions, Some(id), false);
            self.reg_search.clear();
            return;
        }
        if bill {
            let due = if self.i_terms > 0 { add_days(today(), self.i_terms).to_string() } else { String::new() };
            let bp = parse_rate_bp(&self.i_tax).unwrap_or(0);
            let r = self.books.invoice_time(id, &today().to_string(), &due, bp);
            let msg = format!("Invoice #{} created from the job's time.", r.as_ref().map_or(0, |i| *i));
            self.done(r, &msg);
        }

        let mut view = None;
        let now = today().to_string();
        ui::card(ui, "Quotes and invoices", |ui| {
            let quotes: Vec<&Quote> = self.quotes.iter().filter(|q| q.job_id == Some(id)).collect();
            let invoices: Vec<&Invoice> = self.invoices.iter().filter(|i| i.job == Some(id)).collect();
            if quotes.is_empty() && invoices.is_empty() {
                ui::label(ui, "None yet. Start with \"New quote for this job\".");
                return;
            }
            egui::Grid::new("jobdocs").striped(true).num_columns(5).spacing([20.0, 6.0]).show(ui, |ui| {
                for h in ["", "DATE", "STATUS"] {
                    ui::th(ui, h);
                }
                th_num(ui, "AMOUNT");
                ui.end_row();
                for q in quotes {
                    ui.label(format!("Quote Q{}", q.id));
                    ui.label(nice_dates(&q.date));
                    ui::label(ui, &q.invoice_id.map_or("Open".to_string(), |i| format!("Invoiced as #{i}")));
                    amount_cell(ui, fmt_cents(q.cents));
                    if ui.small_button("View").clicked() {
                        view = Some((books::Doc::Quote, q.id));
                    }
                    ui.end_row();
                }
                for i in invoices {
                    ui.label(format!("Invoice #{}", i.number));
                    ui.label(nice_dates(&i.date));
                    if i.paid {
                        ui.label(RichText::new("Paid ✔").color(MONEY_IN));
                    } else if i.overdue(&now) {
                        ui.label(RichText::new(format!("Overdue, {} owed", fmt_cents(i.balance()))).color(MONEY_OUT));
                    } else {
                        ui::label(ui, &format!("{} owed", fmt_cents(i.balance())));
                    }
                    amount_cell(ui, fmt_cents(i.cents));
                    if ui.small_button("View").clicked() {
                        view = Some((books::Doc::Invoice, i.id));
                    }
                    ui.end_row();
                }
            });
        });
        if let Some((doc, did)) = view {
            self.open_doc(doc, did);
        }

        let (mut log, mut del_time) = (false, None);
        ui::card(ui, "Time", |ui| {
            ui.horizontal_wrapped(|ui| {
                date_picker(ui, "tmdate", &mut self.tm_date);
                field(ui, &mut self.tm_hours, 80.0, "Hours");
                ui::label(ui, "at $");
                field(ui, &mut self.tm_rate, 90.0, "per hour");
                field(ui, &mut self.tm_note, 280.0, "What you did");
                log = ui::primary(ui, "Log time");
            });
            if unbilled.is_empty() {
                return;
            }
            ui.add_space(6.0);
            egui::Grid::new("time").striped(true).num_columns(5).spacing([20.0, 6.0]).show(ui, |ui| {
                for h in ["DATE", "HOURS", "NOTE"] {
                    ui::th(ui, h);
                }
                th_num(ui, "UNBILLED");
                ui.end_row();
                for t in &unbilled {
                    ui.label(nice_dates(&t.date));
                    ui.label(format!("{:.2}", t.minutes as f64 / 60.0));
                    ui::label(ui, &t.note);
                    amount_cell(ui, fmt_cents(t.cents()));
                    if ui.small_button("✕").clicked() {
                        del_time = Some(t.id);
                    }
                    ui.end_row();
                }
            });
        });
        if log {
            let hours = self.tm_hours.trim().parse::<f64>().ok().filter(|h| *h > 0.0 && *h < 1000.0);
            let r = match (hours, parse_cents(&self.tm_rate)) {
                (None, _) => Err("enter hours like 2.5".to_string()),
                (_, None) => Err("enter an hourly rate like 60".to_string()),
                (Some(h), Some(rate)) => self.books.add_time(id, &self.tm_date.to_string(), (h * 60.0).round() as i64, rate, &self.tm_note).map_err(|e| e.to_string()),
            };
            if self.done(r, "Time logged.") {
                (self.tm_hours, self.tm_note) = (String::new(), String::new());
            }
        }
        if let Some(t) = del_time {
            let r = self.books.delete_time(t);
            self.done(r, "Removed.");
        }

        let costs = self.books.job_costs(id).unwrap_or_default();
        ui::card(ui, "Costs", |ui| {
            ui::label(ui, "Expenses count here once they're tagged with this job: pick the job when you record them, in the register, or with an import rule.");
            if costs.is_empty() {
                return;
            }
            let mut by_cat: Vec<(String, i64)> = vec![];
            for (_, _, cat, c) in &costs {
                match by_cat.iter_mut().find(|x| &x.0 == cat) {
                    Some(x) => x.1 += c,
                    None => by_cat.push((cat.clone(), *c)),
                }
            }
            egui::Grid::new("jobcats").num_columns(2).spacing([24.0, 4.0]).show(ui, |ui| {
                for (cat, c) in &by_cat {
                    ui.label(RichText::new(cat).font(ui::semibold(15.0)));
                    amount_cell(ui, fmt_cents(*c));
                    ui.end_row();
                }
            });
            ui.add_space(8.0);
            egui::Grid::new("jobcosts").striped(true).num_columns(4).spacing([20.0, 6.0]).show(ui, |ui| {
                for h in ["DATE", "DESCRIPTION", "CATEGORY"] {
                    ui::th(ui, h);
                }
                th_num(ui, "AMOUNT");
                ui.end_row();
                for (d, memo, cat, c) in &costs {
                    ui.label(d);
                    ui.label(memo);
                    ui::label(ui, cat);
                    amount_cell(ui, fmt_cents(*c));
                    ui.end_row();
                }
            });
        });
    }

    fn people(&mut self, ui: &mut egui::Ui) {
        ui::page(ui, "Customers & Vendors", "Who you bill and who you pay. Click a name to see or change all their details.");
        if let Some((vendor, p)) = self.party_edit.as_mut() {
            let vendor = *vendor;
            let (mut save, mut cancel) = (false, false);
            let title = match (vendor, p.id == 0) {
                (false, true) => "New customer",
                (true, true) => "New vendor",
                (false, false) => "Customer details",
                (true, false) => "Vendor details",
            };
            ui::card(ui, title, |ui| {
                ui.horizontal(|ui| {
                    ui::label(ui, "Display name");
                    field(ui, &mut p.name, 300.0, "How they show in lists and on invoices");
                    ui::label(ui, "Blank uses the company name.");
                });
                ui.add_space(4.0);
                contact_fields(ui, "partyinfo", &mut p.info);
                if vendor {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut p.is_1099, "1099 contractor")
                            .on_hover_text("Tick for contractors you pay by check or bank transfer ($2,000+ a year from 2026 needs a 1099-NEC)");
                        ui.add_space(16.0);
                        ui::label(ui, "Tax ID (from their W-9)");
                        field(ui, &mut p.tax_id, 180.0, "EIN or SSN");
                    });
                }
                ui.horizontal(|ui| {
                    ui::label(ui, "Notes");
                    ui.add(egui::TextEdit::multiline(&mut p.info.extra).desired_width(560.0).desired_rows(3).hint_text("Private: gate code, preferences, account numbers…"));
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    save = ui::primary(ui, "Save");
                    cancel = ui.button("Cancel").clicked();
                });
            });
            if save {
                if p.name.trim().is_empty() {
                    p.name = p.info.name.clone();
                }
                let p = p.clone();
                let r = self.books.save_party(vendor, &p);
                if self.done(r, "Saved.") {
                    self.party_edit = None;
                }
            } else if cancel {
                self.party_edit = None;
            }
            return;
        }
        ui.horizontal(|ui| {
            field(ui, &mut self.people_q, 360.0, "Search by name, company, contact, phone, email or city");
            if !self.people_q.is_empty() && ui.small_button("✕").on_hover_text("Clear").clicked() {
                self.people_q.clear();
            }
        });
        ui.add_space(10.0);
        let q = self.people_q.clone();
        let mut open = None;
        for vendor in [false, true] {
            let list = if vendor { &self.vendors } else { &self.customers };
            ui::card(ui, if vendor { "Vendors" } else { "Customers" }, |ui| {
                if ui::primary(ui, if vendor { "+ New vendor" } else { "+ New customer" }) {
                    open = Some((vendor, Party::default()));
                }
                ui.add_space(6.0);
                if list.is_empty() {
                    ui::label(ui, "None yet.");
                    return;
                }
                egui::ScrollArea::horizontal().id_salt(("pscroll", vendor)).show(ui, |ui| {
                    egui::Grid::new(("plist", vendor)).striped(true).num_columns(7).spacing([20.0, 6.0]).show(ui, |ui| {
                        for h in ["NAME", "COMPANY", "CONTACT", "PHONE", "EMAIL", "CITY"] {
                            ui::th(ui, h);
                        }
                        if vendor {
                            ui::th(ui, "1099");
                        }
                        ui.end_row();
                        for p in list.iter().filter(|p| {
                            let i = &p.info;
                            matches(&format!("{} {} {} {} {} {} {} {}", p.name, i.name, i.dba, i.contact, i.phone, p.email, i.city, p.address), &q)
                        }) {
                            if ui.link(&p.name).clicked() {
                                open = Some((vendor, p.clone()));
                            }
                            ui.label(&p.info.name);
                            ui.label(&p.info.contact);
                            ui.label(&p.info.phone);
                            ui.label(&p.email);
                            ui.label(&p.info.city);
                            if vendor {
                                ui.label(if p.is_1099 { "Yes" } else { "" });
                            }
                            ui.end_row();
                        }
                    });
                });
            });
        }
        if open.is_some() {
            self.party_edit = open;
        }
    }

    fn reconcile(&mut self, ui: &mut egui::Ui) {
        ui::page(ui, "Reconcile", "Match your books to a bank or card statement. Tick each transaction that appears on the statement.");
        let liability = self.account(self.rc_account).is_some_and(|a| a.kind == "liability");
        // Show balances the way the statement does: what's in the bank, or what you owe on the card.
        let sign = if liability { -1 } else { 1 };
        let lines = self.books.recon_lines(self.rc_account, &self.rc_date.to_string()).unwrap_or_default();
        let cleared = self.books.cleared_balance(self.rc_account).unwrap_or(0) * sign;
        let last = self.books.last_reconciled(self.rc_account).ok().flatten();
        let ending = parse_cents(&self.rc_ending);
        let mut toggles = vec![];
        let mut finish = false;
        ui::card(ui, "Statement", |ui| {
            egui::Grid::new("rcform").num_columns(6).spacing([16.0, 10.0]).show(ui, |ui| {
                ui::label(ui, "Account");
                account_combo(ui, "rcacct", &self.accounts, BANKS, &mut self.rc_account);
                ui::label(ui, "Statement date");
                date_picker(ui, "rcdate", &mut self.rc_date);
                ui::label(ui, if liability { "Statement balance owed" } else { "Statement ending balance" });
                field(ui, &mut self.rc_ending, 140.0, "0.00");
                ui.end_row();
            });
            if let Some((d, c)) = &last {
                ui::label(ui, &format!("Last reconciled through {} at {}.", nice_dates(&d), fmt_cents(c * sign)));
            }
        });
        ui::card(ui, "Transactions to check off", |ui| {
            if lines.is_empty() {
                ui::label(ui, "Nothing left to check off through this date.");
            }
            egui::Grid::new("rclines").striped(true).num_columns(4).spacing([20.0, 6.0]).show(ui, |ui| {
                for l in &lines {
                    let mut on = l.ticked;
                    if ui.checkbox(&mut on, "").changed() {
                        toggles.push((l.line_id, on));
                    }
                    ui.label(nice_dates(&l.date));
                    ui.label(&l.memo);
                    amount_cell(ui, money(l.cents * sign));
                    ui.end_row();
                }
            });
        });
        ui::card(ui, "", |ui| {
            let diff = ending.map(|e| e - cleared);
            egui::Grid::new("rcsum").num_columns(2).spacing([24.0, 6.0]).show(ui, |ui| {
                ui::label(ui, "Statement balance");
                amount_cell(ui, ending.map_or("—".to_string(), fmt_cents));
                ui.end_row();
                ui::label(ui, "Cleared in your books");
                amount_cell(ui, fmt_cents(cleared));
                ui.end_row();
                ui.label(RichText::new("Difference").font(ui::semibold(15.0)));
                let d = diff.map_or("—".to_string(), fmt_cents);
                amount_cell(ui, RichText::new(d).font(ui::semibold(15.0)).color(if diff == Some(0) { MONEY_IN } else { MONEY_OUT }));
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.add_enabled_ui(diff == Some(0), |ui| finish = ui::primary(ui, "Finish reconciling"));
            if diff.is_some_and(|d| d != 0) {
                ui::label(ui, "When the difference is $0.00 you can finish. Missing something? Record it on Transactions.");
            }
        });
        for (line, on) in toggles {
            let r = self.books.tick(line, on);
            if let Err(e) = r {
                self.status = format!("Error: {e}");
            }
        }
        if finish {
            let r = self.books.finish_reconcile(self.rc_account, &self.rc_date.to_string(), ending.unwrap_or(0) * sign);
            if self.done(r, "Reconciled. Those transactions are now locked in as cleared.") {
                self.rc_ending.clear();
            }
        }
    }

    fn mileage(&mut self, ui: &mut egui::Ui) {
        let rate = self.settings.mileage_rate;
        ui::page(
            ui,
            "Mileage",
            &format!("Log business driving. At {} a mile it's a tax deduction (it doesn't move money in your books).", fmt_cents(rate)),
        );
        let mut add = false;
        ui::card(ui, "Log a trip", |ui| {
            ui.horizontal_wrapped(|ui| {
                date_picker(ui, "midate", &mut self.mi_date);
                field(ui, &mut self.mi_purpose, 300.0, "Purpose, e.g. Lumber yard pickup");
                field(ui, &mut self.mi_miles, 90.0, "Miles");
                pick(ui, "mijob", self.jobs.iter().map(|j| (j.id, j.name.as_str())), &mut self.mi_job, "(no job)");
                add = ui::primary(ui, "Add trip");
            });
            ui::label(ui, "Change the rate on Business & Settings when the IRS publishes a new one.");
        });
        if add {
            let tenths = self.mi_miles.trim().parse::<f64>().ok().map(|m| (m * 10.0).round() as i64);
            let r = match tenths {
                Some(t) => self.books.add_trip(&self.mi_date.to_string(), &self.mi_purpose, t, self.mi_job).map_err(|e| e.to_string()),
                None => Err("enter miles like 12.5".to_string()),
            };
            if self.done(r, "Trip logged.") {
                (self.mi_purpose, self.mi_miles) = (String::new(), String::new());
            }
        }
        let mut del = None;
        let year = today().year();
        ui::card(ui, &format!("{year} trips"), |ui| {
            let miles: i64 = self.trips.iter().map(|t| t.tenths).sum();
            ui::stat_tiles(
                ui,
                &[
                    ("Business miles", format!("{:.1}", miles as f64 / 10.0), String::new()),
                    ("Deduction so far", fmt_cents((miles * rate + 5) / 10), String::new()),
                ],
            );
            if self.trips.is_empty() {
                ui::label(ui, "No trips logged this year.");
                return;
            }
            egui::Grid::new("trips").striped(true).num_columns(6).spacing([20.0, 6.0]).show(ui, |ui| {
                for h in ["DATE", "PURPOSE", "JOB", "MILES"] {
                    ui::th(ui, h);
                }
                th_num(ui, "DEDUCTION");
                ui.end_row();
                for t in &self.trips {
                    ui.label(nice_dates(&t.date));
                    ui.label(&t.purpose);
                    ui::label(ui, t.job.as_deref().unwrap_or(""));
                    ui.label(format!("{:.1}", t.tenths as f64 / 10.0));
                    amount_cell(ui, fmt_cents((t.tenths * rate + 5) / 10));
                    if ui.small_button("✕").clicked() {
                        del = Some(t.id);
                    }
                    ui.end_row();
                }
            });
        });
        if let Some(id) = del {
            let r = self.books.delete_trip(id);
            self.done(r, "Trip removed.");
        }
    }

    fn proposals_ui(&mut self, ui: &mut egui::Ui) {
        ui::page(ui, "Proposals", "Write up the scope, timeline and terms around a quote. Every proposal is kept, accepted or not.");
        if self.prop.is_none() {
            let mut open = None;
            ui.horizontal(|ui| {
                if ui::primary(ui, "+ New proposal") {
                    self.prop = Some(proposals::Proposal { date: today().to_string(), status: "draft".into(), ..Default::default() });
                }
                ui.add_space(12.0);
                field(ui, &mut self.prop_q, 280.0, "Search by title or customer");
                for (f, label) in [("", "All"), ("draft", "Drafts"), ("sent", "Sent"), ("accepted", "Accepted"), ("declined", "Declined")] {
                    ui.selectable_value(&mut self.prop_filter, f, label);
                }
            });
            ui.add_space(10.0);
            ui::card(ui, "", |ui| {
                let shown: Vec<&proposals::Proposal> = self
                    .proposals
                    .iter()
                    .filter(|p| (self.prop_filter.is_empty() || p.status == self.prop_filter) && matches(&format!("{} {}", p.title, p.customer), &self.prop_q))
                    .collect();
                if shown.is_empty() {
                    ui::label(ui, if self.proposals.is_empty() { "No proposals yet. Start one from a template with \"+ New proposal\"." } else { "Nothing matches." });
                    return;
                }
                egui::Grid::new("props").striped(true).num_columns(5).spacing([24.0, 8.0]).show(ui, |ui| {
                    for h in ["TITLE", "CUSTOMER", "DATE", "STATUS"] {
                        ui::th(ui, h);
                    }
                    ui.end_row();
                    for p in shown {
                        if ui.link(&p.title).clicked() {
                            open = Some(p.clone());
                        }
                        ui.label(&p.customer);
                        ui.label(nice_dates(&p.date));
                        let color = match p.status.as_str() {
                            "accepted" => MONEY_IN,
                            "declined" => MONEY_OUT,
                            _ => ui::pal(ui).weak,
                        };
                        ui.label(RichText::new(&p.status).color(color));
                        ui.end_row();
                    }
                });
            });
            if open.is_some() {
                self.prop = open;
            }
            return;
        }

        let (mut save, mut close, mut print, mut email, mut apply_tpl, mut save_tpl, mut del_tpl, mut delete, mut convert) =
            (false, false, false, false, false, false, false, false, false);
        let (mut add_cust, mut add_job) = (false, false);
        let quotes: Vec<(i64, String)> = self.quotes.iter().map(|q| (q.id, format!("Q{} {} {}", q.id, q.customer, fmt_cents(q.cents)))).collect();
        let Some(p) = self.prop.as_mut() else { return };
        if ui.link("← All proposals").clicked() {
            close = true;
        }
        ui.add_space(6.0);
        ui::card(ui, if p.id == 0 { "New proposal" } else { "Edit proposal" }, |ui| {
            egui::Grid::new("propform").num_columns(4).spacing([16.0, 10.0]).show(ui, |ui| {
                ui::label(ui, "Title");
                field(ui, &mut p.title, 320.0, "e.g. Walnut counter and shelving");
                ui::label(ui, "Status");
                egui::ComboBox::from_id_salt("pstatus").selected_text(p.status.clone()).width(160.0).show_ui(ui, |ui| {
                    for st in proposals::STATUSES {
                        ui.selectable_value(&mut p.status, st.to_string(), st);
                    }
                });
                ui.end_row();
                ui::label(ui, "Customer");
                ui.horizontal(|ui| {
                    pick(ui, "pcust", self.customers.iter().map(|c| (c.id, c.name.as_str())), &mut p.customer_id, "(none)");
                    ui.toggle_value(&mut self.prop_new_cust, "+ New customer");
                });
                ui::label(ui, "Job");
                ui.horizontal(|ui| {
                    pick(ui, "pjob", self.jobs.iter().map(|j| (j.id, j.name.as_str())), &mut p.job_id, "(none)");
                    ui.toggle_value(&mut self.prop_new_job, "+ New job");
                });
                ui.end_row();
                ui::label(ui, "Pricing from quote");
                pick(ui, "pquote", quotes.iter().map(|q| (q.0, q.1.as_str())), &mut p.quote_id, "(no pricing table)");
                ui::label(ui, "Date");
                let mut d: Date = p.date.parse().unwrap_or(today());
                date_picker(ui, "pdate", &mut d);
                p.date = d.to_string();
                ui.end_row();
            });
            if self.prop_new_cust {
                ui.horizontal(|ui| {
                    field(ui, &mut self.c_name, 260.0, "Customer name");
                    field(ui, &mut self.c_email, 240.0, "Email");
                    add_cust = ui::primary(ui, "Add customer");
                });
            }
            if self.prop_new_job {
                ui.horizontal(|ui| {
                    field(ui, &mut self.j_name, 260.0, if p.title.is_empty() { "Job name" } else { &p.title });
                    ui::label(ui, "for this proposal's customer");
                    add_job = ui::primary(ui, "Add job");
                });
            }
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui::label(ui, "Template");
                pick(ui, "ptpl", self.templates.iter().map(|t| (t.0, t.1.as_str())), &mut self.tpl_pick, "Choose…");
                apply_tpl = ui.add_enabled(self.tpl_pick.is_some(), egui::Button::new("Use template")).on_hover_text("Replaces the text below").clicked();
                del_tpl = self.tpl_pick.is_some() && ui.small_button("Delete template").clicked();
                ui.add_space(16.0);
                field(ui, &mut self.tpl_name, 200.0, "Template name");
                save_tpl = ui.button("Save text as template").clicked();
            });
            ui::label(ui, "Write with \"# \" for a heading and \"- \" for a bullet. Fill-ins: {customer} {business} {title} {job} {date} {total}, and {pricing} on its own line for the quote's price table.");
            ui.add(egui::TextEdit::multiline(&mut p.body).desired_width(f32::INFINITY).desired_rows(18).font(egui::TextStyle::Body));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                save = ui::primary(ui, "Save");
                print = ui.button("Preview / print").clicked();
                email = ui.button("Email as PDF").clicked();
                let open_quote = p.quote_id.and_then(|q| self.quotes.iter().find(|x| x.id == q)).is_some_and(|q| q.invoice_id.is_none());
                if p.status == "accepted" && open_quote {
                    convert = ui.button("Turn its quote into an invoice").clicked();
                }
                if p.id != 0 {
                    ui.add_space(24.0);
                    delete = ui.small_button("Delete proposal").clicked();
                }
            });
        });
        if add_cust {
            let r = self.books.add_customer(&self.c_name, &self.c_email);
            if let (Ok(id), Some(p)) = (&r, self.prop.as_mut()) {
                p.customer_id = Some(*id);
            }
            if self.done(r, "Customer added and picked for this proposal.") {
                (self.c_name, self.c_email, self.prop_new_cust) = (String::new(), String::new(), false);
            }
        }
        if add_job {
            let (cust, title) = self.prop.as_ref().map_or((None, String::new()), |p| (p.customer_id, p.title.clone()));
            // A blank name takes the proposal's title.
            let name = if self.j_name.trim().is_empty() { title } else { self.j_name.clone() };
            let r = self.books.add_job(&name, cust);
            if let (Ok(id), Some(p)) = (&r, self.prop.as_mut()) {
                p.job_id = Some(*id);
            }
            if self.done(r, "Job added and picked for this proposal.") {
                (self.j_name, self.prop_new_job) = (String::new(), false);
            }
        }
        let Some(p) = self.prop.as_mut() else { return };
        let draft = p.clone();
        if apply_tpl
            && let Some(t) = self.templates.iter().find(|t| Some(t.0) == self.tpl_pick)
                && let Some(p) = self.prop.as_mut() {
                    p.body = t.2.clone();
                }
        if save_tpl {
            let r = self.books.save_template(&self.tpl_name, &draft.body);
            if self.done(r, "Template saved.") {
                self.tpl_name.clear();
            }
        }
        if del_tpl
            && let Some(id) = self.tpl_pick.take() {
                let r = self.books.delete_template(id);
                self.done(r, "Template deleted.");
            }
        // Printing and emailing work from the saved copy, so save first.
        if save || print || email || convert {
            let r = self.books.save_proposal(&draft);
            match &r {
                Ok(id) => {
                    if let Some(p) = self.prop.as_mut() {
                        p.id = *id;
                    }
                }
                Err(_) => {
                    self.done(r, "");
                    return;
                }
            }
            let id = r.unwrap_or(0);
            self.dirty = true;
            if save {
                self.status = "Proposal saved.".into();
            }
            if print {
                let pdf = self.books.proposal_pdf(id);
                self.open_pdf(&format!("proposal-{id}"), pdf);
            }
            if email {
                match self.books.proposal_email(id).map_err(|e| e.to_string()).and_then(|m| Ok((m, self.books.proposal_pdf(id)?))) {
                    Ok(((to, subject, body), pdf)) => self.send_pdf_email(to, subject, body, pdf, &format!("Proposal {id}"), "customer", "Customers & Vendors"),
                    Err(e) => self.status = format!("Error: {e}"),
                }
            }
            if convert
                && let Some(q) = draft.quote_id {
                    let due = if self.i_terms > 0 { add_days(today(), self.i_terms).to_string() } else { String::new() };
                    let r = self.books.convert_quote(q, &today().to_string(), &due);
                    self.done(r, "Quote converted to an invoice dated today.");
                }
        }
        if delete {
            let r = self.books.delete_proposal(draft.id);
            if self.done(r, "Proposal deleted.") {
                self.prop = None;
            }
        }
        if close {
            self.prop = None;
        }
    }

    fn purchases(&mut self, ui: &mut egui::Ui) {
        ui::page(ui, "Purchase Orders", "Order from vendors, mark it received when it arrives, then pay the bill. Costs land on the job.");
        let cost_kinds = ["expense", "asset"];
        let mut save = false;
        ui::card(ui, "New purchase order", |ui| {
            egui::Grid::new("poform").num_columns(6).spacing([16.0, 10.0]).show(ui, |ui| {
                ui::label(ui, "Vendor");
                ui.horizontal(|ui| {
                    pick(ui, "povend", self.vendors.iter().map(|v| (v.id, v.name.as_str())), &mut self.po_vendor, "Choose…");
                    ui.toggle_value(&mut self.po_new_vendor, "+ New vendor");
                });
                ui::label(ui, "Date");
                date_picker(ui, "podate", &mut self.po_date);
                ui::label(ui, "Job");
                pick(ui, "pojob", self.jobs.iter().map(|j| (j.id, j.name.as_str())), &mut self.po_job, "(none)");
                ui.end_row();
            });
            if self.po_new_vendor {
                let add = ui
                    .horizontal(|ui| {
                        field(ui, &mut self.v_name, 260.0, "Vendor name");
                        field(ui, &mut self.v_email, 240.0, "Email (optional)");
                        ui::primary(ui, "Add vendor")
                    })
                    .inner;
                if add {
                    let r = self.books.add_vendor(&self.v_name, &self.v_email);
                    if let Ok(id) = &r {
                        self.po_vendor = Some(*id);
                    }
                    if self.done(r, "Vendor added and picked.") {
                        (self.v_name, self.v_email, self.po_new_vendor) = (String::new(), String::new(), false);
                    }
                }
            }
            ui.add_space(6.0);
            let mut remove = None;
            egui::Grid::new("poitems").num_columns(4).spacing([12.0, 8.0]).show(ui, |ui| {
                ui::th(ui, "DESCRIPTION");
                ui::th(ui, "AMOUNT");
                ui::th(ui, "CATEGORY");
                ui.end_row();
                for (i, (d, a, acct)) in self.po_items.iter_mut().enumerate() {
                    field(ui, d, 360.0, "What you're ordering");
                    field(ui, a, 120.0, "0.00");
                    account_combo(ui, ("poacct", i), &self.accounts, &cost_kinds, acct);
                    if ui.small_button("✕").clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
            if let Some(i) = remove {
                self.po_items.remove(i);
            }
            ui.horizontal(|ui| {
                if ui.button("+ Add line").clicked() {
                    let last = self.po_items.last().map_or(0, |l| l.2);
                    self.po_items.push((String::new(), String::new(), last));
                }
                ui.toggle_value(&mut self.po_new_cat, "+ New category");
                let total: i64 = self.po_items.iter().filter_map(|l| parse_cents(&l.1)).sum();
                ui.label(RichText::new(format!("Total {}", fmt_cents(total))).font(ui::semibold(16.0)));
            });
            if self.po_new_cat {
                let add = ui
                    .horizontal(|ui| {
                        field(ui, &mut self.t_cat_name, 260.0, "Expense category name");
                        ui::primary(ui, "Add category")
                    })
                    .inner;
                if add {
                    let r = self.books.add_account("", &self.t_cat_name, "expense", "");
                    if let Ok(id) = &r {
                        // Goes on every line still without a category, or the last line.
                        let (n, unset) = (self.po_items.len(), self.po_items.iter().any(|l| l.2 == 0));
                        for (i, l) in self.po_items.iter_mut().enumerate() {
                            if l.2 == 0 || (i + 1 == n && !unset) {
                                l.2 = *id;
                            }
                        }
                    }
                    if self.done(r, "Category added and picked.") {
                        (self.t_cat_name, self.po_new_cat) = (String::new(), false);
                    }
                }
            }
            field(ui, &mut self.po_notes, 520.0, "Notes for the vendor (delivery, pickup, specs)");
            save = ui::primary(ui, "Save purchase order");
        });
        if save {
            let items: Option<Vec<purchasing::PoItem>> = self
                .po_items
                .iter()
                .filter(|(d, a, _)| !(d.trim().is_empty() && a.trim().is_empty()))
                .map(|(d, a, acct)| parse_cents(a).map(|c| (d.trim().to_string(), c, *acct)))
                .collect();
            let r = match items {
                Some(items) => self
                    .books
                    .create_po(&purchasing::PoInput {
                        vendor_id: self.po_vendor.unwrap_or(0),
                        date: self.po_date.to_string(),
                        job: self.po_job,
                        items,
                        notes: self.po_notes.clone(),
                    })
                    .map_err(|e| e.to_string()),
                None => Err("check the amounts".to_string()),
            };
            if self.done(r, "Purchase order saved. Email or print it for the vendor.") {
                self.po_items = vec![(String::new(), String::new(), self.po_items.first().map_or(0, |l| l.2))];
                self.po_notes.clear();
            }
        }

        let (mut view, mut mail, mut cancel, mut dlg) = (None, None, None, None);
        ui::card(ui, "Purchase orders", |ui| {
            if self.pos.is_empty() {
                ui::label(ui, "No purchase orders yet.");
                return;
            }
            egui::ScrollArea::horizontal().id_salt("poscroll").show(ui, |ui| {
                egui::Grid::new("pos").striped(true).num_columns(9).spacing([14.0, 8.0]).show(ui, |ui| {
                    for h in ["PO", "VENDOR", "JOB", "DATE"] {
                        ui::th(ui, h);
                    }
                    th_num(ui, "TOTAL");
                    ui::th(ui, "STATUS");
                    ui.end_row();
                    for p in &self.pos {
                        ui.label(format!("#{}", p.id));
                        ui.label(&p.vendor);
                        ui::label(ui, p.job.as_deref().unwrap_or(""));
                        ui.label(nice_dates(&p.date));
                        amount_cell(ui, fmt_cents(p.total));
                        match (p.status.as_str(), p.owed) {
                            ("received", true) => ui.label(RichText::new(format!("Received {}, bill to pay", nice_dates(&p.received))).color(MONEY_OUT)),
                            ("received", false) => ui.label(RichText::new(format!("Received {}, paid ✔", nice_dates(&p.received))).color(MONEY_IN)),
                            ("cancelled", _) => ui.label(RichText::new("Cancelled").color(ui::pal(ui).weak)),
                            _ => ui.label("Open"),
                        };
                        if ui.button("View / print").clicked() {
                            view = Some(p.id);
                        }
                        if ui.button("Email").clicked() {
                            mail = Some(p.id);
                        }
                        ui.horizontal(|ui| {
                            if p.status == "open" {
                                if ui::primary(ui, "Mark received") {
                                    dlg = Some((true, p.id));
                                }
                                if ui.small_button("Cancel PO").clicked() {
                                    cancel = Some(p.id);
                                }
                            } else if p.owed && ui::primary(ui, "Pay bill") {
                                dlg = Some((false, p.id));
                            }
                        });
                        ui.end_row();
                    }
                });
            });
        });
        if let Some(id) = view {
            let pdf = self.books.po_pdf(id);
            self.open_pdf(&format!("po-{id}"), pdf);
        }
        if let Some(id) = mail {
            match self.books.po_email(id).map_err(|e| e.to_string()).and_then(|m| Ok((m, self.books.po_pdf(id)?))) {
                Ok(((to, subject, body), pdf)) => self.send_pdf_email(to, subject, body, pdf, &format!("Purchase Order {id}"), "vendor", "Customers & Vendors"),
                Err(e) => self.status = format!("Error: {e}"),
            }
        }
        if let Some(id) = cancel {
            let r = self.books.cancel_po(id);
            self.done(r, "Purchase order cancelled.");
        }
        if let Some((receive, id)) = dlg {
            self.po_dlg = Some((receive, id, today(), if receive { None } else { Some(self.i_bank) }));
        }
        self.po_dialog(ui);
    }

    fn po_dialog(&mut self, ui: &mut egui::Ui) {
        let Some((receive, id, date, paid_from)) = self.po_dlg.as_mut() else { return };
        let receive = *receive;
        let banks = self.accounts.iter().filter(|a| BANKS.contains(&a.kind.as_str()) && a.system.is_none()).map(|a| (a.id, a.name.as_str()));
        let (mut ok, mut cancel) = (false, false);
        let m = egui::Modal::new(egui::Id::unique("podlg")).show(ui.ctx(), |ui| {
            let title = if receive { format!("PO #{id} received") } else { format!("Pay the bill for PO #{id}") };
            ui.label(RichText::new(title).font(ui::semibold(18.0)));
            ui.add_space(6.0);
            egui::Grid::new("podlggrid").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                ui::label(ui, "Date");
                date_picker(ui, "podlgdate", date);
                ui.end_row();
                ui::label(ui, if receive { "Paid" } else { "Paid from" });
                pick(ui, "podlgbank", banks, paid_from, if receive { "Not yet: the vendor will bill me" } else { "Choose…" });
                ui.end_row();
            });
            if receive {
                ui::label(ui, "Not paid yet? It's kept as a bill to pay (Accounts Payable) until you pay it.");
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ok = ui::primary(ui, if receive { "Mark received" } else { "Pay bill" });
                cancel = ui.button("Cancel").clicked();
            });
        });
        let (id, date, paid_from) = (*id, date.to_string(), *paid_from);
        if ok {
            let r = match (receive, paid_from) {
                (true, from) => self.books.receive_po(id, &date, from).map_err(|e| e.to_string()),
                (false, Some(bank)) => self.books.pay_po(id, &date, bank).map_err(|e| e.to_string()),
                (false, None) => Err("choose where the money came from".to_string()),
            };
            if self.done(r, if receive { "Received. The cost is booked (and on the job, if it has one)." } else { "Bill paid." }) {
                self.po_dlg = None;
            }
        } else if cancel || m.should_close() {
            self.po_dlg = None;
        }
    }

    fn assets_ui(&mut self, ui: &mut egui::Ui) {
        ui::page(ui, "Assets", "Equipment, vehicles, computers and other things you own and use for years, grouped by category.");
        // Categories are the fixed-asset accounts, so each asset lands on the right line of the balance sheet.
        let cats: Vec<&Account> = self
            .accounts
            .iter()
            .filter(|a| a.kind == "asset" && a.system.is_none() && a.active && (a.number.is_empty() || a.number.as_str() >= "1200"))
            .collect();
        let (mut add, mut add_cat) = (false, false);
        ui::card(ui, "Add an asset", |ui| {
            egui::Grid::new("assetform").num_columns(4).spacing([16.0, 10.0]).show(ui, |ui| {
                ui::label(ui, "Name");
                field(ui, &mut self.as_new.name, 260.0, "e.g. Table saw");
                ui::label(ui, "Category");
                let text = cats.iter().find(|a| a.id == self.as_new.account).map_or("Choose…".to_string(), |a| a.name.clone());
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("ascat").selected_text(text).width(220.0).show_ui(ui, |ui| {
                        for physical in [true, false] {
                            ui.label(RichText::new(if physical { "Physical: depreciated" } else { "Intangible: amortized" }).small().color(ui::pal(ui).weak));
                            for a in cats.iter().filter(|a| a.intangible != physical) {
                                ui.selectable_value(&mut self.as_new.account, a.id, &a.name);
                            }
                        }
                    });
                    ui.toggle_value(&mut self.as_new_cat, "+ New category");
                });
                ui.end_row();
                ui::label(ui, "Bought");
                date_picker(ui, "asdate", &mut self.as_date);
                ui::label(ui, "Cost");
                field(ui, &mut self.as_cost, 140.0, "0.00");
                ui.end_row();
                ui::label(ui, "Useful life (years)");
                ui.add(egui::DragValue::new(&mut self.as_new.life_years).range(0..=50))
                    .on_hover_text("For straight-line depreciation. 0 = not depreciated (land, for example)");
                ui::label(ui, "Serial number");
                field(ui, &mut self.as_new.serial, 220.0, "optional");
                ui.end_row();
                ui::label(ui, "Paid from");
                pick(ui, "aspaid", self.accounts.iter().filter(|a| BANKS.contains(&a.kind.as_str()) && a.system.is_none()).map(|a| (a.id, a.name.as_str())), &mut self.as_paid_from, "Already in my books");
                ui::label(ui, "Notes");
                field(ui, &mut self.as_new.notes, 220.0, "optional");
                ui.end_row();
            });
            if self.as_new_cat {
                add_cat = ui
                    .horizontal(|ui| {
                        field(ui, &mut self.as_cat_name, 260.0, "Category name, e.g. Tools");
                        ui.checkbox(&mut self.as_cat_intangible, "Intangible")
                            .on_hover_text("Software, websites, domain names, licenses: things you can't touch. They're amortized instead of depreciated.");
                        ui::primary(ui, "Add category")
                    })
                    .inner;
            }
            if parse_cents(&self.as_cost).is_some_and(|c| c > 0 && c < 250_000) {
                ui.label(
                    RichText::new("Under $2,500: you can usually just expense this instead of tracking it as an asset (the de minimis safe harbor). Record it as Money out under Tools & Small Equipment or Office Supplies. Track it here anyway if you want it on your insurance list.")
                        .color(ui::pal(ui).weak),
                );
            }
            ui::label(ui, "Pick \"Paid from\" to book the purchase now; leave it if you already recorded the purchase as a transaction. Add a photo from the list below.");
            add = ui::primary(ui, "Add asset");
        });
        if add_cat {
            // Categories are asset accounts; numbered after the existing ones (1500s, before Accumulated Depreciation at 1590).
            let next = self.accounts.iter().filter(|a| a.kind == "asset").filter_map(|a| a.number.parse::<u32>().ok()).filter(|n| (1500..1590).contains(n)).max().map_or(1500, |n| n + 10);
            let desc = if self.as_cat_intangible { "Intangible asset category" } else { "Fixed asset category" };
            let r = self.books.add_account(&next.min(1589).to_string(), &self.as_cat_name, "asset", desc).and_then(|id| {
                self.books.set_intangible(id, self.as_cat_intangible)?;
                Ok(id)
            });
            if let Ok(id) = &r {
                self.as_new.account = *id;
            }
            if self.done(r, "Category added and picked.") {
                (self.as_cat_name, self.as_new_cat, self.as_cat_intangible) = (String::new(), false, false);
            }
        }
        if add {
            let r = match parse_cents(&self.as_cost) {
                Some(c) => {
                    let a = Asset { cost: c, purchased: self.as_date.to_string(), ..self.as_new.clone() };
                    self.books.add_asset(&a, self.as_paid_from).map_err(|e| e.to_string())
                }
                None => Err("enter the cost like 1200.00".to_string()),
            };
            if self.done(r, "Asset added.") {
                self.as_new = Asset { life_years: self.as_new.life_years, account: self.as_new.account, ..Default::default() };
                self.as_cost.clear();
            }
        }

        let now = today().to_string();
        let (mut save, mut delete) = (None, None);
        let (mut photo_add, mut photo_view, mut photo_del) = (None, None, None);
        let mut cat = String::new();
        ui::card(ui, "Your assets", |ui| {
            if self.assets.is_empty() {
                ui::label(ui, "No assets yet.");
                return;
            }
            egui::ScrollArea::horizontal().id_salt("assetscroll").show(ui, |ui| {
                egui::Grid::new("assets").striped(true).num_columns(10).spacing([14.0, 6.0]).show(ui, |ui| {
                    for h in ["NAME", "SERIAL", "BOUGHT", "LIFE"] {
                        ui::th(ui, h);
                    }
                    for h in ["COST", "DEPRECIATED", "BOOK VALUE"] {
                        th_num(ui, h);
                    }
                    ui::th(ui, "DISPOSED");
                    ui::th(ui, "PHOTO");
                    ui.end_row();
                    for a in self.assets.iter_mut() {
                        if a.category != cat {
                            cat = a.category.clone();
                            ui.label(RichText::new(&cat).font(ui::semibold(16.0)).color(ACCENT));
                            if a.intangible {
                                ui.label(RichText::new("intangible · amortized").small().color(ui::pal(ui).weak));
                            }
                            ui.end_row();
                        }
                        let mut c = field(ui, &mut a.name, 200.0, "").lost_focus();
                        c |= field(ui, &mut a.serial, 130.0, "").lost_focus();
                        ui.label(nice_dates(&a.purchased));
                        c |= ui.add(egui::DragValue::new(&mut a.life_years).range(0..=50).suffix(" yr")).changed();
                        let dep = a.depreciation_through(&now);
                        amount_cell(ui, fmt_cents(a.cost));
                        amount_cell(ui, fmt_cents(dep));
                        amount_cell(ui, fmt_cents(a.cost - dep));
                        let mut gone = !a.disposed.is_empty();
                        let when = a.disposed.clone();
                        if ui.checkbox(&mut gone, when).on_hover_text("Tick when you sell or scrap it").changed() {
                            a.disposed = if gone { now.clone() } else { String::new() };
                            c = true;
                        }
                        ui.horizontal(|ui| {
                            if a.photo {
                                if ui.link("View").clicked() {
                                    photo_view = Some(a.id);
                                }
                                if ui.small_button("Replace").clicked() {
                                    photo_add = Some(a.id);
                                }
                                if ui.small_button("✕").on_hover_text("Remove the photo").clicked() {
                                    photo_del = Some(a.id);
                                }
                            } else if ui.button("+ Photo").on_hover_text("For insurance: a picture of the item, its serial plate or the receipt").clicked() {
                                photo_add = Some(a.id);
                            }
                        });
                        if ui.small_button("✕").on_hover_text("Remove from the list (a booked purchase stays in your books)").clicked() {
                            delete = Some(a.id);
                        }
                        if c {
                            save = Some(a.clone());
                        }
                        ui.end_row();
                    }
                });
            });
        });
        if let Some(a) = save {
            let r = self.books.update_asset(&a);
            self.done(r, "Asset saved.");
        }
        if let Some(id) = delete {
            let r = self.books.delete_asset(id);
            self.done(r, "Removed from the list.");
        }
        if let Some(id) = photo_add
            && let Some(p) = rfd::FileDialog::new().add_filter("Photo", &["jpg", "jpeg", "png", "heic", "webp", "pdf"]).pick_file() {
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or("photo.jpg".into());
                let r = std::fs::read(&p).map_err(|e| e.to_string()).and_then(|b| self.books.set_asset_photo(id, &name, &b).map_err(|e| e.to_string()));
                self.done(r, "Photo saved with the asset.");
            }
        if let Some(id) = photo_view {
            let r = self.books.asset_photo(id).map_err(|e| e.to_string()).and_then(|a| {
                let (name, data) = a.ok_or("no photo on this asset")?;
                let safe: String = name.chars().map(|c| if c.is_alphanumeric() || ".-_".contains(c) { c } else { '_' }).collect();
                let path = std::env::temp_dir().join(format!("AccountingCraft-asset-{id}-{safe}"));
                std::fs::write(&path, data).map_err(|e| e.to_string())?;
                open_external(&path.display().to_string())
            });
            self.done(r, "Opened the photo.");
        }
        if let Some(id) = photo_del {
            let r = self.books.remove_asset_photo(id);
            self.done(r, "Photo removed.");
        }

        let year = today().year();
        let mut book = None;
        ui::card(ui, "Depreciation & amortization", |ui| {
            ui::label(ui, "Straight-line, month by month from purchase. Intangibles (software, websites) are amortized the same way into their own accounts. Book it once a year, usually at year end. Tax depreciation (Section 179, bonus) can differ; ask your preparer.");
            for y in [year - 1, year] {
                let amount = self.books.depreciation_for_year(y).unwrap_or(0);
                let booked = self.books.depreciation_booked(y).unwrap_or(false);
                ui.horizontal(|ui| {
                    ui.label(format!("{y}: {}", fmt_cents(amount)));
                    if booked {
                        ui.label(RichText::new("booked ✔").color(MONEY_IN));
                    } else if amount > 0 && ui.button(format!("Book {y} depreciation")).clicked() {
                        book = Some(y);
                    }
                });
            }
        });
        if let Some(y) = book {
            let r = self.books.post_depreciation(y);
            self.done(r, &format!("{y} depreciation booked on Dec 31."));
        }
    }

    fn accounts_ui(&mut self, ui: &mut egui::Ui) {
        ui::page(
            ui,
            "Chart of Accounts",
            "Edit any field; it saves when you leave it. Accounts with transactions keep their type; switch them off to hide them.",
        );
        ui::card(ui, "New account", |ui| {
            ui.horizontal(|ui| {
                field(ui, &mut self.a_number, 70.0, "No.");
                field(ui, &mut self.a_name, 240.0, "Name");
                egui::ComboBox::from_id_salt("akind").selected_text(self.a_kind).width(120.0).show_ui(ui, |ui| {
                    for k in KINDS {
                        ui.selectable_value(&mut self.a_kind, k, k);
                    }
                });
                field(ui, &mut self.a_desc, 280.0, "Description (optional)");
                if ui::primary(ui, "Add account") {
                    let r = self.books.add_account(&self.a_number, &self.a_name, self.a_kind, &self.a_desc);
                    if self.done(r, "Account added.") {
                        (self.a_number, self.a_name, self.a_desc) = (String::new(), String::new(), String::new());
                    }
                }
            });
        });
        let (mut save, mut delete) = (None, None);
        let mut section = String::new();
        ui::card(ui, "", |ui| {
            // Wide table: scroll sideways inside the card instead of overflowing it.
            egui::ScrollArea::horizontal().id_salt("accts").show(ui, |ui| {
                egui::Grid::new("accts").striped(true).num_columns(7).spacing([12.0, 6.0]).show(ui, |ui| {
                    for h in ["NO.", "NAME", "TYPE", "DESCRIPTION", "ON", "USED", ""] {
                        ui::th(ui, h);
                    }
                    ui.end_row();
                    for a in self.accounts.iter_mut() {
                        if a.kind != section {
                            section = a.kind.clone();
                            let title = match section.as_str() {
                                "asset" => "Assets",
                                "liability" => "Liabilities",
                                "equity" => "Equity",
                                "income" => "Income",
                                _ => "Expenses",
                            };
                            ui.label(RichText::new(title).font(ui::semibold(16.0)).color(ACCENT));
                            ui.end_row();
                        }
                        let locked = a.used > 0 || a.system.is_some();
                        let mut changed = field(ui, &mut a.number, 70.0, "").lost_focus();
                        changed |= field(ui, &mut a.name, 240.0, "").lost_focus();
                        ui.add_enabled_ui(!locked, |ui| {
                            egui::ComboBox::from_id_salt(("kind", a.id)).selected_text(a.kind.clone()).width(110.0).show_ui(ui, |ui| {
                                for k in KINDS {
                                    changed |= ui.selectable_value(&mut a.kind, k.to_string(), k).changed();
                                }
                            });
                        });
                        changed |= field(ui, &mut a.description, 280.0, "").lost_focus();
                        changed |= ui.add_enabled(a.system.is_none(), egui::Checkbox::without_text(&mut a.active)).changed();
                        ui::label(ui, &if a.used > 0 { a.used.to_string() } else { String::new() });
                        if a.system.is_some() {
                            ui.label(RichText::new("built in").small().color(ui::pal(ui).weak))
                                .on_hover_text("The app posts here automatically. You can rename it.");
                        } else if !locked && ui.small_button("✕").on_hover_text("Delete").clicked() {
                            delete = Some(a.id);
                        } else {
                            ui.label("");
                        }
                        if changed {
                            save = Some(a.clone());
                        }
                        ui.end_row();
                    }
                });
            });
        });
        if let Some(a) = save {
            let r = self.books.update_account(&a);
            self.done(r, "Account saved.");
        }
        if let Some(id) = delete {
            let r = self.books.delete_account(id);
            self.done(r, "Account deleted.");
        }
    }

    fn business(&mut self, ui: &mut egui::Ui) {
        ui::page(ui, "Business & Settings", "Your details for invoices, plus rates, the books lock, backups and logins.");
        ui::card(ui, "Details", |ui| {
            contact_fields(ui, "biz", &mut self.biz);
            ui.horizontal(|ui| {
                ui::label(ui, "Extra lines");
                ui.add(egui::TextEdit::multiline(&mut self.biz.extra).desired_width(300.0).desired_rows(2).hint_text("Optional, e.g. license number. Printed last."));
            });
            ui::label(ui, "The company name, address, phones, email and website print at the top of invoices, quotes, POs and proposals.");
            if ui::primary(ui, "Save") {
                let r = self.books.save_business(&self.biz);
                self.done(r, "Saved.");
            }
        });
        ui::card(ui, "Logo", |ui| {
            ui::label(ui, &match &self.settings.logo {
                Some(b) => format!("Logo stored in your books file ({} KB).", b.len() / 1024),
                None => "No logo yet. PNG or JPEG, up to 2 MB.".into(),
            });
            ui.horizontal(|ui| {
                if ui::primary(ui, "Choose logo…")
                    && let Some(p) = rfd::FileDialog::new().add_filter("Image", &["png", "jpg", "jpeg"]).pick_file() {
                        let r = std::fs::read(&p).map_err(|e| e.to_string()).and_then(|b| self.books.set_logo(Some(&b)).map_err(|e| e.to_string()));
                        self.done(r, "Logo saved. Open any invoice to see it.");
                    }
                if self.settings.logo.is_some() && ui.button("Remove logo").clicked() {
                    let r = self.books.set_logo(None);
                    self.done(r, "Logo removed.");
                }
            });
        });
        let mut save = false;
        ui::card(ui, "Settings", |ui| {
            egui::Grid::new("prefs").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                ui::label(ui, "Online payment link");
                field(ui, &mut self.p_pay_link, 420.0, "https://buy.stripe.com/… or https://paypal.me/…")
                    .on_hover_text("Shown as a Pay online button on invoices and in invoice emails");
                ui.end_row();
                ui::label(ui, "Mileage rate ($ per mile)");
                field(ui, &mut self.p_mileage, 100.0, "0.70");
                ui.end_row();
                ui::label(ui, "Default hourly rate ($)");
                field(ui, &mut self.p_hourly, 100.0, "0.00");
                ui.end_row();
                ui::label(ui, "Income tax % for estimates");
                field(ui, &mut self.p_tax_pct, 100.0, "12");
                ui.end_row();
                ui::label(ui, "Lock the books");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.p_lock_on, "Lock everything dated on or before");
                    ui.add_enabled_ui(self.p_lock_on, |ui| date_picker(ui, "plock", &mut self.p_lock));
                });
                ui.end_row();
            });
            ui::label(ui, "Lock a year once your taxes are filed so nothing in it changes by accident.");
            save = ui::primary(ui, "Save settings");
        });
        if save {
            let mut s = self.settings.clone();
            s.pay_link = self.p_pay_link.trim().to_string();
            s.lock_date = if self.p_lock_on { self.p_lock.to_string() } else { String::new() };
            let r = match (parse_cents(&self.p_mileage), parse_cents(&self.p_hourly), self.p_tax_pct.trim().parse::<i64>()) {
                (Some(m), Some(h), Ok(t)) => {
                    (s.mileage_rate, s.hourly_rate, s.income_tax_pct) = (m, h, t);
                    self.books.save_prefs(&s).map_err(|e| e.to_string())
                }
                _ => Err("check the rates: numbers like 0.70, 60 and 12".to_string()),
            };
            if self.done(r, "Settings saved.") {
                self.settings = s;
                self.load_prefs();
            }
        }
        let mut backup = false;
        ui::card(ui, "Backups", |ui| {
            let dir = std::path::Path::new(&self.path).parent().map(|p| p.join("backups")).unwrap_or_default();
            let shown = std::fs::canonicalize(&dir).map(|p| p.display().to_string()).unwrap_or(dir.display().to_string());
            ui::label(ui, &format!("A copy is saved automatically every time you open the app (the newest 30 are kept) in {}", shown.trim_start_matches(r"\\?\")));
            ui.horizontal(|ui| {
                backup = ui::primary(ui, "Back up now…");
                ui::label(ui, "Save a copy anywhere, like a USB drive or cloud folder. To restore, use Open books… in the sidebar.");
            });
        });
        if backup {
            let name = format!("accountingcraft-backup-{}.db", today());
            if let Some(p) = rfd::FileDialog::new().set_file_name(&name).add_filter("Books", &["db"]).save_file() {
                let r = self.books.backup_to(&p).map_err(|e| e.to_string()).and_then(|_| {
                    // The backup is encrypted; it opens with the same logins.
                    std::fs::copy(lock::keys_path(&self.path), lock::keys_path(&p.to_string_lossy())).map(|_| ()).map_err(|e| e.to_string())
                });
                self.done(r, &format!("Backed up to {} (keep the .keys file with it)", p.display()));
            }
        }
        self.logins(ui);
    }

    /// Owner-only: who can open these books, passwords and the recovery key.
    fn logins(&mut self, ui: &mut egui::Ui) {
        let users = self.books.users().unwrap_or_default();
        let mut action: Option<Box<dyn FnOnce(&mut App) -> (Result<(), String>, String)>> = None;
        ui::card(ui, "Logins & security", |ui| {
            ui::label(ui, "These books are encrypted. Each person signs in with their own password. Owners can do everything; bookkeepers can't open this screen; view-only people can look but not change anything.");
            ui.add_space(6.0);
            egui::Grid::new("users").num_columns(3).spacing([24.0, 6.0]).show(ui, |ui| {
                ui::th(ui, "USERNAME");
                ui::th(ui, "ROLE");
                ui.label("");
                ui.end_row();
                for (name, role) in &users {
                    ui.label(name);
                    ui.label(role.label());
                    if !name.eq_ignore_ascii_case(self.user.trim()) && ui.small_button("Remove").clicked() {
                        let n = name.clone();
                        action = Some(Box::new(move |a: &mut App| {
                            (lock::remove_login(&a.books, &a.path, &a.user, &n), format!("Removed {n}. They can no longer sign in. If they might have a copy of your books, change the encryption key below."))
                        }));
                    }
                    ui.end_row();
                }
            });
            ui.add_space(8.0);
            ui.label(RichText::new("Add someone, or reset their password").strong());
            ui.horizontal(|ui| {
                field(ui, &mut self.u_name, 160.0, "Username");
                ui.add_sized([180.0, 32.0], egui::TextEdit::singleline(&mut self.u_pw).password(true).hint_text("Their password"));
                egui::ComboBox::from_id_salt("u_role").selected_text(self.u_role.label()).show_ui(ui, |ui| {
                    for r in lock::Role::ALL {
                        ui.selectable_value(&mut self.u_role, r, r.label());
                    }
                });
                if ui::primary(ui, "Save login") {
                    action = Some(Box::new(|a: &mut App| {
                        let (n, pw, r) = (a.u_name.clone(), std::mem::take(&mut a.u_pw), a.u_role);
                        let res = lock::set_login(&a.books, &a.key, &a.path, &n, &pw, r);
                        if res.is_ok() {
                            a.u_name.clear();
                        }
                        (res, format!("Saved the login for {n}. Give them their password in person."))
                    }));
                }
            });
            ui.add_space(8.0);
            ui.label(RichText::new("Your password").strong());
            ui.horizontal(|ui| {
                ui.add_sized([180.0, 32.0], egui::TextEdit::singleline(&mut self.u_my_pw).password(true).hint_text("New password"));
                if ui.button("Change my password").clicked() {
                    action = Some(Box::new(|a: &mut App| {
                        let pw = std::mem::take(&mut a.u_my_pw);
                        (lock::change_password(&a.key, &a.path, &a.user, &pw), "Password changed.".into())
                    }));
                }
            });
            ui.add_space(8.0);
            ui.label(RichText::new("Recovery key").strong());
            ui::label(ui, "If every owner forgets their password, the recovery key is the only way back in. Without a password or this key, nobody can open your books.");
            match &self.u_recovery {
                Some(code) => {
                    ui.label(RichText::new(code).monospace().size(18.0));
                    ui::label(ui, "Your old recovery key no longer works. Print or save this one and keep it somewhere safe.");
                }
                None if !self.u_temps.is_empty() => {}
                None => {
                    if ui.button("Make a new recovery key").clicked() {
                        action = Some(Box::new(|a: &mut App| match lock::new_recovery_key(&a.key, &a.path) {
                            Ok(c) => {
                                a.u_recovery = Some(c);
                                (Ok(()), "New recovery key made.".into())
                            }
                            Err(e) => (Err(e), String::new()),
                        }));
                    }
                }
            }
            ui.add_space(8.0);
            ui.label(RichText::new("Change the encryption key").strong());
            ui::label(ui, "Do this after removing someone who might have kept a copy of your books and key file: their copy won't open these books any more. You keep your password. Everyone else gets a new temporary password, and you get a new recovery key. Backups made before today keep the old key.");
            if self.u_temps.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_sized([180.0, 32.0], egui::TextEdit::singleline(&mut self.u_rot_pw).password(true).hint_text("Your password"));
                    if ui.button("Change the key").clicked() {
                        action = Some(Box::new(|a: &mut App| {
                            let pw = std::mem::take(&mut a.u_rot_pw);
                            match lock::rotate_key(&a.books, &a.key, &a.path, &a.user, &pw) {
                                Ok((key, temps, code)) => {
                                    a.key = key;
                                    a.u_recovery = Some(code);
                                    a.u_temps = temps;
                                    (Ok(()), "The books now use a new encryption key.".into())
                                }
                                Err(e) => (Err(e), String::new()),
                            }
                        }));
                    }
                });
            } else {
                ui::label(ui, "Done. Give each person their new password in person. An owner can set a different one for them any time above.");
                egui::Grid::new("temps").num_columns(2).spacing([24.0, 6.0]).show(ui, |ui| {
                    for (name, pw) in &self.u_temps {
                        ui.label(name);
                        ui.label(RichText::new(pw).monospace().size(16.0));
                        ui.end_row();
                    }
                });
            }
        });
        if let Some(f) = action {
            let (r, ok) = f(self);
            self.done(r, &ok);
        }
    }
}

/// What the sign-in screen is showing.
#[derive(Default)]
struct Gate {
    user: String,
    pw: String,
    pw2: String,
    code: String,
    recovering: bool,
    msg: String,
    /// Just set up: the recovery key to show once, and the session waiting behind it.
    recovery: Option<String>,
    pending: Option<lock::Session>,
    delete_plain: bool,
}

/// The window: the sign-in screen until someone unlocks the books, then the app.
struct Root {
    path: String,
    remember: bool,
    app: Option<App>,
    gate: Gate,
    last_input: std::time::Instant,
    backed_up: Option<String>,
    theme: egui::ThemePreference,
}

const IDLE_LOCK: std::time::Duration = std::time::Duration::from_secs(15 * 60);

impl Root {
    fn start(&mut self, s: lock::Session) {
        if self.backed_up.as_deref() != Some(self.path.as_str()) {
            let _ = extras::auto_backup(&s.books, &self.path); // best effort; the app works without it
            self.backed_up = Some(self.path.clone()); // once per file per run, not after every lock
        }
        if self.remember {
            remember_books(&self.path);
        }
        self.app = Some(App::new(s, self.path.clone()));
        self.gate = Gate::default();
        self.last_input = std::time::Instant::now();
    }

    fn gate_ui(&mut self, ui: &mut egui::Ui) {
        let p = ui::pal(ui);
        let has_logins = lock::keys_path(&self.path).exists();
        let name = std::path::Path::new(&self.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let mut result: Option<Result<lock::Session, String>> = None;
        let mut try_sample = false;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg)).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space((ui.available_height() * 0.12).max(20.0));
                let img = egui::ColorImage::from_rgba_unmultiplied([64, 64], &icon::abacus(64));
                let tex = ui.ctx().load_texture("gate-logo", img, egui::TextureOptions::LINEAR);
                ui.add(egui::Image::new((tex.id(), egui::vec2(56.0, 56.0))));
                ui.label(RichText::new("AccountingCraft").font(ui::semibold(24.0)));
                ui.label(RichText::new(&name).color(p.weak)).on_hover_text(&self.path);
                ui.add_space(16.0);
            });
            ui.vertical_centered(|ui| {
                ui.set_max_width(440.0);
                let g = &mut self.gate;
                if let Some(code) = g.recovery.clone() {
                    ui::card(ui, "Save your recovery key", |ui| {
                        ui::label(ui, "Your books are now encrypted. If you ever forget your password, this key is the only way back in. Nobody else can recover your books for you.");
                        ui.add_space(8.0);
                        ui.label(RichText::new(&code).monospace().size(16.0).strong());
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Copy").clicked() {
                                ui.ctx().copy_text(code.clone());
                            }
                            if ui.button("Save to a file…").clicked()
                                && let Some(f) = rfd::FileDialog::new().set_file_name("AccountingCraft recovery key.txt").save_file() {
                                    let text = format!("AccountingCraft recovery key for {}\r\n\r\n{code}\r\n\r\nKeep this somewhere safe, away from this computer.\r\n", self.path);
                                    g.msg = match std::fs::write(&f, text) {
                                        Ok(_) => format!("Saved to {}", f.display()),
                                        Err(e) => format!("Couldn't save: {e}"),
                                    };
                                }
                        });
                        let plain = lock::plain_backups(&self.path);
                        if !plain.is_empty() {
                            ui.add_space(8.0);
                            ui.checkbox(&mut g.delete_plain, if plain.len() == 1 { "Delete my old unencrypted backup".to_string() } else { format!("Delete my {} old unencrypted backups", plain.len()) })
                                .on_hover_text("Backups made before today aren't encrypted. New backups are.");
                        }
                        ui.add_space(8.0);
                        if ui::primary(ui, "I've saved it. Continue") {
                            if g.delete_plain {
                                for f in plain {
                                    let _ = std::fs::remove_file(f);
                                }
                            }
                            if let Some(s) = g.pending.take() {
                                result = Some(Ok(s));
                            }
                        }
                    });
                } else if g.recovering {
                    ui::card(ui, "Reset an owner's password", |ui| {
                        ui::label(ui, "Enter the recovery key you saved when you first protected these books.");
                        egui::Grid::new("rec").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                            ui::label(ui, "Recovery key");
                            field(ui, &mut g.code, 280.0, "XXXX-XXXX-…");
                            ui.end_row();
                            ui::label(ui, "Owner username");
                            field(ui, &mut g.user, 280.0, "");
                            ui.end_row();
                            ui::label(ui, "New password");
                            ui.add_sized([280.0, 32.0], egui::TextEdit::singleline(&mut g.pw).password(true));
                            ui.end_row();
                            ui::label(ui, "Again");
                            ui.add_sized([280.0, 32.0], egui::TextEdit::singleline(&mut g.pw2).password(true));
                            ui.end_row();
                        });
                        ui.horizontal(|ui| {
                            if ui::primary(ui, "Reset and sign in") {
                                result = Some(if g.pw != g.pw2 { Err("The passwords don't match.".into()) } else { lock::recover(&self.path, &g.code, &g.user, &g.pw) });
                            }
                            if ui.button("Back").clicked() {
                                g.recovering = false;
                                g.msg.clear();
                            }
                        });
                    });
                } else if has_logins {
                    ui::card(ui, "Sign in", |ui| {
                        egui::Grid::new("login").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                            ui::label(ui, "Username");
                            let u = field(ui, &mut g.user, 280.0, "");
                            if g.user.is_empty() && !u.has_focus() && g.msg.is_empty() {
                                u.request_focus();
                            }
                            ui.end_row();
                            ui::label(ui, "Password");
                            let pw = ui.add_sized([280.0, 32.0], egui::TextEdit::singleline(&mut g.pw).password(true));
                            if !g.user.is_empty() && g.pw.is_empty() && !u.has_focus() && !pw.has_focus() {
                                pw.request_focus();
                            }
                            ui.end_row();
                        });
                        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                        ui.horizontal(|ui| {
                            if ui::primary(ui, "Sign in") || (enter && !g.pw.is_empty()) {
                                result = Some(lock::login(&self.path, &g.user, &g.pw));
                            }
                            if ui.link("Forgot your password?").clicked() {
                                g.recovering = true;
                                g.msg.clear();
                            }
                        });
                    });
                } else {
                    ui::card(ui, "Protect your books", |ui| {
                        ui::label(ui, "Create the owner login. Your books will be encrypted so only people you give a login can open them. You can add a bookkeeper or view-only logins later in Business & Settings.");
                        ui.add_space(6.0);
                        egui::Grid::new("setup").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                            ui::label(ui, "Your username");
                            field(ui, &mut g.user, 280.0, "e.g. your first name");
                            ui.end_row();
                            ui::label(ui, "Password");
                            ui.add_sized([280.0, 32.0], egui::TextEdit::singleline(&mut g.pw).password(true).hint_text("8 or more characters"));
                            ui.end_row();
                            ui::label(ui, "Again");
                            ui.add_sized([280.0, 32.0], egui::TextEdit::singleline(&mut g.pw2).password(true));
                            ui.end_row();
                        });
                        if ui::primary(ui, "Encrypt and continue") {
                            if g.pw != g.pw2 {
                                g.msg = "The passwords don't match.".into();
                            } else {
                                match lock::setup(&self.path, &g.user, &g.pw) {
                                    Ok((s, code)) => {
                                        g.pending = Some(s);
                                        g.recovery = Some(code);
                                        g.msg.clear();
                                    }
                                    Err(e) => g.msg = e,
                                }
                            }
                        }
                    });
                }
                if !g.msg.is_empty() {
                    ui.label(RichText::new(&g.msg).color(MONEY_OUT));
                }
                ui.add_space(8.0);
                if g.recovery.is_none() && ui.small_button("Open other books…").clicked()
                    && let Some(f) = rfd::FileDialog::new().add_filter("Books", &["db"]).pick_file() {
                        self.path = f.display().to_string();
                        self.remember = true;
                        self.gate = Gate::default();
                    }
                if self.gate.recovery.is_none() && ui.small_button("Try with sample books").on_hover_text("A made-up business to explore. Your own books aren't touched.").clicked() {
                    try_sample = true;
                }
            });
        });
        if try_sample {
            match open_sample() {
                Ok((path, s)) => {
                    (self.path, self.remember) = (path, false);
                    self.start(s);
                    if let Some(app) = self.app.as_mut() {
                        app.status = format!("These are sample books. If they lock, sign in as \"{SAMPLE_LOGIN}\" with password \"{SAMPLE_PASSWORD}\".");
                    }
                }
                Err(e) => self.gate.msg = e,
            }
            return;
        }
        match result {
            Some(Ok(s)) => self.start(s),
            Some(Err(e)) => {
                self.gate.pw.clear();
                self.gate.pw2.clear();
                self.gate.msg = e;
            }
            None => {}
        }
    }
}

const SAMPLE_LOGIN: &str = "sample";
const SAMPLE_PASSWORD: &str = "sample-books";

/// Fresh sample books in the temp folder, replacing any from last time.
fn open_sample() -> Result<(String, lock::Session), String> {
    let path = std::env::temp_dir().join("AccountingCraft-sample.db").display().to_string();
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(lock::keys_path(&path));
    let (s, _) = lock::setup(&path, SAMPLE_LOGIN, SAMPLE_PASSWORD)?;
    sample::fill(&s.books, today()).map_err(|e| format!("Couldn't make the sample books: {e}"))?;
    Ok((path, s))
}

impl eframe::App for Root {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let theme = ui.ctx().options(|o| o.theme_preference);
        if theme != self.theme {
            self.theme = theme;
            if let Some(f) = theme_file() {
                let _ = f.parent().map(std::fs::create_dir_all);
                let _ = std::fs::write(f, format!("{theme:?}"));
            }
        }
        let Some(app) = self.app.as_mut() else {
            return self.gate_ui(ui);
        };
        let now = std::time::Instant::now();
        if ui.input(|i| !i.events.is_empty() || i.pointer.is_moving()) {
            self.last_input = now;
        }
        let idle = now.duration_since(self.last_input) > IDLE_LOCK && app.u_recovery.is_none(); // never lose an unsaved recovery key
        if app.lock_now || idle {
            // Dropping the app closes the database; signing in again reopens it.
            let user = app.user.clone();
            self.app = None;
            self.gate = Gate { user, msg: "Locked. Sign in to continue.".into(), ..Default::default() };
            return self.gate_ui(ui);
        }
        app.ui(ui, frame);
        if let Some(p) = app.switch_to.take() {
            self.path = p;
            self.remember = true;
            self.app = None;
            self.gate = Gate::default();
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_secs(30)); // so the idle lock fires
    }
}

fn main() -> eframe::Result {
    // One file per business. Order: the path given on the command line, the last one opened, the default.
    let arg = std::env::args().nth(1);
    let path = arg
        .clone()
        .or_else(|| last_books_file().and_then(|f| std::fs::read_to_string(f).ok()).map(|s| s.trim().to_string()).filter(|p| std::path::Path::new(p).exists()))
        .unwrap_or_else(|| {
            // First run: Documents\AccountingCraft, not whatever folder the exe was started from (often Downloads).
            let dir = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(|h| std::path::Path::new(&h).join("Documents").join("AccountingCraft"));
            match dir.filter(|d| std::fs::create_dir_all(d).is_ok()) {
                Some(d) => d.join("my-books.db").display().to_string(),
                None => "accountingcraft.db".into(),
            }
        });
    // A one-off file given on the command line doesn't change the default.
    let root = Root { path, remember: arg.is_none(), app: None, gate: Gate::default(), last_input: std::time::Instant::now(), backed_up: None, theme: saved_theme() };
    let icon = egui::IconData { rgba: icon::abacus(256), width: 256, height: 256 };
    eframe::run_native(
        "AccountingCraft",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 820.0]).with_min_inner_size([900.0, 600.0]).with_icon(icon),
            ..Default::default()
        },
        Box::new(|cc| {
            ui::install(&cc.egui_ctx);
            cc.egui_ctx.set_theme(root.theme);
            Ok(Box::new(root))
        }),
    )
}
