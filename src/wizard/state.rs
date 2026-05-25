use crate::proxy::ProxyConfig;


#[derive(Debug, Clone)]
pub struct IncludedComponents {
    pub system_packages: bool,
    pub ssl: bool,
    pub xui_panel: bool,
}

/// All configuration collected from the user during the wizard phase.
/// This is the single source of truth passed to downloader and generator.
#[derive(Debug, Clone)]
pub struct BuildConfig {
    // ── Target server info ──────────────────────────────────────
    pub os: TargetOs,
    pub arch: TargetArch,
    pub os_version: Option<String>, // e.g. "22.04", "9"

    // ── Package installation mode ────────────────────────────────
    pub package_mode: PackageMode,

    // ── x-ui binary ─────────────────────────────────────────────
    pub xui_version: XuiVersion,

    // ── Panel settings ───────────────────────────────────────────
    pub panel_port: u16,
    pub panel_username: String,
    pub panel_password: String,
    pub panel_web_base_path: String,

    // ── SSL ──────────────────────────────────────────────────────
    pub ssl: SslConfig,

    // ── Proxy (optional) ─────────────────────────────────────────
    pub proxy: Option<ProxyConfig>,

    // ── Server Info ─────────────────────────────────────────────────────────
    pub server_host: String,

    // ── Output ───────────────────────────────────────────────────
    pub output_dir: String,
    pub output_kind: OutputKind,
    
    // ── Included Components (Updater Mode) ───────────────────────
    pub included: IncludedComponents,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum OutputKind {
    Sfx,    // Single .sh file
    Folder, // Plain directory
}

// ─── OS ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum TargetOs {
    Ubuntu,
    Debian,
    CentOs,
    Fedora,
    AlmaLinux,
    Rocky,
    Rhel,
    Alpine,
    Arch,
    Manjaro,
    OpenSuse,
}

impl TargetOs {
    pub fn display_name(&self) -> &'static str {
        match self {
            TargetOs::Ubuntu     => "Ubuntu",
            TargetOs::Debian     => "Debian",
            TargetOs::CentOs     => "CentOS",
            TargetOs::Fedora     => "Fedora",
            TargetOs::AlmaLinux  => "AlmaLinux",
            TargetOs::Rocky      => "Rocky Linux",
            TargetOs::Rhel       => "RHEL",
            TargetOs::Alpine     => "Alpine Linux",
            TargetOs::Arch       => "Arch Linux",
            TargetOs::Manjaro    => "Manjaro",
            TargetOs::OpenSuse   => "OpenSUSE",
        }
    }
}

// ─── Architecture ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum TargetArch {
    Amd64,
    Arm64,
    Armv7,
    I386,
    S390x,
}

impl TargetArch {
    pub fn xui_suffix(&self) -> &'static str {
        match self {
            TargetArch::Amd64  => "amd64",
            TargetArch::Arm64  => "arm64",
            TargetArch::Armv7  => "armv7",
            TargetArch::I386   => "386",
            TargetArch::S390x  => "s390x",
        }
    }

    /// Debian/Ubuntu package architecture name
    pub fn deb_arch(&self) -> &'static str {
        match self {
            TargetArch::Amd64  => "amd64",
            TargetArch::Arm64  => "arm64",
            TargetArch::Armv7  => "armhf",
            TargetArch::I386   => "i386",
            TargetArch::S390x  => "s390x",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            TargetArch::Amd64  => "amd64 (x86_64) — Most common",
            TargetArch::Arm64  => "arm64 (aarch64)",
            TargetArch::Armv7  => "armv7",
            TargetArch::I386   => "386 (x86 32-bit)",
            TargetArch::S390x  => "s390x (IBM)",
        }
    }
}

// ─── Package mode ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum PackageMode {
    /// Install packages from internet during installation
    Online,
    /// Download packages now, install offline
    Offline,
}

// ─── x-ui version ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum XuiVersion {
    Latest,
    Specific(String), // e.g. "v2.5.1"
}

// ─── SSL ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum SslConfig {
    /// No SSL — plain HTTP
    None,
    /// User provides existing cert + key files
    Custom {
        fullchain_path: String,
        privkey_path:   String,
    },
    /// Tool generates a self-signed certificate
    SelfSigned {
        /// IP or domain for the CN/SAN
        common_name: String,
        /// Generate the certificate dynamically on the target server during installation
        dynamic: bool,
    },
    /// Use Let's Encrypt DNS-01 Challenge
    LetsEncrypt {
        domain: String,
    },
}
