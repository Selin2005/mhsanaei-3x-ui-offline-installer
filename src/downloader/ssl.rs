use anyhow::Result;
use console::style;
use rcgen::{generate_simple_self_signed, CertifiedKey};
use std::fs;

/// Copy user-provided cert files to the bundle ssl/ directory.
pub fn copy_custom(fullchain_src: &str, privkey_src: &str, out_dir: &str) -> Result<()> {
    let ssl_dir = format!("{}/ssl", out_dir);
    fs::create_dir_all(&ssl_dir)?;

    fs::copy(fullchain_src, format!("{}/fullchain.pem", ssl_dir))?;
    fs::copy(privkey_src, format!("{}/privkey.pem", ssl_dir))?;

    println!(
        "  {} SSL files copied → {}",
        style("✓").green(),
        style(&ssl_dir).yellow()
    );
    Ok(())
}

/// Generate a self-signed certificate for the given IP or domain.
pub fn generate_self_signed(common_name: &str, out_dir: &str) -> Result<()> {
    println!(
        "  {} Generating self-signed certificate for {}...",
        style("→").cyan(),
        style(common_name).yellow()
    );

    let ssl_dir = format!("{}/ssl", out_dir);
    fs::create_dir_all(&ssl_dir)?;

    // Build Subject Alternative Names — support both IP and domain
    let subject_alt_names = vec![common_name.to_string()];

    let CertifiedKey { cert, key_pair } = generate_simple_self_signed(subject_alt_names)
        .map_err(|e| anyhow::anyhow!("Failed to generate self-signed certificate: {}", e))?;

    let cert_pem = cert.pem();
    let key_pem  = key_pair.serialize_pem();

    fs::write(format!("{}/fullchain.pem", ssl_dir), &cert_pem)?;
    fs::write(format!("{}/privkey.pem",   ssl_dir), &key_pem)?;

    // Secure key permissions (best-effort on Linux)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(format!("{}/privkey.pem", ssl_dir))?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(format!("{}/privkey.pem", ssl_dir), perms)?;
    }

    println!(
        "  {} Self-signed certificate generated → {}/ssl/",
        style("✓").green(),
        out_dir
    );
    println!();
    println!("  {}", style("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━").yellow());
    println!("  {} {}", style("ℹ️  Self-Signed Certificate Guide:").bold(), "");
    println!("  {}", style("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━").yellow());
    println!("  • This certificate is suitable for personal use.");
    println!("  • Browsers will show a security warning when opening the panel.");
    println!("  • To bypass the warning in Chrome: click anywhere on the page and");
    println!("    type: {}", style("thisisunsafe").bold().cyan());
    println!("  • In Firefox: Advanced → Accept Risk and Continue");
    println!("  {}", style("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━").yellow());
    println!();

    Ok(())
}

use acme2::{AccountBuilder, DirectoryBuilder, OrderBuilder, OrderStatus, AuthorizationStatus, ChallengeStatus};
use serde::Deserialize;
use std::time::Duration;
use tokio::time::sleep;
use sha2::{Digest, Sha256};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use crate::proxy::{build_client, ProxyConfig};

#[derive(Deserialize)]
struct DohResponse {
    #[serde(rename = "Answer")]
    answer: Option<Vec<DohAnswer>>,
}

#[derive(Deserialize)]
struct DohAnswer {
    data: String,
}

