use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::Read;


use crate::wizard::state::BuildConfig;

pub const MANIFEST_FILE: &str = "manifest.json";
const MANIFEST_VERSION: &str = "1";

// ─── Data model ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version:    String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Serialized snapshot of BuildConfig so we can restore/compare it.
    pub config:     serde_json::Value,
    pub steps:      HashMap<String, BundleStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleStep {
    pub status: StepStatus,
    /// Primary files produced by this step (relative to bundle root).
    pub files:  Vec<String>,
    /// sha256 of each file (same order as `files`).
    pub sha256: Vec<String>,
    pub note:   Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Partial,
    Done,
    Failed,
}

impl std::fmt::Display for StepStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StepStatus::Pending  => write!(f, "pending"),
            StepStatus::Partial  => write!(f, "partial"),
            StepStatus::Done     => write!(f, "done"),
            StepStatus::Failed   => write!(f, "failed"),
        }
    }
}

// ─── Known step keys ─────────────────────────────────────────────────────────

pub const STEP_XUI_BINARY:   &str = "xui_binary";
pub const STEP_XUI_SH:       &str = "xui_sh";
pub const STEP_SERVICE_FILE: &str = "service_file";
pub const STEP_PACKAGES:     &str = "packages";
pub const STEP_SSL:          &str = "ssl";
pub const STEP_INSTALL_SH:   &str = "install_sh";

pub const ALL_STEPS: &[&str] = &[
    STEP_XUI_BINARY,
    STEP_XUI_SH,
    STEP_SERVICE_FILE,
    STEP_PACKAGES,
    STEP_SSL,
    STEP_INSTALL_SH,
];

// ─── Constructors ─────────────────────────────────────────────────────────────

impl Manifest {
    /// Create a fresh manifest for a new bundle.
    pub fn new(config: &BuildConfig) -> Self {
        let now = Utc::now();
        let mut steps = HashMap::new();
        for &key in ALL_STEPS {
            steps.insert(
                key.to_string(),
                BundleStep {
                    status: StepStatus::Pending,
                    files:  vec![],
                    sha256: vec![],
                    note:   None,
                },
            );
        }
        Self {
            version:    MANIFEST_VERSION.to_string(),
            created_at: now,
            updated_at: now,
            config:     serde_json::to_value(SerializableConfig::from(config))
                            .unwrap_or(serde_json::Value::Null),
            steps,
        }
    }

    // ─── Persistence ─────────────────────────────────────────────────────────

    pub fn load(bundle_dir: &str) -> Result<Self> {
        let path = format!("{}/{}", bundle_dir, MANIFEST_FILE);
        let data = fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&data)?)
    }

    pub fn save(&mut self, bundle_dir: &str) -> Result<()> {
        use anyhow::Context;
        self.updated_at = Utc::now();
        let path = format!("{}/{}", bundle_dir, MANIFEST_FILE);
        let data = serde_json::to_string_pretty(self)?;
        fs::write(&path, data)
            .with_context(|| format!("Failed to write manifest file at: {}", path))?;
        Ok(())
    }

    // ─── Step management ─────────────────────────────────────────────────────

    /// Mark a step as successfully completed and record its files + sha256 hashes.
    pub fn mark_done(
        &mut self,
        bundle_dir: &str,
        step: &str,
        files: Vec<String>,
    ) -> Result<()> {
        // Compute sha256 for each file
        let mut hashes = Vec::with_capacity(files.len());
        for f in &files {
            let full = format!("{}/{}", bundle_dir, f);
            hashes.push(sha256_file(&full).unwrap_or_else(|_| "error".to_string()));
        }

        self.steps.insert(
            step.to_string(),
            BundleStep {
                status: StepStatus::Done,
                files,
                sha256: hashes,
                note: None,
            },
        );
        self.save(bundle_dir)?;
        Ok(())
    }

    /// Mark a step as partially done (e.g., some packages downloaded).
    pub fn mark_partial(
        &mut self,
        bundle_dir: &str,
        step: &str,
        files: Vec<String>,
        note: Option<String>,
    ) -> Result<()> {
        let mut hashes = Vec::with_capacity(files.len());
        for f in &files {
            let full = format!("{}/{}", bundle_dir, f);
            hashes.push(sha256_file(&full).unwrap_or_else(|_| "error".to_string()));
        }

        self.steps.insert(
            step.to_string(),
            BundleStep {
                status: StepStatus::Partial,
                files,
                sha256: hashes,
                note,
            },
        );
        self.save(bundle_dir)?;
        Ok(())
    }

    /// Returns true if the step has status Done and all its files pass sha256 check.
    pub fn step_is_valid(&self, bundle_dir: &str, step: &str) -> bool {
        let Some(s) = self.steps.get(step) else { return false };
        if s.status != StepStatus::Done { return false; }
        if s.files.is_empty() { return false; }

        for (file, expected_hash) in s.files.iter().zip(s.sha256.iter()) {
            let full = format!("{}/{}", bundle_dir, file);
            match sha256_file(&full) {
                Ok(h) if &h == expected_hash => {}
                _ => return false,
            }
        }
        true
    }

    pub fn step_is_done(&self, step: &str) -> bool {
        self.steps
            .get(step)
            .map(|s| s.status == StepStatus::Done)
            .unwrap_or(false)
    }
}

