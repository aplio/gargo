use self_update::backends::github::{ReleaseList, Update as GithubUpdate};
use self_update::update::Release;
use semver::Version;

const REPO_OWNER: &str = "aplio";
const REPO_NAME: &str = "gargo";
const BIN_NAME: &str = "gargo";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpgradeCommand {
    Check,
    Update,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpdateRequest {
    current_version: String,
    target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LatestRelease {
    version: String,
    tag: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpgradeCheckStatus {
    UpToDate { current: String },
    UpdateAvailable { current: String, latest: String },
}

impl UpgradeCheckStatus {
    pub fn current_version(&self) -> &str {
        match self {
            Self::UpToDate { current } => current,
            Self::UpdateAvailable { current, .. } => current,
        }
    }

    pub fn latest_version(&self) -> &str {
        match self {
            Self::UpToDate { current } => current,
            Self::UpdateAvailable { latest, .. } => latest,
        }
    }

    pub fn has_update(&self) -> bool {
        matches!(self, Self::UpdateAvailable { .. })
    }
}

trait UpdateSource {
    fn latest_release(&self, request: &UpdateRequest) -> Result<LatestRelease, String>;
    fn perform_upgrade(
        &self,
        request: &UpdateRequest,
        latest: &LatestRelease,
    ) -> Result<(), String>;
}

struct GithubUpdateSource;

impl UpdateSource for GithubUpdateSource {
    fn latest_release(&self, request: &UpdateRequest) -> Result<LatestRelease, String> {
        let releases = ReleaseList::configure()
            .repo_owner(REPO_OWNER)
            .repo_name(REPO_NAME)
            .build()
            .map_err(|e| format!("failed to build GitHub release query: {e}"))?
            .fetch()
            .map_err(|e| format!("failed to fetch releases from GitHub: {e}"))?;

        let release = releases
            .into_iter()
            .next()
            .ok_or_else(|| "no releases found in GitHub repository".to_string())?;

        latest_release_from_release(release, request)
    }

    fn perform_upgrade(
        &self,
        request: &UpdateRequest,
        latest: &LatestRelease,
    ) -> Result<(), String> {
        GithubUpdate::configure()
            .repo_owner(REPO_OWNER)
            .repo_name(REPO_NAME)
            .bin_name(BIN_NAME)
            .target(&request.target)
            .target_version_tag(&latest.tag)
            .current_version(&request.current_version)
            .no_confirm(true)
            .show_download_progress(true)
            .build()
            .map_err(|e| format!("failed to configure updater: {e}"))?
            .update()
            .map_err(|e| format!("failed to upgrade binary: {e}"))?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MockUpdateState {
    UpToDate,
    HasUpdate,
    Error,
}

impl MockUpdateState {
    fn from_env() -> Self {
        match std::env::var("GARGO_TEST_UPDATE_STATE") {
            Ok(value) if value.eq_ignore_ascii_case("has_update") => Self::HasUpdate,
            Ok(value) if value.eq_ignore_ascii_case("error") => Self::Error,
            _ => Self::UpToDate,
        }
    }
}

struct MockUpdateSource {
    state: MockUpdateState,
}

impl MockUpdateSource {
    fn from_env() -> Self {
        Self {
            state: MockUpdateState::from_env(),
        }
    }
}

impl UpdateSource for MockUpdateSource {
    fn latest_release(&self, request: &UpdateRequest) -> Result<LatestRelease, String> {
        match self.state {
            MockUpdateState::Error => Err("mock update source failure".to_string()),
            MockUpdateState::UpToDate => Ok(LatestRelease {
                version: request.current_version.clone(),
                tag: format!("v{}", request.current_version),
            }),
            MockUpdateState::HasUpdate => {
                let mut version = parse_semver(&request.current_version)?;
                version.patch += 1;
                Ok(LatestRelease {
                    version: version.to_string(),
                    tag: format!("v{version}"),
                })
            }
        }
    }

    fn perform_upgrade(
        &self,
        _request: &UpdateRequest,
        _latest: &LatestRelease,
    ) -> Result<(), String> {
        match self.state {
            MockUpdateState::Error => Err("mock upgrade failure".to_string()),
            MockUpdateState::UpToDate | MockUpdateState::HasUpdate => Ok(()),
        }
    }
}

pub fn run(command: UpgradeCommand) -> Result<String, String> {
    let request = build_request()?;
    if use_mock_update_source() {
        let source = MockUpdateSource::from_env();
        run_with_source(&source, command, &request)
    } else {
        let source = GithubUpdateSource;
        run_with_source(&source, command, &request)
    }
}

pub fn check_status() -> Result<UpgradeCheckStatus, String> {
    let request = build_request()?;
    if use_mock_update_source() {
        let source = MockUpdateSource::from_env();
        check_status_with_source(&source, &request)
    } else {
        let source = GithubUpdateSource;
        check_status_with_source(&source, &request)
    }
}

fn run_with_source(
    source: &dyn UpdateSource,
    command: UpgradeCommand,
    request: &UpdateRequest,
) -> Result<String, String> {
    let status = check_status_with_source(source, request)?;
    match command {
        UpgradeCommand::Check => Ok(format_check_status(&status)),
        UpgradeCommand::Update => {
            if let UpgradeCheckStatus::UpToDate { current } = status {
                return Ok(format!(
                    "Already up to date: {} ({}/{})",
                    current,
                    std::env::consts::OS,
                    std::env::consts::ARCH
                ));
            }
            let latest = source.latest_release(request)?;
            let current = status.current_version().to_string();
            let newest = status.latest_version().to_string();
            source.perform_upgrade(request, &latest)?;
            Ok(format!("Upgraded gargo from {} to {}", current, newest))
        }
    }
}

fn check_status_with_source(
    source: &dyn UpdateSource,
    request: &UpdateRequest,
) -> Result<UpgradeCheckStatus, String> {
    let latest = source.latest_release(request)?;
    let current = parse_semver(&request.current_version)?;
    let newest = parse_semver(&latest.version)?;

    if newest > current {
        Ok(UpgradeCheckStatus::UpdateAvailable {
            current: current.to_string(),
            latest: newest.to_string(),
        })
    } else {
        Ok(UpgradeCheckStatus::UpToDate {
            current: current.to_string(),
        })
    }
}

fn format_check_status(status: &UpgradeCheckStatus) -> String {
    match status {
        UpgradeCheckStatus::UpToDate { current } => format!(
            "Already up to date: {} ({}/{})",
            current,
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
        UpgradeCheckStatus::UpdateAvailable { current, latest } => format!(
            "Update available: {} -> {} ({}/{})",
            current,
            latest,
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
    }
}

fn use_mock_update_source() -> bool {
    matches!(
        std::env::var("GARGO_TEST_UPDATE_SOURCE").as_deref(),
        Ok("mock")
    )
}

fn build_request() -> Result<UpdateRequest, String> {
    let target = resolve_target_triple()?;
    Ok(UpdateRequest {
        current_version: env!("CARGO_PKG_VERSION").to_string(),
        target,
    })
}

fn resolve_target_triple() -> Result<String, String> {
    let triple = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        (os, arch) => {
            return Err(format!(
                "unsupported platform for upgrade: {os}/{arch} (supported: macos|linux + x86_64|aarch64)"
            ));
        }
    };
    Ok(triple.to_string())
}

fn latest_release_from_release(
    release: Release,
    request: &UpdateRequest,
) -> Result<LatestRelease, String> {
    release.asset_for(&request.target, None).ok_or_else(|| {
        format!(
            "latest release does not include an asset for target {}",
            request.target
        )
    })?;
    let version = normalize_version_string(&release.version);
    let tag = if release.version.starts_with('v') {
        release.version
    } else {
        format!("v{}", version)
    };
    Ok(LatestRelease { version, tag })
}

fn normalize_version_string(version: &str) -> String {
    version.trim_start_matches('v').to_string()
}

fn parse_semver(value: &str) -> Result<Version, String> {
    Version::parse(value.trim_start_matches('v'))
        .map_err(|e| format!("invalid version '{value}': {e}"))
}

#[cfg(test)]
mod tests {
    use super::{
        MockUpdateSource, MockUpdateState, UpdateRequest, UpgradeCheckStatus,
        check_status_with_source, parse_semver, resolve_target_triple,
    };

    #[test]
    fn semver_parser_accepts_with_or_without_v() {
        assert_eq!(
            parse_semver("0.1.12").expect("parse"),
            semver::Version::new(0, 1, 12)
        );
        assert_eq!(
            parse_semver("v0.1.12").expect("parse"),
            semver::Version::new(0, 1, 12)
        );
    }

    #[test]
    fn target_triple_matches_supported_platforms() {
        let target = resolve_target_triple().expect("resolve platform");
        let valid = matches!(
            target.as_str(),
            "x86_64-apple-darwin"
                | "aarch64-apple-darwin"
                | "x86_64-unknown-linux-gnu"
                | "aarch64-unknown-linux-gnu"
        );
        assert!(valid, "unexpected target: {target}");
    }

    #[test]
    fn typed_status_reports_available_update() {
        let source = MockUpdateSource {
            state: MockUpdateState::HasUpdate,
        };
        let request = UpdateRequest {
            current_version: "0.1.19".to_string(),
            target: "x86_64-unknown-linux-gnu".to_string(),
        };

        let status = check_status_with_source(&source, &request).expect("status");
        assert_eq!(
            status,
            UpgradeCheckStatus::UpdateAvailable {
                current: "0.1.19".to_string(),
                latest: "0.1.20".to_string(),
            }
        );
        assert!(status.has_update());
        assert_eq!(status.current_version(), "0.1.19");
        assert_eq!(status.latest_version(), "0.1.20");
    }

    #[test]
    fn typed_status_reports_up_to_date() {
        let source = MockUpdateSource {
            state: MockUpdateState::UpToDate,
        };
        let request = UpdateRequest {
            current_version: "0.1.19".to_string(),
            target: "x86_64-unknown-linux-gnu".to_string(),
        };

        let status = check_status_with_source(&source, &request).expect("status");
        assert_eq!(
            status,
            UpgradeCheckStatus::UpToDate {
                current: "0.1.19".to_string(),
            }
        );
        assert!(!status.has_update());
        assert_eq!(status.current_version(), "0.1.19");
        assert_eq!(status.latest_version(), "0.1.19");
    }
}
