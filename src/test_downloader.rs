use anyhow::Result;
use std::io::Write;
use console::style;
use crate::wizard::state::{BuildConfig, TargetOs, TargetArch, PackageMode, XuiVersion, OutputKind, SslConfig};
use crate::downloader::packages;
use crate::proxy;

pub async fn test_all_mirrors() -> Result<()> {
    println!("{}", style("━".repeat(54)).cyan());
    println!("{}", style("  🔍  Testing Package Mirrors and Parsers...").cyan().bold());
    println!("{}", style("━".repeat(54)).cyan());
    println!();

    let oss = vec![
        TargetOs::Ubuntu,
        TargetOs::Debian,
        TargetOs::AlmaLinux,
        TargetOs::Rocky,
        TargetOs::Fedora,
        TargetOs::Alpine,
    ];

    let client = proxy::build_client(&None)?;

    for os in oss {
        let display = os.display_name();
        println!("  {} Testing {:<20}", style("→").cyan().bold(), style(display).bold());
        
        let config = BuildConfig {
            os: os.clone(),
            arch: TargetArch::Amd64,
            os_version: None,
            package_mode: PackageMode::Offline,
            server_host: "1.1.1.1".to_string(),
            xui_version: XuiVersion::Latest,
            panel_port: 54321,
            panel_username: "admin".to_string(),
            panel_password: "admin".to_string(),
            panel_web_base_path: "/".to_string(),
            ssl: SslConfig::None,
            proxy: None,
            output_dir: "/tmp/test".to_string(),
            output_kind: OutputKind::Folder,
            included: crate::wizard::state::IncludedComponents {
                system_packages: true,
                ssl: true,
                xui_panel: true,
            },
        };

        let packages = crate::os_detect::required_packages(&os);
        let mirror = crate::os_detect::mirror_info(&os).unwrap();
        
        use crate::os_detect::PkgFormat;
        
        for pkg in packages {
            print!("     - {:<18} ", pkg);
            let _ = std::io::stdout().flush();

            let res = match mirror.format {
                PkgFormat::Deb => packages::download_deb_test(&client, pkg, config.arch.deb_arch(), mirror.mirror_bases).await,
                PkgFormat::Rpm => packages::download_rpm_test(&client, pkg, mirror.mirror_bases).await,
                PkgFormat::Apk => packages::download_apk_test(&client, pkg, mirror.mirror_bases).await,
            };

            match res {
                Ok(url) => {
                    println!("{}  {}", style("✓").green(), style(url).dim());
                }
                Err(e) => {
                    println!("{}  Failed: {}", style("✗").red(), style(e).red());
                }
            }
        }
        println!();
    }

    println!();
    println!("{}", style("  ✅  Mirror testing complete.").green().bold());
    Ok(())
}
