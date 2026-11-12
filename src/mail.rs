//! Email with a PDF attached, by asking a mail program to open a new message with it attached:
//! Outlook (through PowerShell) or Thunderbird on Windows, Apple Mail on macOS, and on Linux
//! xdg-email, which uses the desktop's own mail program. When none of them answers (for example
//! webmail users), the caller opens a plain mailto and shows the PDF in the file manager.

use std::path::Path;
use std::process::Command;

/// Opens a new message with `attachment` attached for the user to review and send. Err when no
/// mail program took it.
pub fn send_with_attachment(to: &str, subject: &str, body: &str, attachment: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // The values go in through environment variables, so no quoting can break the script.
        const OUTLOOK: &str = "$m = (New-Object -ComObject Outlook.Application).CreateItem(0); \
            $m.To = $env:AC_TO; $m.Subject = $env:AC_SUBJECT; $m.Body = $env:AC_BODY; \
            [void]$m.Attachments.Add($env:AC_FILE); $m.Display()";
        let outlook = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", OUTLOOK])
            .env("AC_TO", to)
            .env("AC_SUBJECT", subject)
            .env("AC_BODY", body)
            .env("AC_FILE", attachment)
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW: no console flash
            .status();
        if outlook.is_ok_and(|s| s.success()) {
            return Ok(());
        }
        let tb = app_path("thunderbird.exe").ok_or("no mail program took the attachment")?;
        Command::new(tb).arg("-compose").arg(thunderbird(to, subject, body, attachment)).spawn().map(|_| ()).map_err(|e| e.to_string())
    }
    #[cfg(target_os = "macos")]
    {
        const MAIL: &str = r#"on run argv
    tell application "Mail"
        set m to make new outgoing message with properties {subject:item 2 of argv, content:item 3 of argv, visible:true}
        tell m
            make new to recipient at end of to recipients with properties {address:item 1 of argv}
            make new attachment with properties {file name:(POSIX file (item 4 of argv)) as alias} at after the last paragraph
        end tell
        activate
    end tell
end run"#;
        ok(Command::new("osascript").arg("-e").arg(MAIL).arg(to).arg(subject).arg(body).arg(attachment).status())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        ok(Command::new("xdg-email").arg("--subject").arg(subject).arg("--body").arg(body).arg("--attach").arg(attachment).arg(to).status())
    }
}

#[cfg(unix)]
fn ok(status: std::io::Result<std::process::ExitStatus>) -> Result<(), String> {
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("mail program refused ({s})")),
        Err(e) => Err(e.to_string()),
    }
}

/// Thunderbird's `-compose` value. Values sit in single quotes, so a straight apostrophe becomes a
/// curly one rather than ending the value early.
#[cfg_attr(not(windows), allow(dead_code))]
fn thunderbird(to: &str, subject: &str, body: &str, attachment: &Path) -> String {
    let q = |s: &str| s.replace('\'', "\u{2019}");
    let url = format!("file:///{}", attachment.display().to_string().replace('\\', "/").trim_start_matches('/'));
    format!("to='{}',subject='{}',body='{}',attachment='{}'", q(to), q(subject), q(body), q(&url))
}

/// Where an installed program lives, from Windows' App Paths registry entry.
#[cfg(windows)]
fn app_path(exe: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    ["HKCU", "HKLM"].iter().find_map(|hive| {
        let key = format!(r"{hive}\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{exe}");
        let out = Command::new("reg").args(["query", &key, "/ve"]).creation_flags(0x0800_0000).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let path = text.lines().find_map(|l| l.split_once("REG_SZ").map(|(_, p)| p.trim().trim_matches('"').to_string()))?;
        Path::new(&path).exists().then_some(path)
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn thunderbird_compose_value() {
        let v = super::thunderbird("a@b.com", "Invoice from Smith's", "Hi,\nThanks", std::path::Path::new(r"C:\Temp\INV-1.pdf"));
        assert_eq!(v, "to='a@b.com',subject='Invoice from Smith\u{2019}s',body='Hi,\nThanks',attachment='file:///C:/Temp/INV-1.pdf'");
    }
}
