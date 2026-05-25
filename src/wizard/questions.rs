use anyhow::Result;
use console::style;
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};

use super::state::*;
use crate::os_detect;
use crate::ui::prompt;

/// Ask all wizard questions and return a fully-populated BuildConfig.
pub async fn run() -> Result<BuildConfig> {
    let theme = ColorfulTheme::default();

    // ── Step 1: Modular Updater (Advanced) ───────────────────────────────────
    println!("{}", style("┌─ Step 1/8 — Build Type (Installer vs Updater) ──────────┐").bold().blue());
    println!();
    
    let is_updater = Confirm::with_theme(&theme)
        .with_prompt("Do you want to customize the components to include in this build? (e.g., to create a lightweight updater)")
        .default(false)
        .interact()?;
        
    let mut included = IncludedComponents {
        system_packages: true,
        ssl: true,
        xui_panel: true,
    };

    if is_updater {
        let items = vec![
            "System Packages (curl, socat, etc.)",
            "SSL Certificates (Setup & Generation)",
            "3x-ui Panel Binary",
        ];
        
        loop {
            let selections = dialoguer::MultiSelect::with_theme(&theme)
                .with_prompt("Select components to INCLUDE in the output bundle (Space to toggle, Enter to confirm)")
                .items(&items)
                .defaults(&[true, true, true])
                .interact()?;
                
            if selections.is_empty() {
                println!("  {} You must select at least one component!", style("✗").red());
                continue;
            }
            
            included.system_packages = selections.contains(&0);
            included.ssl = selections.contains(&1);
            included.xui_panel = selections.contains(&2);
            break;
        }
    }
    println!();

    // ── Step 2: OS ───────────────────────────────────────────────────────────
    println!("{}", style("┌─ Step 2/8 — Target Server Info ─────────────────────────┐").bold().blue());
    println!();

    let os_items = vec![
        "Ubuntu",
        "Debian",
        "CentOS",
        "Fedora",
        "AlmaLinux / Rocky Linux / RHEL",
        "Alpine Linux",
        "Arch Linux / Manjaro",
        "OpenSUSE",
    ];
    let os_sel = Select::with_theme(&theme)
        .with_prompt("Select Target Server OS")
        .items(&os_items)
        .default(0)
        .interact()?;

    let os = match os_sel {
        0 => TargetOs::Ubuntu,
        1 => TargetOs::Debian,
        2 => TargetOs::CentOs,
        3 => TargetOs::Fedora,
        4 => TargetOs::AlmaLinux,
        5 => TargetOs::Alpine,
        6 => TargetOs::Arch,
        7 => TargetOs::OpenSuse,
        _ => TargetOs::Ubuntu,
    };

    let os_version_str: String = Input::with_theme(&theme)
        .with_prompt("OS Version (Optional — e.g., 22.04 or 9)")
        .allow_empty(true)
        .interact_text()?;
    let os_version = if os_version_str.trim().is_empty() { None } else { Some(os_version_str.trim().to_string()) };

    let arch_items = vec![
        TargetArch::Amd64.display_name(),
        TargetArch::Arm64.display_name(),
        TargetArch::Armv7.display_name(),
        TargetArch::I386.display_name(),
        TargetArch::S390x.display_name(),
    ];
    let arch_sel = Select::with_theme(&theme)
        .with_prompt("Target Server CPU Architecture")
        .items(&arch_items)
        .default(0)
        .interact()?;

    let arch = match arch_sel {
        0 => TargetArch::Amd64,
        1 => TargetArch::Arm64,
        2 => TargetArch::Armv7,
        3 => TargetArch::I386,
        4 => TargetArch::S390x,
        _ => TargetArch::Amd64,
    };

    println!();

    // ── Step 3: Package mode ─────────────────────────────────────────────────
    let mut package_mode = PackageMode::Online;
    if included.system_packages {
        println!("{}", style("┌─ Step 3/8 — System Package Installation ────────────────┐").bold().blue());
        println!();
    
        let pkg_mode_items = vec![
            "Online — Target server has internet access",
            "Offline — Download packages now for air-gapped installation",
        ];
        let pkg_mode_sel = Select::with_theme(&theme)
            .with_prompt("Package Installation Mode")
            .items(&pkg_mode_items)
            .default(0)
            .interact()?;
    
        package_mode = if pkg_mode_sel == 1 {
            if !os_detect::supports_offline_packages(&os) {
                println!("\n  {} {}", style("⚠️").yellow(), style("This OS does not support full offline packages. Online mode will be used.").yellow());
                PackageMode::Online
            } else {
                PackageMode::Offline
            }
        } else {
            PackageMode::Online
        };
        println!();
    } else {
        println!("{}", style("┌─ Step 3/8 — System Package Installation (Skipped) ──────┐").bold().blue());
        println!("  {} Skipped due to updater settings.\n", style("⏭️").dim());
    }

    // ── Step 4: Server IP/Host ───────────────────────────────────────────────
    println!("{}", style("┌─ Step 4/8 — Target Server Address ──────────────────────┐").bold().blue());
    println!();
    let server_host: String = Input::with_theme(&theme)
        .with_prompt("Target Server IP or Domain (Used for displaying the access link)")
        .default("127.0.0.1".to_string())
        .interact_text()?;
    let server_host = server_host.trim().to_string();
    println!();

    // ── Step 5: x-ui version ─────────────────────────────────────────────────
    let mut xui_version = XuiVersion::Latest;
    if included.xui_panel {
        println!("{}", style("┌─ Step 5/8 — x-ui Version ───────────────────────────────┐").bold().blue());
        println!();
        let ver_items = vec!["Latest Version (GitHub)", "Specific Version"];
        let ver_sel = Select::with_theme(&theme).items(&ver_items).default(0).interact()?;
        xui_version = if ver_sel == 0 { XuiVersion::Latest } else {
            let v: String = Input::with_theme(&theme).with_prompt("Version (e.g. v2.5.1)").interact_text()?;
            XuiVersion::Specific(if v.starts_with('v') { v } else { format!("v{}", v) })
        };
        println!();
    } else {
        println!("{}", style("┌─ Step 5/8 — x-ui Version (Skipped) ─────────────────────┐").bold().blue());
        println!("  {} Skipped due to updater settings.\n", style("⏭️").dim());
    }

    // ── Step 6: Panel settings ───────────────────────────────────────────────
    let mut panel_port = 8080;
    let mut panel_username = "".to_string();
    let mut panel_password = "".to_string();
    let mut panel_web_base_path = "".to_string();
    
    if included.xui_panel {
        println!("{}", style("┌─ Step 6/8 — Panel Settings ─────────────────────────────┐").bold().blue());
        println!();
        panel_port = prompt::random_port();
        panel_username = prompt::random_string(8);
        panel_password = prompt::random_string(10);
        panel_web_base_path = prompt::random_string(12);
    
        println!("  {} Port:          {}", style("→").green(), style(panel_port).yellow().bold());
        println!("  {} Username:      {}", style("→").green(), style(&panel_username).yellow().bold());
        println!("  {} Password:      {}", style("→").green(), style(&panel_password).yellow().bold());
        println!("  {} Web Path:      /{}", style("→").green(), style(&panel_web_base_path).yellow().bold());
        println!("  {} Access Link:   http://{}:{}/{}", style("→").green(), style(&server_host).cyan(), style(panel_port).cyan(), style(&panel_web_base_path).cyan());
        println!();
    } else {
        println!("{}", style("┌─ Step 6/8 — Panel Settings (Skipped) ───────────────────┐").bold().blue());
        println!("  {} Skipped due to updater settings.\n", style("⏭️").dim());
    }

    // ── Step 7: SSL ──────────────────────────────────────────────────────────
    let mut ssl = SslConfig::None;
    if included.ssl {
        println!("{}", style("┌─ Step 7/8 — SSL Settings ───────────────────────────────┐").bold().blue());
        println!();
        let ssl_items = vec![
            "No SSL",
            "Custom SSL",
            "Self-Signed (Recommended)",
            "Let's Encrypt (For domains/subdomains only)"
        ];
        let ssl_sel = Select::with_theme(&theme).items(&ssl_items).default(2).interact()?;
        ssl = match ssl_sel {
            0 => SslConfig::None,
            1 => {
                let cert: String = Input::with_theme(&theme).with_prompt("Fullchain Path").interact_text()?;
                let key:  String = Input::with_theme(&theme).with_prompt("Privkey Path").interact_text()?;
                SslConfig::Custom { fullchain_path: cert.trim().into(), privkey_path: key.trim().into() }
            }
            2 => {
                let domain: String = Input::with_theme(&theme)
                    .with_prompt("Enter the IP or Domain for the certificate")
                    .default(server_host.clone())
                    .interact_text()?;
                let domain = domain.trim().to_string();
                
                println!();
                let dynamic = Confirm::with_theme(&theme)
                    .with_prompt("Make installer reusable? (Generate SSL certificate dynamically during installation on target server)")
                    .default(false)
                    .interact()?;
                SslConfig::SelfSigned { common_name: domain, dynamic }
            }
            _ => {
                let domain: String = Input::with_theme(&theme)
                    .with_prompt("Enter the Domain or Subdomain for Let's Encrypt")
                    .default(if server_host != "127.0.0.1" { server_host.clone() } else { "".to_string() })
                    .interact_text()?;
                let domain = domain.trim().to_string();
                
                println!();
                let is_ip = domain.split('.').count() == 4 && domain.split('.').all(|part| part.parse::<u8>().is_ok());
                if is_ip {
                    println!("  {} Let's Encrypt requires a valid domain or subdomain, not an IP.", style("✗").red());
                    anyhow::bail!("Invalid domain for Let's Encrypt.");
                }
                SslConfig::LetsEncrypt { domain }
            }
        };
        println!();
    } else {
        println!("{}", style("┌─ Step 7/8 — SSL Settings (Skipped) ─────────────────────┐").bold().blue());
        println!("  {} Skipped due to updater settings.\n", style("⏭️").dim());
    }

    // ── Step 8: Output Kind ──────────────────────────────────────────────────
    println!("{}", style("┌─ Step 8/8 — Output File Type ───────────────────────────┐").bold().blue());
    println!();
    let out_items = vec![
        "Self-Extracting (.sh) — Single file, easiest to transfer (Recommended)",
        "Normal Folder — Includes all files separately",
    ];
    let out_sel = Select::with_theme(&theme).with_prompt("How would you like to receive the bundle?").items(&out_items).default(0).interact()?;
    let output_kind = if out_sel == 0 { OutputKind::Sfx } else { OutputKind::Folder };

    let output_dir: String = Input::with_theme(&theme)
        .with_prompt("Storage Path")
        .default("./xui-bundle".into())
        .validate_with(|input: &String| -> Result<(), &str> {
            let path = input.trim();
            if path.is_empty() { return Err("Path cannot be empty"); }
            
            let p = std::path::Path::new(path);
            
            // Check if we can write to the parent directory
            let mut current = p;
            while let Some(parent) = current.parent() {
                if parent.as_os_str().is_empty() { break; }
                if parent.exists() {
                    if parent.is_dir() {
                        // Check if writable
                        if let Ok(metadata) = std::fs::metadata(parent) {
                            if metadata.permissions().readonly() {
                                return Err("Selected path is in a read-only directory");
                            }
                        }
                        // Try to create a temporary directory inside to be 100% sure
                        let test_dir = parent.join(".xui_write_test");
                        if std::fs::create_dir(&test_dir).is_err() {
                            return Err("Permission denied: Cannot write to this directory");
                        }
                        let _ = std::fs::remove_dir(test_dir);
                        return Ok(());
                    } else {
                        return Err("Parent path exists but is not a directory");
                    }
                }
                current = parent;
            }

            // If we reached here, it's either relative or root
            if path.starts_with('/') {
                // Testing root access
                if std::fs::metadata("/").is_ok() {
                    let test_dir = std::path::Path::new("/.xui_write_test");
                    if std::fs::create_dir(test_dir).is_err() {
                        return Err("Permission denied: You do not have write access to the root (/) directory. Use a relative path like ./output");
                    }
                    let _ = std::fs::remove_dir(test_dir);
                }
            }

            Ok(())
        })
        .interact_text()?;

    let output_dir = output_dir.trim().to_string();
    let abs_path = std::fs::canonicalize(std::path::Path::new(&output_dir))
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default().join(&output_dir));
    
    println!("  {} Will save to: {}", style("→").dim(), style(abs_path.display()).cyan());

    // ── Final Confirm ────────────────────────────────────────────────────────
    println!("\n{}", style("━".repeat(50)).dim());
    let ok = Confirm::with_theme(&theme).with_prompt("Are you sure about the settings above?").default(true).interact()?;
    if !ok { anyhow::bail!("Cancelled."); }

    Ok(BuildConfig {
        os, arch, os_version, package_mode, server_host, xui_version,
        panel_port, panel_username, panel_password, panel_web_base_path,
        ssl, output_dir: output_dir.trim().to_string(), output_kind, proxy: None,
        included,
    })
}
