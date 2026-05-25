use anyhow::Result;
use console::style;
use std::fs;

use crate::os_detect::{self, PkgFormat};
use crate::wizard::state::{BuildConfig, PackageMode, SslConfig, TargetOs};

/// Render the offline install.sh from the config and write it to the bundle.
pub fn render(config: &BuildConfig, resolved_version: &str) -> Result<()> {
    let script = build_script(config, resolved_version);
    let dest = format!("{}/install.sh", config.output_dir);
    fs::write(&dest, &script)?;

    // Make it executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&dest)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&dest, perms)?;
    }

    println!(
        "  {} install.sh generated → {}",
        style("✓").green(),
        style(&dest).yellow().bold()
    );
    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────


fn build_script(c: &BuildConfig, resolved_version: &str) -> String {
    let arch_suffix = c.arch.xui_suffix();

    // ── Package section ───────────────────────────────────────────────────────
    let pkg_section = build_pkg_section(c);
    let install_call = match c.package_mode {
        PackageMode::Online  => "install_base_online",
        PackageMode::Offline => "install_base_offline",
    };

    // ── SSL section ───────────────────────────────────────────────────────────
    let ssl_section = build_ssl_section(c);
    let ssl_call = match &c.ssl {
        SslConfig::None => "# SSL is disabled".to_string(),
        _               => "setup_ssl".to_string(),
    };

    // ── Service management (Alpine vs systemd) ────────────────────────────────
    let is_alpine = c.os == TargetOs::Alpine;
    let (service_stop, service_install) = build_service_section(is_alpine);

    // ── CentOS VERSION_ID ─────────────────────────────────────────────────────
    let version_id_line = if c.os == TargetOs::CentOs {
        format!("VERSION_ID=\"{}\"", c.os_version.as_deref().unwrap_or(""))
    } else {
        String::new()
    };

    // ── Assemble final script ─────────────────────────────────────────────────
    let mut s = String::new();

    s.push_str("#!/bin/bash\n");
    s.push_str("# ============================================================\n");
    s.push_str(&format!("# 3x-ui Offline Installer — Customized by xui-offline-builder\n"));
    s.push_str(&format!("# Target OS      : {}\n", c.os.display_name()));
    s.push_str(&format!("# Architecture   : {}\n", arch_suffix));
    s.push_str("# ============================================================\n");
    s.push_str("set -e\n\n");

    s.push_str("red='\\033[0;31m'\n");
    s.push_str("green='\\033[0;32m'\n");
    s.push_str("blue='\\033[0;34m'\n");
    s.push_str("yellow='\\033[0;33m'\n");
    s.push_str("plain='\\033[0m'\n\n");

    s.push_str("# Bundle path\n");
    s.push_str("BUNDLE_DIR=\"$(cd \"$(dirname \"${BASH_SOURCE[0]}\")\" && pwd)\"\n\n");

    s.push_str("xui_folder=\"/usr/local/x-ui\"\n");
    s.push_str("xui_service=\"/etc/systemd/system\"\n");

    if !version_id_line.is_empty() {
        s.push('\n');
        s.push_str(&version_id_line);
        s.push('\n');
    }
    s.push_str("# ── Root Check ──────────────────────────────────────────────\n");
    s.push_str("[[ $EUID -ne 0 ]] && echo -e \"${red}Error: This script must be run as root.${plain}\" && exit 1\n\n");

    s.push_str("# ── Safety Confirmation & Detection ─────────────────────────\n");
    s.push_str(&format!("echo -e \"${{blue}}This is a 3x-ui Offline Installer (${{cyan}}{}${{blue}}).${{plain}}\"\n", resolved_version));
    s.push_str("echo -e \"It will install the panel and all dependencies from local files.\"\n\n");

    s.push_str("ACTION=\"install\"\n");
    s.push_str("if [[ -d \"$xui_folder\" ]]; then\n");
    s.push_str("    # Try to detect current version\n");
    s.push_str("    current_v=$(\"$xui_folder/x-ui\" v 2>/dev/null | grep -oE '[0-9]+\\.[0-9]+\\.[0-9]+' | head -n1 || echo \"Unknown\")\n");
    s.push_str("    echo -e \"${yellow}⚠️  Existing 3x-ui installation detected!${plain}\"\n");
    let bundle_v = resolved_version.trim_start_matches('v');
    s.push_str(&format!("    echo -e \"   Installed Version: ${{cyan}}v${{current_v#v}}${{plain}}\"\n"));
    s.push_str(&format!("    echo -e \"   Bundle Version:    ${{cyan}}v{}${{plain}}\"\n\n", bundle_v));
    let has_ssl = c.included.ssl && !matches!(c.ssl, SslConfig::None);
    let has_panel = c.included.xui_panel;
    
    s.push_str("    echo -e \"What would you like to do?\"\n");
    let mut opt_idx = 1;
    let mut script_opts = String::new();
    
    if has_panel {
        s.push_str(&format!("    echo -e \"  ${{cyan}}[{}] Update${{plain}} (Keep database, settings, and users)\"\n", opt_idx));
        script_opts.push_str(&format!("        {}) ACTION=\"update\" ;;\n", opt_idx));
        opt_idx += 1;
        
        s.push_str(&format!("    echo -e \"  ${{cyan}}[{}] Reinstall${{plain}} (Clean install, overwrite everything)\"\n", opt_idx));
        script_opts.push_str(&format!("        {}) ACTION=\"reinstall\" ;;\n", opt_idx));
        opt_idx += 1;
    }
    
    if has_ssl {
        s.push_str(&format!("    echo -e \"  ${{cyan}}[{}] Update SSL Only${{plain}} (Replace SSL certificates only)\"\n", opt_idx));
        script_opts.push_str(&format!("        {}) ACTION=\"update_ssl\" ;;\n", opt_idx));
        opt_idx += 1;
    }
    
    s.push_str(&format!("    echo -e \"  ${{cyan}}[{}] Abort${{plain}}\"\n", opt_idx));
    
    s.push_str(&format!("    read -p \"Choose an option [1-{}]: \" opt\n", opt_idx));
    s.push_str("    case $opt in\n");
    s.push_str(&script_opts);
    s.push_str("        *) echo -e \"${red}Aborted.${plain}\" ; exit 0 ;;\n");
    s.push_str("    esac\n");
    s.push_str("else\n");
    if !has_panel {
        s.push_str("    echo -e \"${red}Error: No existing 3x-ui installation found to update.${plain}\"\n");
        s.push_str("    exit 1\n");
    } else {
        s.push_str("    read -p \"🚀 Do you want to start the installation? [y/N]: \" confirm\n");
        s.push_str("    if [[ ! \"$confirm\" =~ ^[Yy]$ ]]; then\n");
        s.push_str("        echo -e \"${red}Installation aborted.${plain}\"\n");
        s.push_str("        exit 0\n");
        s.push_str("    fi\n");
    }
    s.push_str("fi\n\n");

    s.push_str("echo -e \"${green}Executing $ACTION process...${plain}\"\n\n");

    // Package functions
    if c.included.system_packages {
        s.push_str("if [[ \"$ACTION\" != \"update_ssl\" ]]; then\n");
        s.push_str("# ── System Package Installation ─────────────────────────────\n");
        s.push_str(&pkg_section);
        s.push_str("\n\n");
        s.push_str(&format!("{}\n\n", install_call));
        s.push_str("fi\n\n");
    }

    // Stop old service
    s.push_str("# ── Stopping Previous Service ───────────────────────────────\n");
    s.push_str(&service_stop);
    s.push_str("\n");
    s.push_str("if [[ \"$ACTION\" == \"reinstall\" ]]; then\n");
    s.push_str("    echo -e \"${yellow}Cleaning previous installation...${plain}\"\n");
    s.push_str("    rm -rf \"$xui_folder\" 2>/dev/null || true\n");
    s.push_str("fi\n\n");

    // Extract binary
    s.push_str("# ── Extracting x-ui Binary ──────────────────────────────────\n");
    s.push_str("echo -e \"${green}Installing x-ui binary...${plain}\"\n");
    s.push_str("mkdir -p \"$(dirname \"$xui_folder\")\"\n");
    s.push_str(&format!(
        "tar zxf \"$BUNDLE_DIR/x-ui-linux-{}.tar.gz\" -C \"$(dirname \"$xui_folder\")\"\n",
        arch_suffix
    ));
    s.push_str("mv \"$(dirname \"$xui_folder\")/x-ui\" \"$xui_folder\" 2>/dev/null || true\n");
    s.push_str("chmod +x \"$xui_folder/x-ui\"\n");
    s.push_str("chmod +x \"$xui_folder/x-ui.sh\"\n");
    if arch_suffix == "armv7" {
        s.push_str("mv \"$xui_folder/bin/xray-linux-armv7\" \"$xui_folder/bin/xray-linux-arm\" 2>/dev/null || true\n");
    }
    s.push_str("chmod +x \"$xui_folder/bin/\"* 2>/dev/null || true\n\n");

    // CLI manager
    s.push_str("# ── Installing CLI manager ──────────────────────────────────\n");
    s.push_str("cp \"$BUNDLE_DIR/x-ui.sh\" /usr/bin/x-ui\n");
    s.push_str("chmod +x /usr/bin/x-ui\n");
    s.push_str("mkdir -p /var/log/x-ui\n\n");

    // Panel config
    s.push_str("# ── Panel Configuration ─────────────────────────────────────\n");
    s.push_str("if [[ \"$ACTION\" != \"update\" ]]; then\n");
    s.push_str("    echo -e \"${green}Configuring panel settings...${plain}\"\n");
    s.push_str(&format!(
        "    \"$xui_folder/x-ui\" setting -username \"{}\" -password \"{}\" -port \"{}\" -webBasePath \"{}\" > /dev/null 2>&1\n",
        c.panel_username, c.panel_password, c.panel_port, c.panel_web_base_path
    ));
    s.push_str("else\n");
    s.push_str("    echo -e \"${blue}Updating binary only. Existing settings preserved.${plain}\"\n");
    s.push_str("fi\n");
    s.push_str("fi\n\n");

    // SSL
    s.push_str("# ── SSL Configuration ───────────────────────────────────────\n");
    s.push_str(&ssl_section);
    s.push('\n');
    s.push_str(&ssl_call);
    s.push_str("\n\n");

    // Service install
    s.push_str("# ── Service Installation & Activation ───────────────────────\n");
    s.push_str("echo -e \"${green}Activating x-ui service...${plain}\"\n");
    s.push_str(&service_install);
    s.push_str("\n\n");

    // etckeeper
    s.push_str("# etckeeper compatibility\n");
    s.push_str("if [ -d \"/etc/.git\" ]; then\n");
    s.push_str("    echo \"x-ui/x-ui.db\" >> /etc/.gitignore 2>/dev/null || true\n");
    s.push_str("fi\n\n");

    // Final output
    let protocol = match c.ssl { SslConfig::None => "http", _ => "https" };
    let host_var = match c.ssl {
        SslConfig::SelfSigned { dynamic: true, .. } => "$TARGET_HOST",
        _ => &c.server_host,
    };
    let access_link = format!("{}://{}:{}/{}", protocol, host_var, c.panel_port, c.panel_web_base_path);

    s.push_str("echo \"\"\n");
    s.push_str("if [[ \"$ACTION\" == \"update_ssl\" ]]; then\n");
    s.push_str("    echo -e \"${green}╔════════════════════════════════════════════════════════════╗${plain}\"\n");
    s.push_str("    echo -e \"${green}║            SSL Certificates updated successfully!          ║${plain}\"\n");
    s.push_str("    echo -e \"${green}╠════════════════════════════════════════════════════════════╣${plain}\"\n");
    s.push_str(&format!("echo -e \"${{green}}║ Access Link:   {:<43} ║${{plain}}\"\n", access_link));
    s.push_str("    echo -e \"${green}╚════════════════════════════════════════════════════════════╝${plain}\"\n");
    s.push_str("elif [[ \"$ACTION\" == \"update\" ]]; then\n");
    s.push_str("    echo -e \"${green}╔════════════════════════════════════════════════════════════╗${plain}\"\n");
    s.push_str("    echo -e \"${green}║                3x-ui updated successfully!                 ║${plain}\"\n");
    s.push_str("    echo -e \"${green}╠════════════════════════════════════════════════════════════╣${plain}\"\n");
    s.push_str("    echo -e \"${green}║ Status:        All settings and users were preserved.      ║${plain}\"\n");
    s.push_str(&format!("echo -e \"${{green}}║ Access Link:   {:<43} ║${{plain}}\"\n", access_link));
    s.push_str("    echo -e \"${green}╚════════════════════════════════════════════════════════════╝${plain}\"\n");
    s.push_str("else\n");
    s.push_str("    echo -e \"${green}╔════════════════════════════════════════════════════════════╗${plain}\"\n");
    s.push_str("    echo -e \"${green}║                3x-ui installed successfully!               ║${plain}\"\n");
    s.push_str("    echo -e \"${green}╠════════════════════════════════════════════════════════════╣${plain}\"\n");
    s.push_str(&format!("    echo -e \"${{green}}║ Username:      {:<43} ║${{plain}}\"\n", c.panel_username));
    s.push_str(&format!("    echo -e \"${{green}}║ Password:      {:<43} ║${{plain}}\"\n", c.panel_password));
    s.push_str(&format!("    echo -e \"${{green}}║ Port:          {:<43} ║${{plain}}\"\n", c.panel_port));
    s.push_str(&format!("    echo -e \"${{green}}║ WebPath:       {:<43} ║${{plain}}\"\n", c.panel_web_base_path));
    s.push_str(&format!("    echo -e \"${{green}}║ Access Link:   {:<43} ║${{plain}}\"\n", access_link));
    s.push_str("    echo -e \"${green}╚════════════════════════════════════════════════════════════╝${plain}\"\n");
    s.push_str("    echo -e \"${yellow}⚠ Keep this information secure!${plain}\"\n");
    s.push_str("fi\n\n");
    s.push_str("echo -e \"Management Commands:\"\n");
    s.push_str("echo -e \"  x-ui start / stop / restart / status / log\"\n");

    s
}

