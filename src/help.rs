//! Built-in help: one pop-up topic per screen, plus short guided tutorials that walk between screens.
//! Plain words on purpose; the reader is a busy owner, not a bookkeeper.

pub struct Topic {
    pub title: &'static str,
    pub intro: &'static str,
    /// (heading, bullet lines)
    pub sections: &'static [(&'static str, &'static [&'static str])],
}

pub struct Step {
    /// Screen key the step opens (see `main.rs` `screen_key`).
    pub screen: &'static str,
    pub title: &'static str,
    pub text: &'static str,
}

pub struct Tutorial {
    pub title: &'static str,
    pub blurb: &'static str,
    pub steps: &'static [Step],
}

pub fn topic(screen: &str) -> Topic {
    match screen {
        "home" => Topic {
            title: "Home",
            intro: "Your business at a glance: cash, what customers owe, what you owe, and profit, plus a to-do list.",
            sections: &[
                ("The tiles", &[
                    "Cash in the bank: everything in your bank and cash accounts.",
                    "Customers owe you: unpaid invoice balances. Overdue ones are flagged.",
                    "You owe: credit cards, loans, unpaid vendor bills, and deposits you're holding for customers.",
                    "Profit: income minus expenses for this month and this year.",
                ]),
                ("To do", &["Each line is a link straight to the fix: uncategorized transactions, overdue invoices, unbilled time, bills to pay."]),
                ("The chart", &["Green is income and red is expenses for each of the last 12 months. Hover a month for the numbers."]),
            ],
        },
        "transactions" => Topic {
            title: "Transactions",
            intro: "Every dollar in or out. Record it here, then sort it out in the register.",
            sections: &[
                ("Record money in, out, or a transfer", &[
                    "Money out: a purchase. Pick the bank or card it came from and a category like Job Materials.",
                    "Money in: a payment that isn't an invoice (invoice payments are recorded on Invoices).",
                    "Transfer: moving money between your own accounts, or paying off a credit card. It's not income or an expense.",
                    "Add a vendor and a job so the cost shows up on the job's profit.",
                    "Repeat: rent, subscriptions and other regular items post themselves on schedule.",
                ]),
                ("The register", &[
                    "Change a category, job or vendor right in the row; it saves at once.",
                    "Edit fixes the date, description, amount or account, and holds the receipt (📎).",
                    "Search and filters narrow the list; \"Put everything shown on job\" tags many at once.",
                    "Invoices, payments and reconciled items are protected; change them from their own screens.",
                ]),
            ],
        },
        "proposals" => Topic {
            title: "Proposals",
            intro: "A friendly write-up of the work, timeline and terms, with your quote's prices inside. Every proposal is kept, won or lost.",
            sections: &[
                ("Writing one", &[
                    "Start from a template, then edit. Use \"# \" at the start of a line for a heading and \"- \" for a bullet.",
                    "Fill-ins are swapped in when you print: {customer} {business} {title} {job} {date} {total}.",
                    "Put {pricing} on its own line to show the linked quote's price table.",
                ]),
                ("After you send it", &[
                    "Set the status to sent, then accepted or declined. Declined proposals stay on file for next time.",
                    "When it's accepted, \"Turn its quote into an invoice\" bills it in one click.",
                    "\"Save text as template\" keeps a version you like for future proposals.",
                ]),
            ],
        },
        "invoices" => Topic {
            title: "Quotes & Invoices",
            intro: "Quote a price, turn it into an invoice, then record payments as they come in.",
            sections: &[
                ("Creating", &[
                    "\"+ New quote\" or \"+ New invoice\" opens the form. The big Quote / Invoice switch at the top shows which you're making. Each line has a Tax box; untick it for non-taxable lines.",
                    "For invoices, Terms set the due date and Repeat makes a monthly (or weekly or yearly) invoice automatically.",
                    "A quote books nothing; it starts a job so you can track costs from day one. Convert it to an invoice when the customer agrees.",
                ]),
                ("Getting paid", &[
                    "Record payment takes full or partial payments. The balance shows in the list.",
                    "Take deposit (on a quote) holds money up front; Apply deposit puts it toward the invoice later.",
                    "Overdue invoices turn red and get a Send reminder button.",
                    "Email attaches a PDF in your mail program. If it can't, the PDF opens in a folder to drag in.",
                ]),
            ],
        },
        "jobs" => Topic {
            title: "Jobs & Time",
            intro: "See what each job earned and cost. Click a job to open everything about it in one place.",
            sections: &[
                ("Costs count against a job when they're tagged with it", &[
                    "Pick the job when you record an expense, in the register, on a purchase order, or with an import rule.",
                    "Invoices for the job count as its income. Profit and margin update on their own.",
                ]),
                ("Time", &[
                    "Log hours on the job's page. \"Bill time\" turns all unbilled hours into one invoice for the job's customer.",
                ]),
                ("Quoted against actual", &["The Quoted column shows the job's quotes, so you can compare them with what you actually billed and spent."]),
            ],
        },
        "purchases" => Topic {
            title: "Purchase Orders",
            intro: "Order from a vendor, then record it when it arrives.",
            sections: &[
                ("Steps", &[
                    "Create the PO: vendor, a job if it's for one, and each line with its category.",
                    "Email or print it for the vendor. Nothing is booked yet.",
                    "Mark received when the goods arrive. Choose how you paid, or \"the vendor will bill me\" to keep it as a bill to pay.",
                    "Pay bill when you pay the vendor's invoice.",
                ]),
                ("Good to know", &["Received costs land on the PO's job automatically. A customer's own PO number goes on your invoice form."]),
            ],
        },
        "people" => Topic {
            title: "Customers & Vendors",
            intro: "Who you bill and who you pay. Click a name to see or change all their details.",
            sections: &[
                ("Tips", &[
                    "Each one has a company name, DBA, contact person, address, two phones, email, website and private notes.",
                    "Search finds people by any word in their name, company, contact, phone, email or city.",
                    "A customer's contact (as \"Attn:\") and address print on invoices and quotes; their email is used when you email them.",
                    "For vendors, tick 1099 for contractors you pay by check or bank transfer and keep the tax ID from their W-9; the 1099 report lists who needs a form.",
                    "The \"+ New customer\" and \"+ New vendor\" buttons on other screens add just a name and email; fill in the rest here later.",
                ]),
            ],
        },
        "reports" => Topic {
            title: "Reports",
            intro: "Pick a report and dates. \"Show detail\" lists the transactions behind every line. Export CSV, Save PDF or Print any report.",
            sections: &[
                ("Which report?", &[
                    "Profit & Loss: did I make money? Balance Sheet: what do I own and owe?",
                    "Accounts Receivable Aging: who owes me, and for how long. Sales by Customer and Expenses by Vendor: your top relationships.",
                    "Job Profitability: every job's margin, or one job's detail.",
                    "Schedule C, 1099 Vendors, Estimated Quarterly Tax and Sales Tax: tax time. Mileage Log and Fixed Assets back them up.",
                    "Trial Balance and General Ledger: the detail your accountant may ask for.",
                ]),
                ("For your accountant", &[
                    "\"Accountant package…\" makes one folder for a quarter or year: every report as a PDF and CSVs, every journal line, and QuickBooks Online import files.",
                    "It first warns about anything unfinished: uncategorized transactions, accounts not reconciled through the period end, or books not locked.",
                    "The read-me inside lists those, plus the date each bank and card account is reconciled through.",
                ]),
            ],
        },
        "reconcile" => Topic {
            title: "Reconcile",
            intro: "Check your books against a bank or card statement, so you know nothing is missing or doubled.",
            sections: &[
                ("Steps", &[
                    "Pick the account, the statement date, and the statement's ending balance.",
                    "Tick each transaction that appears on the statement.",
                    "When the difference is $0.00, click Finish. Those transactions are locked in as cleared.",
                    "Off by an amount? Look for a missing or doubled transaction, or a typo in an amount.",
                ]),
            ],
        },
        "mileage" => Topic {
            title: "Mileage",
            intro: "Log business driving. At the IRS rate it's a tax deduction; it doesn't move money in your books.",
            sections: &[
                ("Tips", &[
                    "Log trips as you go; a log written months later is hard to defend.",
                    "Set this year's IRS rate on Business & Settings.",
                    "If you deduct mileage, don't also deduct actual gas and repairs for the same vehicle.",
                ]),
            ],
        },
        "assets" => Topic {
            title: "Assets",
            intro: "Tools, vehicles and equipment you'll use for years, grouped by category, with their value over time.",
            sections: &[
                ("Adding one", &[
                    "Pick the category (Equipment, Vehicles, Computers…), the cost, and how many years you'll use it.",
                    "\"Paid from\" books the purchase now. Leave it if you already recorded the purchase as a transaction.",
                    "\"+ New category\" adds your own category, like Tools or Trailers. Tick Intangible for things you can't touch, like software or a website.",
                    "Physical things are depreciated; intangibles (software, websites, domain names, licenses) are amortized into their own accounts.",
                    "Under $2,500? You can usually just expense it as Money out instead (the de minimis safe harbor). Track it here anyway if you want it on your insurance list.",
                    "\"+ Photo\" keeps a picture with each asset for insurance: the item, its serial plate or the receipt.",
                ]),
                ("Depreciation", &[
                    "Each year, click \"Book depreciation\" (usually at year end). It spreads the cost over the asset's life.",
                    "Tick Disposed when you sell or scrap something. Tax depreciation can differ; ask your preparer.",
                ]),
            ],
        },
        "import" => Topic {
            title: "Import & Rules",
            intro: "Bring in transactions from your bank, and let rules sort them for you.",
            sections: &[
                ("Bank CSV", &[
                    "Download a CSV from your bank's website, choose it here, and say which columns hold the date, description and amount.",
                    "Importing the same file twice is safe; duplicates are skipped.",
                ]),
                ("Rules", &[
                    "\"Contains SHELL → Car & Truck, vendor Shell\" sorts every matching import automatically.",
                    "Use \"Apply rules\" to sort what's already sitting in Uncategorized.",
                ]),
                ("Automatic bank feed", &["SimpleFIN Bridge (about $15 a year) downloads transactions for you. Connect once with a setup token."]),
            ],
        },
        "accounts" => Topic {
            title: "Chart of Accounts",
            intro: "The categories your money is sorted into. The standard list fits most small businesses; change it to suit yours.",
            sections: &[
                ("Tips", &[
                    "Rename or renumber anything; it saves when you click away.",
                    "Accounts with transactions keep their type. Switch them off to hide them instead of deleting.",
                    "\"Built in\" accounts are ones the app posts to itself (like Accounts Receivable). You can rename them.",
                ]),
            ],
        },
        "business" => Topic {
            title: "Business & Settings",
            intro: "Your details for invoices, rates, the books lock, backups, and who can sign in.",
            sections: &[
                ("Worth setting up", &[
                    "Company name, address, phones, email, website and logo appear on invoices, quotes, POs and proposals.",
                    "A payment link (Stripe, PayPal, Square) adds a Pay online button to invoices.",
                    "Lock the books through a date once your taxes are filed, so nothing in that year changes by accident.",
                    "A backup is made every time you open the app. \"Back up now\" saves an extra copy, say to a USB drive. Keep the .keys file that's saved next to it; the backup needs it to open.",
                ]),
                ("Logins & security", &[
                    "Your books file and its backups are encrypted. Without a login or the recovery key, nobody can read them, even with the file in hand.",
                    "Owners can do everything. Bookkeepers can do day-to-day work but can't open this screen. View-only people can look at everything but can't change anything.",
                    "To add someone, type their username and a password, choose their role, and Save login. Give them the password in person. Saving again for the same username resets their password.",
                    "The books lock themselves after 15 minutes without use. Click Lock in the sidebar to lock them sooner.",
                    "Forgot your password? On the sign-in screen, click \"Forgot your password?\" and enter your recovery key. Lost both? There is no back door, so keep the recovery key somewhere safe.",
                    "Open the books with the .db file and the .db.keys file side by side. Moving or copying the books? Move both.",
                ]),
            ],
        },
        _ => Topic {
            title: "Help & Tutorials",
            intro: "Short guided tours, and help for every screen. Press F1 anywhere for help on that screen.",
            sections: &[],
        },
    }
}

