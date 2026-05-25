use anyhow::Result;
use console::style;
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};
use std::fs;
use std::path::Path;

use crate::manifest::{
    Manifest, StepStatus, MANIFEST_FILE,
    STEP_INSTALL_SH, STEP_PACKAGES, STEP_SERVICE_FILE,
    STEP_SSL, STEP_XUI_BINARY, STEP_XUI_SH,
};
use crate::wizard::state::BuildConfig;
use crate::generator;

// ─── Public API ──────────────────────────────────────────────────────────────

/// What the user chose to do with an existing bundle.
#[derive(Debug)]
pub enum ResumeAction {
    Continue(Manifest),
    Edited(Manifest, bool /* needs_redownload */),
    Restart,
    Exit,
}

/// Check whether a bundle already exists in `dir`.
pub fn detect_existing_bundle(dir: &str) -> Option<Manifest> {
    let manifest_path = format!("{}/{}", dir, MANIFEST_FILE);
    if !Path::new(&manifest_path).exists() {
        return None;
    }
    Manifest::load(dir).ok()
}

/// Show the resume UI and return what the user wants to do.
pub fn run_resume_mode(dir: &str, manifest: Manifest) -> Result<ResumeAction> {
    let theme = ColorfulTheme::default();

    // ── Header ───────────────────────────────────────────────────────────────
    println!("{}", style("━".repeat(54)).yellow());
    println!("{}", style("  📦  Existing bundle found!").yellow().bold());
    println!("{}", style("━".repeat(54)).yellow());

    if let Some(cfg) = manifest.config.as_object() {
        let get = |k: &str| cfg.get(k).and_then(|v| v.as_str()).unwrap_or("?");
        println!("  {:20} {}", style("OS:").dim(),            style(get("os")).cyan());
        println!("  {:20} {}", style("Architecture:").dim(),  style(get("arch")).cyan());
        println!("  {:20} {}", style("x-ui Version:").dim(),  style(get("xui_version")).cyan());
        println!("  {:20} {}", style("Panel Port:").dim(),   style(get("panel_port")).yellow());
        println!("  {:20} {}", style("Username:").dim(),     style(get("panel_username")).yellow());
        println!("  {:20} {}", style("SSL:").dim(),          style(get("ssl_kind")).cyan());
        println!(
            "  {:20} {}",
            style("Created At:").dim(),
            style(manifest.created_at.format("%Y-%m-%d %H:%M UTC").to_string()).dim()
        );
    }
    println!();

    // ── Verify ───────────────────────────────────────────────────────────────
    println!("{}", style("  🔍  Verifying bundle integrity...").bold());
    println!("{}", style("  ─────────────────────────────────────────").dim());

    let (all_ok, any_missing) = show_verify_report(dir, &manifest);

    println!("{}", style("  ─────────────────────────────────────────").dim());
    println!();

    if all_ok {
        println!("  {} All files are healthy.", style("✅").green());
    } else {
        println!(
            "  {} Some files are missing or corrupted.",
            style("⚠️").yellow()
        );
    }
    println!();

    // ── Resume menu ───────────────────────────────────────────────────────────
    let mut menu_items: Vec<&str> = Vec::new();
    if any_missing {
        menu_items.push("Continue download — complete missing/incomplete items");
    }
    menu_items.push("Edit settings — Port / Username / Password / SSL");
    menu_items.push("Full restart — delete bundle and start from scratch");
    menu_items.push("Exit");

    let sel = Select::with_theme(&theme)
        .with_prompt("What would you like to do?")
        .items(&menu_items)
        .default(0)
        .interact()?;

    let mut idx = sel;
    if any_missing {
        if idx == 0 {
            return Ok(ResumeAction::Continue(manifest));
        }
        idx -= 1;
    }

    match idx {
        0 => {
            println!();
            let (updated, needs_dl) = edit_settings(dir, manifest)?;
            Ok(ResumeAction::Edited(updated, needs_dl))
        }
        1 => {
            println!();
            let ok = Confirm::with_theme(&theme)
                .with_prompt(&format!(
                    "Are you sure? The folder {} will be completely deleted.",
                    style(dir).yellow()
                ))
                .default(false)
                .interact()?;

            if ok {
                fs::remove_dir_all(dir)?;
                println!("  {} Previous bundle deleted.", style("✓").green());
                Ok(ResumeAction::Restart)
            } else {
                Ok(ResumeAction::Exit)
            }
        }
        _ => Ok(ResumeAction::Exit),
    }
}