fn build_pkg_section(c: &BuildConfig) -> String {
    let online_cmd = os_detect::install_command_online(&c.os);
    match c.package_mode {
        PackageMode::Online => {
            format!("install_base_online() {{\n    {}\n}}", online_cmd)
        }
        PackageMode::Offline => {
            let fmt = os_detect::mirror_info(&c.os)
                .map(|m| m.format)
                .unwrap_or(PkgFormat::Deb);
            let offline_cmd = os_detect::install_command_offline(&c.os, &fmt);
            let mut s = String::new();
            s.push_str("install_base_offline() {\n");
            s.push_str("    echo \"Installing packages from offline bundle...\"\n");
            s.push_str("    cd \"$BUNDLE_DIR\"\n");
            for line in offline_cmd.lines() {
                s.push_str(&format!("    {}\n", line));
            }
            s.push_str("}\n");
            s.push_str("install_base_online() {\n");
            s.push_str("    echo \"Online fallback for missing packages...\"\n");
            s.push_str(&format!("    {} || true\n", online_cmd));
            s.push_str("}");
            s
        }
    }
}

fn build_ssl_section(c: &BuildConfig) -> String {
    match &c.ssl {
        SslConfig::None => {
            "# SSL Disabled — Panel runs on HTTP\n".to_string()
        }
        SslConfig::Custom { .. } | SslConfig::SelfSigned { dynamic: false, .. } | SslConfig::LetsEncrypt { .. } => {
            let mut s = String::new();
            s.push_str("setup_ssl() {\n");
            s.push_str("    local cert_dest=\"/root/cert/bundle\"\n");
            s.push_str("    mkdir -p \"$cert_dest\"\n");
            s.push_str("    cp \"$BUNDLE_DIR/ssl/fullchain.pem\" \"$cert_dest/fullchain.pem\"\n");
            s.push_str("    cp \"$BUNDLE_DIR/ssl/privkey.pem\"   \"$cert_dest/privkey.pem\"\n");
            s.push_str("    chmod 644 \"$cert_dest/fullchain.pem\"\n");
            s.push_str("    chmod 600 \"$cert_dest/privkey.pem\"\n");
            s.push_str("    /usr/local/x-ui/x-ui cert \\\n");
            s.push_str("        -webCert \"$cert_dest/fullchain.pem\" \\\n");
            s.push_str("        -webCertKey \"$cert_dest/privkey.pem\" > /dev/null 2>&1 || true\n");
            s.push_str("    echo \"  SSL certificate installed\"\n");
            s.push_str("}\n");
            s
        }
        SslConfig::SelfSigned { dynamic: true, .. } => {
            let mut s = String::new();
            s.push_str("setup_ssl() {\n");
            s.push_str("    local cert_dest=\"/root/cert/bundle\"\n");
            s.push_str("    mkdir -p \"$cert_dest\"\n");
            s.push_str("    echo -e \"\\n${blue}┌─ Dynamic SSL Generation ─────────────────────────────┐${plain}\"\n");
            s.push_str("    while true; do\n");
            s.push_str("        read -p \"Enter Target Server IP or Domain for SSL: \" TARGET_HOST\n");
            s.push_str("        TARGET_HOST=$(echo \"$TARGET_HOST\" | xargs)\n");
            s.push_str("        if [[ -n \"$TARGET_HOST\" ]]; then break; fi\n");
            s.push_str("        echo -e \"${red}Host cannot be empty.${plain}\"\n");
            s.push_str("    done\n");
            s.push_str("    echo -e \"Generating Self-Signed Certificate for ${yellow}$TARGET_HOST${plain}...\"\n");
            s.push_str("    openssl req -x509 -newkey rsa:2048 -nodes -keyout \"$cert_dest/privkey.pem\" -out \"$cert_dest/fullchain.pem\" -days 3650 -subj \"/CN=$TARGET_HOST\" 2>/dev/null\n");
            s.push_str("    chmod 644 \"$cert_dest/fullchain.pem\"\n");
            s.push_str("    chmod 600 \"$cert_dest/privkey.pem\"\n");
            s.push_str("    /usr/local/x-ui/x-ui cert \\\n");
            s.push_str("        -webCert \"$cert_dest/fullchain.pem\" \\\n");
            s.push_str("        -webCertKey \"$cert_dest/privkey.pem\" > /dev/null 2>&1 || true\n");
            s.push_str("    echo -e \"  ${green}✓ SSL certificate generated and installed for $TARGET_HOST${plain}\"\n");
            s.push_str("}\n");
            s
        }
    }
}

fn build_service_section(is_alpine: bool) -> (String, String) {
    if is_alpine {
        let stop = "rc-service x-ui stop 2>/dev/null || true".to_string();
        let install = concat!(
            "cp \"$BUNDLE_DIR/x-ui.rc\" /etc/init.d/x-ui\n",
            "chmod +x /etc/init.d/x-ui\n",
            "rc-update add x-ui\n",
            "rc-service x-ui start"
        ).to_string();
        (stop, install)
    } else {
        let stop = "systemctl stop x-ui 2>/dev/null || true".to_string();
        let install = concat!(
            "cp \"$BUNDLE_DIR/x-ui.service\" /etc/systemd/system/x-ui.service\n",
            "chown root:root /etc/systemd/system/x-ui.service\n",
            "chmod 644 /etc/systemd/system/x-ui.service\n",
            "systemctl daemon-reload\n",
            "systemctl enable x-ui\n",
            "systemctl start x-ui"
        ).to_string();
        (stop, install)
    }
}