pub const TUTORIALS: &[Tutorial] = &[
    Tutorial {
        title: "Get set up (5 minutes)",
        blurb: "Your business details, your bank, and your first transaction.",
        steps: &[
            Step { screen: "business", title: "Your business", text: "Type your business name and address, then Save. Add your logo too; it goes on everything you send." },
            Step { screen: "accounts", title: "Your accounts", text: "Checking and Credit Card are ready. Rename them to match your real accounts, like \"Chase Checking\", or add more." },
            Step { screen: "transactions", title: "A starting balance", text: "Choose Money in, enter today's bank balance, and pick Opening Balance Equity as the category. Now your books match the bank." },
            Step { screen: "people", title: "Customers and vendors", text: "Add a customer or two with their email and address, so invoices can go straight to them." },
            Step { screen: "home", title: "That's it", text: "Home shows where you stand and what needs doing. Press F1 on any screen for help." },
        ],
    },
    Tutorial {
        title: "Your first invoice",
        blurb: "Quote, invoice, email it, and record the payment.",
        steps: &[
            Step { screen: "invoices", title: "Start an invoice", text: "Click \"+ New quote\". Pick the customer, then add a line for each thing you'll do or sell." },
            Step { screen: "invoices", title: "Quote first, then invoice", text: "Save the quote and send it. When they agree, \"Convert to invoice\" on the Quotes tab bills it. Or use \"+ New invoice\" to bill directly." },
            Step { screen: "invoices", title: "Send it", text: "In the Invoices tab, click Email. Your mail program opens with the PDF attached and the details filled in." },
            Step { screen: "invoices", title: "Get paid", text: "When the money arrives, click Record payment. Partial payments are fine; the balance updates." },
        ],
    },
    Tutorial {
        title: "Import from your bank",
        blurb: "Bring in a month of transactions and sort them fast.",
        steps: &[
            Step { screen: "import", title: "Get the file", text: "On your bank's website, download transactions as CSV. Back here, click Browse and choose it." },
            Step { screen: "import", title: "Columns", text: "Tell the app which column holds the date, description and amount, pick the account, and click Import." },
            Step { screen: "import", title: "Rules", text: "Add rules for things you buy often, like \"SHELL\" → Car & Truck. Then click \"Apply rules\" to sort them." },
            Step { screen: "transactions", title: "Finish sorting", text: "Tick \"Only uncategorized\" in the register and pick a category for what's left." },
        ],
    },
    Tutorial {
        title: "Track a job from quote to profit",
        blurb: "Quote it, buy materials, log time, bill it, and see the margin.",
        steps: &[
            Step { screen: "invoices", title: "Quote it", text: "Make a quote for the customer. Saving it creates a job with the same name." },
            Step { screen: "purchases", title: "Buy materials", text: "Make a purchase order for the job's materials. When it arrives, Mark received and the cost lands on the job." },
            Step { screen: "jobs", title: "Open the job", text: "Click the job's name. Log your hours under Time; you can bill them later with one click." },
            Step { screen: "jobs", title: "Bill and compare", text: "Turn the quote into an invoice, or Bill time. The tiles show quoted, invoiced, costs, profit and margin." },
        ],
    },
    Tutorial {
        title: "Reconcile a statement",
        blurb: "Prove your books match the bank, once a month.",
        steps: &[
            Step { screen: "reconcile", title: "Statement details", text: "Pick the account, the statement's end date, and its ending balance." },
            Step { screen: "reconcile", title: "Tick them off", text: "Tick each transaction that's on the statement. Watch the difference shrink." },
            Step { screen: "reconcile", title: "Finish", text: "At $0.00, click Finish reconciling. If it won't reach zero, look for a missing or doubled item." },
        ],
    },
    Tutorial {
        title: "Get ready for tax time",
        blurb: "The reports your preparer wants, and a clean year.",
        steps: &[
            Step { screen: "transactions", title: "Nothing uncategorized", text: "Tick \"Only uncategorized\" and sort everything left. Uncategorized money can't be deducted properly." },
            Step { screen: "assets", title: "Depreciation", text: "Book the year's depreciation on the Assets screen." },
            Step { screen: "reports", title: "The reports", text: "Choose Accountant package…, pick Full year, and fix anything it warns about. Then make the package and send the folder to your preparer." },
            Step { screen: "business", title: "Lock the year", text: "After you file, lock the books through Dec 31 so nothing in that year changes by accident." },
        ],
    },
];

/// Tutorials that visit a screen, for the "Learn more" links in its help.
pub fn tutorials_for(screen: &str) -> Vec<usize> {
    TUTORIALS.iter().enumerate().filter(|(_, t)| t.steps.iter().any(|s| s.screen == screen)).map(|(i, _)| i).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_tutorial_step_has_help() {
        for t in super::TUTORIALS {
            assert!(!t.steps.is_empty());
            for s in t.steps {
                assert_ne!(super::topic(s.screen).title, "Help & Tutorials", "unknown screen {}", s.screen);
            }
        }
    }
}