// ─── Verify report ────────────────────────────────────────────────────────────

pub fn show_verify_report(dir: &str, manifest: &Manifest) -> (bool, bool) {
    let steps = [
        (STEP_XUI_BINARY,   "x-ui binary (.tar.gz) "),
        (STEP_XUI_SH,       "x-ui.sh (CLI manager) "),
        (STEP_SERVICE_FILE, "Service/RC file        "),
        (STEP_PACKAGES,     "Offline packages       "),
        (STEP_SSL,          "SSL (cert/key)         "),
        (STEP_INSTALL_SH,   "install.sh             "),
    ];

    let mut all_ok      = true;
    let mut any_missing = false;

    for (key, label) in &steps {
        let step = manifest.steps.get(*key);

        // Detect "skipped" steps (Done with no files = online mode / no-ssl)
        if let Some(s) = step {
            if s.status == StepStatus::Done && s.files.is_empty() {
                let reason = if *key == STEP_PACKAGES { "Online Mode" } else { "No SSL" };
                println!(
                    "  {} {}  {}",
                    style("⏭️").dim(),
                    style(label).dim(),
                    style(format!("Skipped ({})", reason)).dim()
                );
                continue;
            }
        }

        let valid = manifest.step_is_valid(dir, key);

        match step.map(|s| &s.status) {
            Some(StepStatus::Done) if valid => {
                println!(
                    "  {} {}  {}",
                    style("✅").green(),
                    style(label).bold(),
                    style(step.unwrap().files.join(", ")).dim()
                );
            }
            Some(StepStatus::Done) => {
                println!(
                    "  {} {}  {}",
                    style("❌").red(),
                    style(label).bold(),
                    style("File missing or corrupted").red()
                );
                all_ok      = false;
                any_missing = true;
            }
            Some(StepStatus::Partial) => {
                let note = step.and_then(|s| s.note.as_deref()).unwrap_or("");
                println!(
                    "  {} {}  {}",
                    style("⚠️ ").yellow(),
                    style(label).bold(),
                    style(format!("Incomplete — {}", note)).yellow()
                );
                all_ok      = false;
                any_missing = true;
            }
            Some(StepStatus::Failed) => {
                let note = step.and_then(|s| s.note.as_deref()).unwrap_or("Unknown");
                println!(
                    "  {} {}  {}",
                    style("❌").red(),
                    style(label).bold(),
                    style(format!("Failed — {}", note)).red()
                );
                all_ok      = false;
                any_missing = true;
            }
            _ => {
                println!(
                    "  {} {}  {}",
                    style("🔲").dim(),
                    style(label).dim(),
                    style("Not done").dim()
                );
                any_missing = true;
            }
        }
    }

    (all_ok, any_missing)
}

// ─── Edit Mode ────────────────────────────────────────────────────────────────

