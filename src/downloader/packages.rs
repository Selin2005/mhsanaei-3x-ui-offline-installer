use anyhow::Result;
use console::style;

use crate::manifest::{Manifest, STEP_PACKAGES};
use crate::os_detect::{self, PkgFormat};
use crate::proxy;
use crate::wizard::state::BuildConfig;
use super::xui::download_with_progress;
use std::time::Instant;

/// Download system packages for offline installation.
/// Skips if the step is already marked Done and valid.
pub async fn download(
    config: &BuildConfig,
    pkg_dir: &str,
    out_dir: &str,
    manifest: &mut Manifest,
) -> Result<()> {
    // If already done and valid, skip
    if manifest.step_is_valid(out_dir, STEP_PACKAGES) {
        println!(
            "  {} packages — Already exist, skipping.",
            style("⏭️").dim()
        );
        return Ok(());
    }

    let Some(mirror) = os_detect::mirror_info(&config.os) else {
        println!(
            "  {} Offline package download is not supported for {}.",
            style("⚠️").yellow(),
            config.os.display_name()
        );
        // Mark as done with empty file list (means "skipped")
        manifest.mark_done(out_dir, STEP_PACKAGES, vec![])?;
        return Ok(());
    };

    let packages = os_detect::required_packages(&config.os);
    let client   = proxy::build_client(&config.proxy)?;

    println!(
        "  {} Downloading {} packages for {}...",
        style("→").cyan(),
        packages.len(),
        config.os.display_name()
    );

    let mut downloaded_files: Vec<String> = vec![];

    for pkg in &packages {
        let result = match mirror.format {
            PkgFormat::Deb => download_deb(&client, pkg, mirror.mirror_base, pkg_dir).await,
            PkgFormat::Rpm => download_rpm(&client, pkg, mirror.mirror_base, pkg_dir).await,
            PkgFormat::Apk => download_apk(&client, pkg, mirror.mirror_base, pkg_dir).await,
        };

        match result {
            Ok(Some(filename)) => {
                downloaded_files.push(format!("packages/{}", filename));
            }
            Ok(None) => {
                println!(
                    "  {} {} skipped (will be installed online)",
                    style("⚠️").yellow(),
                    pkg
                );
            }
            Err(e) => {
                println!("  {} {} — Error: {}", style("✗").red(), pkg, e);
            }
        }
    }

    // Mark partial if some packages failed, done if all succeeded
    if downloaded_files.len() == packages.len() {
        manifest.mark_done(out_dir, STEP_PACKAGES, downloaded_files)?;
    } else if !downloaded_files.is_empty() {
        let count = downloaded_files.len();
        manifest.mark_partial(
            out_dir,
            STEP_PACKAGES,
            downloaded_files,
            Some(format!(
                "{}/{} packages downloaded",
                count,
                packages.len()
            )),
        )?;
    } else {
        manifest.mark_done(out_dir, STEP_PACKAGES, vec![])?;
    }

    println!(
        "  {} Packages downloaded → {}",
        style("✓").green(),
        style(pkg_dir).yellow()
    );
    Ok(())
}

/// Download a .deb from the Ubuntu/Debian pool mirror.
async fn download_deb(
    client: &reqwest::Client,
    pkg: &str,
    _mirror_base: &str,
    dest_dir: &str,
) -> Result<Option<String>> {
    let api_url = format!("https://packages.ubuntu.com/jammy/{}/download", pkg);

    if let Ok(Some(body)) = fetch_html_with_retry(client, &api_url).await {
        if let Some(url) = extract_deb_url(&body, "amd64")
            .or_else(|| extract_deb_url(&body, "all"))
        {
            let filename = url.split('/').last().unwrap_or(pkg).to_string();
            let dest = format!("{}/{}", dest_dir, filename);
            download_with_progress(client, &url, &dest, &format!("{} (.deb)", pkg)).await?;
            return Ok(Some(filename));
        }
    }
    Ok(None)
}

fn extract_deb_url(html: &str, arch: &str) -> Option<String> {
    for line in html.lines() {
        if line.contains(".deb") && line.contains(arch) && line.contains("http") {
            if let Some(start) = line.find("href=\"") {
                let rest = &line[start + 6..];
                if let Some(end) = rest.find('"') {
                    let url = &rest[..end];
                    if url.ends_with(".deb") {
                        return Some(url.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Download an RPM from Rocky/RHEL mirrors.
async fn download_rpm(
    client: &reqwest::Client,
    pkg: &str,
    mirror_base: &str,
    dest_dir: &str,
) -> Result<Option<String>> {
    let first = pkg.chars().next().unwrap_or('a');
    let index_url = format!("{}/{}/", mirror_base, first);

    if let Ok(Some(body)) = fetch_html_with_retry(client, &index_url).await {
        if let Some(filename) = extract_rpm_filename(&body, pkg) {
            let url = format!("{}/{}/{}", mirror_base, first, filename);
            let dest = format!("{}/{}", dest_dir, filename);
            download_with_progress(client, &url, &dest, &format!("{} (.rpm)", pkg)).await?;
            return Ok(Some(filename));
        }
    }
    Ok(None)
}

async fn fetch_html_with_retry(client: &reqwest::Client, url: &str) -> Result<Option<String>> {
    let mut first_failure: Option<Instant> = None;
    let mut warned = false;

    loop {
        match client.get(url).send().await {
            Ok(r) => {
                if r.status().is_success() {
                    if let Ok(body) = r.text().await {
                        return Ok(Some(body));
                    }
                } else if r.status().is_client_error() {
                    return Ok(None); // e.g., 404, do not retry forever
                }
            }
            Err(_) => {}
        }

        if first_failure.is_none() {
            first_failure = Some(Instant::now());
        }
        if let Some(ff) = first_failure {
            if ff.elapsed().as_secs() >= 60 && !warned {
                println!("  {} Warning: Still trying, but internet seems down. Please check your connection or press Ctrl+C to exit.", style("⚠️").yellow());
                warned = true;
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }
}

fn extract_rpm_filename(html: &str, pkg: &str) -> Option<String> {
    for line in html.lines() {
        if line.contains(pkg) && line.contains(".rpm") {
            if let Some(start) = line.find("href=\"") {
                let rest = &line[start + 6..];
                if let Some(end) = rest.find('"') {
                    let name = &rest[..end];
                    if name.starts_with(pkg) && name.ends_with(".rpm") {
                        return Some(name.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Download an APK from Alpine CDN.
async fn download_apk(
    client: &reqwest::Client,
    pkg: &str,
    mirror_base: &str,
    dest_dir: &str,
) -> Result<Option<String>> {
    let direct_url = format!("{}/{}.apk", mirror_base, pkg);
    let filename   = format!("{}.apk", pkg);
    let dest       = format!("{}/{}", dest_dir, filename);

    match download_with_progress(client, &direct_url, &dest, &format!("{} (.apk)", pkg)).await {
        Ok(_) => Ok(Some(filename)),
        Err(_) => {
            let _ = std::fs::remove_file(&dest);
            Ok(None)
        }
    }
}