pub async fn generate_lets_encrypt(domain: &str, out_dir: &str, proxy: &Option<ProxyConfig>) -> Result<()> {
    if let Some(p) = proxy {
        std::env::set_var("HTTP_PROXY", &p.url);
        std::env::set_var("HTTPS_PROXY", &p.url);
        std::env::set_var("ALL_PROXY", &p.url);
    }

    println!("  {} Connecting to Let's Encrypt...", style("→").cyan());
    
    let dir = DirectoryBuilder::new("https://acme-v02.api.letsencrypt.org/directory".to_string()).build().await?;
    let mut builder = AccountBuilder::new(dir);
    builder.contact(vec![format!("mailto:admin@{}", domain)]);
    builder.terms_of_service_agreed(true);
    let account = builder.build().await?;

    let mut builder = OrderBuilder::new(account);
    builder.add_dns_identifier(domain.to_string());
    let order = builder.build().await?;

    let authorizations = order.authorizations().await?;
    let auth = authorizations.into_iter().next().unwrap();
    let challenge = auth.get_challenge("dns-01").unwrap();
    
    let key_auth = challenge.key_authorization()?.unwrap();
    let digest = Sha256::digest(key_auth.as_bytes());
    let txt_value = URL_SAFE_NO_PAD.encode(&digest);
    let txt_name = format!("_acme-challenge.{}", domain);
    // For "sub.example.com" → the record name in DNS panel is "_acme-challenge.sub"
    // For "example.com" → the record name in DNS panel is "_acme-challenge"
    let domain_parts: Vec<&str> = domain.splitn(2, '.').collect();
    let root_domain = if domain_parts.len() == 2 {
        // Check if this looks like a subdomain (more than 2 parts total)
        let all_parts: Vec<&str> = domain.split('.').collect();
        if all_parts.len() > 2 {
            // e.g., sub.example.com → root is example.com, subdomain part is sub
            let root_parts = &all_parts[all_parts.len()-2..];
            root_parts.join(".")
        } else {
            // e.g., example.com → root is example.com
            domain.to_string()
        }
    } else {
        domain.to_string()
    };

    // Build the DNS record "Name" field as it should appear in DNS panels
    let all_parts: Vec<&str> = domain.split('.').collect();
    let dns_record_name = if all_parts.len() > 2 {
        // Subdomain case: sub.example.com → _acme-challenge.sub
        let subdomain_parts = &all_parts[..all_parts.len()-2];
        format!("_acme-challenge.{}", subdomain_parts.join("."))
    } else {
        // Root domain case: example.com → _acme-challenge
        "_acme-challenge".to_string()
    };

    println!();
    println!("  {}", style("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━").yellow());
    println!("  {} ", style("🔐 Let's Encrypt DNS-01 Challenge — Step-by-Step Guide").bold());
    println!("  {}", style("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━").yellow());
    println!();
    println!("  To verify that you own {}, Let's Encrypt needs you", style(domain).cyan().bold());
    println!("  to add a special DNS TXT record to your domain's DNS settings.");
    println!();
    println!("  {}", style("What you need to do:").bold().underlined());
    println!();
    println!("  1. Log in to your DNS provider's panel (e.g., Cloudflare, Namecheap, etc.)");
    println!("  2. Go to the DNS settings for your domain: {}", style(&root_domain).cyan().bold());
    println!("  3. Add a {} DNS record with these values:", style("TXT").bold().yellow());
    println!();
    println!("     ┌──────────────────────────────────────────────────────────────┐");
    println!("     │  Type:   {}                                                  │", style("TXT").bold().green());
    println!("     │  Name:   {:<51}│", style(&dns_record_name).cyan().bold());
    println!("     │  Value:  {:<51}│", style(&txt_value).cyan().bold());
    println!("     │  TTL:    {} (or Auto)                                      │", style("1 min").yellow());
    println!("     └──────────────────────────────────────────────────────────────┘");
    println!();
    println!("  {}", style("Example for Cloudflare:").bold().underlined());
    println!("  • Go to your domain → DNS → Records → Add Record");
    println!("  • Type: TXT");
    println!("  • Name: {}", style(&dns_record_name).cyan());
    println!("  • Content: {}", style(&txt_value).cyan());
    println!("  • Proxy status: DNS only (gray cloud)");
    println!("  • Click Save");
    println!();
    println!("  {}", style("Example for Namecheap:").bold().underlined());
    println!("  • Go to Domain List → Manage → Advanced DNS → Add New Record");
    println!("  • Type: TXT");
    println!("  • Host: {}", style(&dns_record_name).cyan());
    println!("  • Value: {}", style(&txt_value).cyan());
    println!("  • TTL: Automatic");
    println!("  • Click Save");
    println!();
    println!("  {}", style("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━").yellow());
    println!("  {}", style("⚠️  Important Notes:").bold().yellow());
    println!("  • Do NOT create a new subdomain! This is just a DNS TXT record.");
    println!("  • The full record that will be queried is: {}", style(&txt_name).dim());
    println!("  • DNS propagation may take 1-5 minutes. The tool will wait and retry.");
    println!("  {}", style("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━").yellow());
    println!();
    println!("  After saving the TXT record in your DNS provider,");
    println!("  press {} to continue...", style("Enter").bold().green());
    
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;

    println!("  {} Verifying DNS record locally via Cloudflare DoH (Proxy supported)...", style("→").cyan());
    let doh_url = format!("https://cloudflare-dns.com/dns-query?name={}&type=TXT", txt_name);
    let client = build_client(proxy)?;

    let mut verified = false;
    for _ in 1..=60 {
        if let Ok(resp) = client.get(&doh_url).header("Accept", "application/dns-json").send().await {
            if let Ok(doh) = resp.json::<DohResponse>().await {
                if let Some(answers) = doh.answer {
                    for ans in answers {
                        let cleaned = ans.data.trim_matches('"');
                        if cleaned == txt_value {
                            verified = true;
                            break;
                        }
                    }
                }
            }
        }
        if verified {
            println!("  {} DNS Verification Successful!", style("✓").green());
            break;
        }
        println!("  {} Record not propagated yet. Retrying in 10 seconds...", style("ℹ").yellow());
        sleep(Duration::from_secs(10)).await;
    }

    if !verified {
        anyhow::bail!("DNS record did not propagate after 10 minutes. Please run again later.");
    }

    println!("  {} Requesting certificate validation from Let's Encrypt...", style("→").cyan());
    let challenge = challenge.validate().await?;
    let challenge = challenge.wait_done(Duration::from_secs(5), 10).await?;
    if challenge.status != ChallengeStatus::Valid {
        anyhow::bail!("Let's Encrypt validation failed.");
    }

    let auth = auth.wait_done(Duration::from_secs(5), 10).await?;
    if auth.status != AuthorizationStatus::Valid {
        anyhow::bail!("Authorization failed.");
    }

    let order = order.wait_ready(Duration::from_secs(5), 5).await?;
    if order.status != OrderStatus::Ready {
        anyhow::bail!("Order is not ready.");
    }

    use openssl::rsa::Rsa;
    use openssl::pkey::PKey;
    use acme2::Csr;
    
    let rsa = Rsa::generate(2048)?;
    let pkey = PKey::from_rsa(rsa)?;
    
    let order = order.finalize(Csr::Automatic(pkey.clone())).await?;
    let order = order.wait_done(Duration::from_secs(5), 10).await?;
    if order.status != OrderStatus::Valid {
        anyhow::bail!("Order failed to finalize.");
    }

    let certs = order.certificate().await?.unwrap();
    
    let ssl_dir = format!("{}/ssl", out_dir);
    fs::create_dir_all(&ssl_dir)?;

    let mut fullchain = String::new();
    for cert in certs {
        fullchain.push_str(&String::from_utf8(cert.to_pem()?)?);
    }
    
    fs::write(format!("{}/fullchain.pem", ssl_dir), fullchain)?;
    fs::write(format!("{}/privkey.pem", ssl_dir), pkey.private_key_to_pem_pkcs8()?)?;

    println!("  {} Certificate generated and saved!", style("✓").green());
    Ok(())
}
