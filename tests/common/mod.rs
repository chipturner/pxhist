// Re-export the test utilities from the main library
use std::{env, path::Path};

// Additional test utilities that require dev-dependencies
use assert_cmd::Command as AssertCommand;
pub use pxh::test_utils::*;

/// Compatibility wrapper for existing PxhCaller usage  
/// Deprecated: Use PxhTestHelper instead
pub struct PxhCaller {
    helper: PxhTestHelper,
}

impl PxhCaller {
    pub fn new() -> Self {
        PxhCaller {
            helper: PxhTestHelper::new().with_custom_db_path("test"), // PxhCaller used "test" as db name
        }
    }

    pub fn call<S: AsRef<str>>(&self, args: S) -> AssertCommand {
        let mut cmd = AssertCommand::new(pxh_path());

        // Set environment from helper
        cmd.env_clear();
        cmd.env("HOME", self.helper.home_dir());
        cmd.env("PXH_DB_PATH", self.helper.db_path());
        cmd.env("PXH_HOSTNAME", &self.helper.hostname);
        cmd.env("USER", &self.helper.username);
        cmd.env(
            "PATH",
            format!(
                "{}:{}",
                pxh_path().parent().unwrap().display(),
                env::var("PATH").unwrap_or_default()
            ),
        );

        // Propagate coverage environment variables if they exist
        if let Ok(profile_file) = env::var("LLVM_PROFILE_FILE") {
            cmd.env("LLVM_PROFILE_FILE", profile_file);
        }
        if let Ok(llvm_cov) = env::var("CARGO_LLVM_COV") {
            cmd.env("CARGO_LLVM_COV", llvm_cov);
        }

        cmd.args(args.as_ref().split(' '));
        cmd
    }

    pub fn tmpdir(&self) -> &Path {
        self.helper.home_dir()
    }
}