fn edit_settings(dir: &str, mut manifest: Manifest) -> Result<(Manifest, bool)> {
    let theme = ColorfulTheme::default();

    println!("{}", style("┌─ Edit Settings ──────────────────────────────────────────┐").bold().blue());
    println!();
    println!("  {}", style("Note: Changing Port/User/Pass/SSL only regenerates install.sh.").dim());
    println!();

    let mut needs_redownload = false;

    loop {
        let edit_items = vec![
            "Change Panel Port",
            "Change Username",
            "Change Password",
            "Change SSL",
            "Regenerate install.sh and Save",
            "Back",
        ];

        let sel = Select::with_theme(&theme)
            .with_prompt("Which setting?")
            .items(&edit_items)
            .default(4)
            .interact()?;

        match sel {
            0 => {
                let p: String = Input::with_theme(&theme)
                    .with_prompt("New Port (1024-65535)")
                    .interact_text()?;
                if let Ok(n) = p.trim().parse::<u16>() {
                    if n >= 1024 {
                        if let Some(obj) = manifest.config.as_object_mut() {
                            obj.insert("panel_port".to_string(), serde_json::json!(n));
                        }
                        println!("  {} Port → {}", style("✓").green(), n);
                    }
                }
            }
            1 => {
                let u: String = Input::with_theme(&theme)
                    .with_prompt("New Username")
                    .interact_text()?;
                if let Some(obj) = manifest.config.as_object_mut() {
                    obj.insert("panel_username".to_string(), serde_json::json!(u.trim()));
                }
                println!("  {} Username updated.", style("✓").green());
            }
            2 => {
                let p: String = Input::with_theme(&theme)
                    .with_prompt("New Password")
                    .interact_text()?;
                if let Some(obj) = manifest.config.as_object_mut() {
                    obj.insert("panel_password".to_string(), serde_json::json!(p.trim()));
                }
                println!("  {} Password updated.", style("✓").green());
            }
            3 => {
                let ssl_items = vec!["No SSL", "Self-Signed", "Custom (Existing files)"];
                let ss = Select::with_theme(&theme)
                    .with_prompt("New SSL Type")
                    .items(&ssl_items)
                    .default(0)
                    .interact()?;

                let ssl_kind = match ss {
                    0 => "none".to_string(),
                    1 => {
                        let cn: String = Input::with_theme(&theme)
                            .with_prompt("IP or Domain for Self-Signed")
                            .interact_text()?;
                        needs_redownload = true;
                        format!("self-signed({})", cn.trim())
                    }
                    _ => {
                        let cert: String = Input::with_theme(&theme).with_prompt("Path to fullchain.pem").interact_text()?;
                        let key:  String = Input::with_theme(&theme).with_prompt("Path to privkey.pem").interact_text()?;
                        let ssl_dir = format!("{}/ssl", dir);
                        fs::create_dir_all(&ssl_dir)?;
                        fs::copy(cert.trim(), format!("{}/fullchain.pem", ssl_dir))?;
                        fs::copy(key.trim(),  format!("{}/privkey.pem",   ssl_dir))?;
                        println!("  {} SSL files copied.", style("✓").green());
                        "custom".to_string()
                    }
                };
                if let Some(obj) = manifest.config.as_object_mut() {
                    obj.insert("ssl_kind".to_string(), serde_json::json!(ssl_kind));
                }
                if let Some(s) = manifest.steps.get_mut(STEP_SSL) {
                    s.status = StepStatus::Pending;
                    s.files.clear();
                    s.sha256.clear();
                }
                println!("  {} SSL updated.", style("✓").green());
            }
            4 => {
                // Regenerate install.sh
                match config_from_manifest(&manifest, dir) {
                    Ok(cfg) => {
                        let rt = tokio::runtime::Handle::current();
                        let resolved_ver = manifest.config["xui_version"]
                            .as_str()
                            .unwrap_or("latest");
                        rt.block_on(generator::build(&cfg, resolved_ver))?;
                        if let Some(s) = manifest.steps.get_mut(STEP_INSTALL_SH) {
                            s.status = StepStatus::Done;
                            s.files  = vec!["install.sh".to_string()];
                        }
                        manifest.save(dir)?;
                        println!("  {} install.sh regenerated.", style("✓").green());
                    }
                    Err(e) => {
                        println!("  {} Error during regeneration: {}", style("✗").red(), e);
                    }
                }
                break;
            }
            _ => break,
        }
    }

    manifest.save(dir)?;
    Ok((manifest, needs_redownload))
}

// ─── Config reconstruction from manifest ─────────────────────────────────────

