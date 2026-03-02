use self_update::backends::github::{ReleaseList, Update as GithubUpdate};
use self_update::update::Release;
use semver::Version;

const REPO_OWNER: &str = "aplio";
const REPO_NAME: &str = "gargo";
const BIN_NAME: &str = "gargo";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpgradeCommand {
    Check,
    Upgrade,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpdateRequest {
    current_version: String,
    target: String,
    expected_asset_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LatestRelease {
    version: String,
    tag: String,
    asset_name: String,
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
                asset_name: request.expected_asset_name.clone(),
            }),
            MockUpdateState::HasUpdate => {
                let mut version = parse_semver(&request.current_version)?;
                version.patch += 1;
                Ok(LatestRelease {
                    version: version.to_string(),
                    tag: format!("v{version}"),
                    asset_name: request.expected_asset_name.clone(),
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

fn run_with_source(
    source: &dyn UpdateSource,
    command: UpgradeCommand,
    request: &UpdateRequest,
) -> Result<String, String> {
    let latest = source.latest_release(request)?;
    if latest.asset_name != request.expected_asset_name {
        return Err(format!(
            "release asset mismatch for {}: expected {}, got {}",
            request.target, request.expected_asset_name, latest.asset_name
        ));
    }

    let current = parse_semver(&request.current_version)?;
    let newest = parse_semver(&latest.version)?;
    match command {
        UpgradeCommand::Check => {
            if newest > current {
                Ok(format!(
                    "Update available: {} -> {} ({}/{})",
                    current,
                    newest,
                    std::env::consts::OS,
                    std::env::consts::ARCH
                ))
            } else {
                Ok(format!(
                    "Already up to date: {} ({}/{})",
                    current,
                    std::env::consts::OS,
                    std::env::consts::ARCH
                ))
            }
        }
        UpgradeCommand::Upgrade => {
            if newest <= current {
                return Ok(format!(
                    "Already up to date: {} ({}/{})",
                    current,
                    std::env::consts::OS,
                    std::env::consts::ARCH
                ));
            }
            source.perform_upgrade(request, &latest)?;
            Ok(format!("Upgraded gargo from {} to {}", current, newest))
        }
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
    let expected_asset_name = format!("{BIN_NAME}-{target}.tar.gz");
    Ok(UpdateRequest {
        current_version: env!("CARGO_PKG_VERSION").to_string(),
        target,
        expected_asset_name,
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
    let asset = release.asset_for(&request.target, None).ok_or_else(|| {
        format!(
            "latest release does not include an asset for target {}",
            request.target
        )
    })?;
    Ok(LatestRelease {
        version: normalize_version_string(&release.version),
        tag: release.version,
        asset_name: asset.name,
    })
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
    use super::{parse_semver, resolve_target_triple};

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
}
