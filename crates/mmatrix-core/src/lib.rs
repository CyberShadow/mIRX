pub const APP_NAME: &str = "mmatrix";
pub const FOUNDATION_PHASE: &str = "bootstrap";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceInfo {
    pub project: &'static str,
    pub phase: &'static str,
    pub windows_target: &'static str,
}

pub fn workspace_info() -> WorkspaceInfo {
    WorkspaceInfo {
        project: APP_NAME,
        phase: FOUNDATION_PHASE,
        windows_target: "x86_64-pc-windows-gnu",
    }
}

pub fn ui_self_test_line() -> &'static str {
    "mmatrix-ui self-test ok"
}

#[cfg(test)]
mod tests {
    use super::{ui_self_test_line, workspace_info};

    #[test]
    fn workspace_info_matches_bootstrap_target() {
        let info = workspace_info();
        assert_eq!(info.project, "mmatrix");
        assert_eq!(info.phase, "bootstrap");
        assert_eq!(info.windows_target, "x86_64-pc-windows-gnu");
    }

    #[test]
    fn ui_self_test_line_is_stable() {
        assert_eq!(ui_self_test_line(), "mmatrix-ui self-test ok");
    }
}