/// Reconstruct a BuildConfig from the JSON snapshot stored in manifest.config.
pub fn config_from_manifest(manifest: &Manifest, dir: &str) -> Result<BuildConfig> {
    use crate::wizard::state::*;

    let cfg = manifest.config.as_object()
        .ok_or_else(|| anyhow::anyhow!("manifest.config has an invalid structure"))?;

    let get_str = |k: &str| -> String {
        cfg.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string()
    };
    let get_u64 = |k: &str| -> u64 {
        cfg.get(k).and_then(|v| v.as_u64()).unwrap_or(0)
    };

    let os = match get_str("os").as_str() {
        "Ubuntu"       => TargetOs::Ubuntu,
        "Debian"       => TargetOs::Debian,
        "CentOS"       => TargetOs::CentOs,
        "Fedora"       => TargetOs::Fedora,
        "AlmaLinux"    => TargetOs::AlmaLinux,
        "Rocky Linux"  => TargetOs::Rocky,
        "RHEL"         => TargetOs::Rhel,
        "Alpine Linux" => TargetOs::Alpine,
        "Arch Linux"   => TargetOs::Arch,
        "Manjaro"      => TargetOs::Manjaro,
        "OpenSUSE"     => TargetOs::OpenSuse,
        _              => TargetOs::Ubuntu,
    };

    let arch = match get_str("arch").as_str() {
        "arm64" => TargetArch::Arm64,
        "armv7" => TargetArch::Armv7,
        "386"   => TargetArch::I386,
        "s390x" => TargetArch::S390x,
        _       => TargetArch::Amd64,
    };

    let package_mode = if get_str("package_mode") == "offline" {
        PackageMode::Offline
    } else {
        PackageMode::Online
    };

    let xui_version = match get_str("xui_version").as_str() {
        "latest" => XuiVersion::Latest,
        v        => XuiVersion::Specific(v.to_string()),
    };

    let ssl_str = get_str("ssl_kind");
    let ssl = if ssl_str == "none" {
        SslConfig::None
    } else if ssl_str == "custom" {
        SslConfig::Custom {
            fullchain_path: format!("{}/ssl/fullchain.pem", dir),
            privkey_path:   format!("{}/ssl/privkey.pem",   dir),
        }
    } else if ssl_str.starts_with("self-signed(") {
        let inner = ssl_str
            .trim_start_matches("self-signed(")
            .trim_end_matches(')');
        let mut parts = inner.split('|');
        let cn = parts.next().unwrap_or("").to_string();
        let dynamic = parts.next().unwrap_or("false") == "true";
        SslConfig::SelfSigned { common_name: cn, dynamic }
    } else if ssl_str.starts_with("letsencrypt(") {
        let domain = ssl_str
            .trim_start_matches("letsencrypt(")
            .trim_end_matches(')')
            .to_string();
        SslConfig::LetsEncrypt { domain }
    } else {
        SslConfig::None
    };

    let sys_pkgs = cfg.get("included_system_packages").and_then(|v| v.as_bool()).unwrap_or(true);
    let ssl_inc = cfg.get("included_ssl").and_then(|v| v.as_bool()).unwrap_or(true);
    let xui_inc = cfg.get("included_xui_panel").and_then(|v| v.as_bool()).unwrap_or(true);

    Ok(BuildConfig {
        os,
        arch,
        os_version:          cfg.get("os_version").and_then(|v| v.as_str()).map(|s| s.to_string()),
        package_mode,
        xui_version,
        panel_port:          get_u64("panel_port") as u16,
        panel_username:      get_str("panel_username"),
        panel_password:      get_str("panel_password"),
        panel_web_base_path: get_str("panel_web_base_path"),
        ssl,
        server_host:         get_str("server_host"),
        proxy:               None,
        output_dir:          dir.to_string(),
        output_kind:         if get_str("output_kind") == "sfx" { OutputKind::Sfx } else { OutputKind::Folder },
        included: crate::wizard::state::IncludedComponents {
            system_packages: sys_pkgs,
            ssl: ssl_inc,
            xui_panel: xui_inc,
        },
    })
}