// ─── SHA-256 helper ───────────────────────────────────────────────────────────

pub fn sha256_file(path: &str) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

// ─── Serializable subset of BuildConfig (for manifest storage) ───────────────

#[derive(Serialize, Deserialize)]
pub struct SerializableConfig {
    pub os:                String,
    pub arch:              String,
    pub os_version:        Option<String>,
    pub package_mode:      String,
    pub xui_version:       String,
    pub panel_port:        u16,
    pub panel_username:    String,
    pub panel_password:    String,
    pub panel_web_base_path: String,
    pub ssl_kind:          String,
    pub server_host:       String,
    pub output_dir:        String,
    pub output_kind:       String,
    pub included_system_packages: bool,
    pub included_ssl: bool,
    pub included_xui_panel: bool,
}

impl From<&BuildConfig> for SerializableConfig {
    fn from(c: &BuildConfig) -> Self {
        use crate::wizard::state::{PackageMode, SslConfig, XuiVersion, OutputKind};
        Self {
            os:                 c.os.display_name().to_string(),
            arch:               c.arch.xui_suffix().to_string(),
            os_version:         c.os_version.clone(),
            package_mode:       match c.package_mode {
                PackageMode::Online  => "online".to_string(),
                PackageMode::Offline => "offline".to_string(),
            },
            xui_version:        match &c.xui_version {
                XuiVersion::Latest      => "latest".to_string(),
                XuiVersion::Specific(v) => v.clone(),
            },
            panel_port:         c.panel_port,
            panel_username:     c.panel_username.clone(),
            panel_password:     c.panel_password.clone(),
            panel_web_base_path: c.panel_web_base_path.clone(),
            ssl_kind:           match &c.ssl {
                SslConfig::None            => "none".to_string(),
                SslConfig::Custom { .. }   => "custom".to_string(),
                SslConfig::SelfSigned { common_name, dynamic } =>
                    format!("self-signed({}|{})", common_name, dynamic),
                SslConfig::LetsEncrypt { domain } =>
                    format!("letsencrypt({})", domain),
            },
            server_host:        c.server_host.clone(),
            output_dir:         c.output_dir.clone(),
            output_kind:        match c.output_kind {
                OutputKind::Sfx    => "sfx".to_string(),
                OutputKind::Folder => "folder".to_string(),
            },
            included_system_packages: c.included.system_packages,
            included_ssl: c.included.ssl,
            included_xui_panel: c.included.xui_panel,
        }
    }
}
