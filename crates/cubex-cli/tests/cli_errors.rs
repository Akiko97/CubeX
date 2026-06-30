use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BAD_STRATEGY: &str = r#"strategy "bad-diagnostics" {
  plugin print = process("../../../target/debug/cubex-print-plugin")

  # This route is intentionally wrong: `missing` is not a declared plugin or engine.
  route missing-source = source == missing -> [print]
}
"#;

#[test]
fn compile_prints_stable_strategy_diagnostic() {
    let fixture = BadStrategyFixture::new();

    let output = Command::new(env!("CARGO_BIN_EXE_cubex"))
        .arg("compile")
        .arg(fixture.path())
        .output()
        .unwrap();

    assert_failed_with_diagnostic(output, fixture.expected_stderr());
}

#[test]
fn check_prints_stable_strategy_diagnostic() {
    let fixture = BadStrategyFixture::new();

    let output = Command::new(env!("CARGO_BIN_EXE_cubex"))
        .args(["check", "-c"])
        .arg(fixture.path())
        .output()
        .unwrap();

    assert_failed_with_diagnostic(output, fixture.expected_stderr());
}

fn assert_failed_with_diagnostic(output: Output, expected_stderr: String) {
    assert!(!output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "");
    assert_eq!(String::from_utf8(output.stderr).unwrap(), expected_stderr);
}

struct BadStrategyFixture {
    dir: PathBuf,
    source: PathBuf,
}

impl BadStrategyFixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("cubex-cli-error-test-{}", unique_id()));
        let source = dir.join("cubex.cx");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(&source, BAD_STRATEGY).unwrap();
        Self { dir, source }
    }

    fn path(&self) -> &Path {
        &self.source
    }

    fn expected_stderr(&self) -> String {
        let route_line = "  route missing-source = source == missing -> [print]";
        let caret_prefix = " ".repeat(2 + route_line.rfind("missing").unwrap());
        format!(
            "strategy compile error at {}:5:36\n  {route_line}\n{caret_prefix}^^^^^^^\n  source comparison references unknown plugin or engine `missing`\n",
            self.source.display()
        )
    }
}

impl Drop for BadStrategyFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn unique_id() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        .to_string()
        + "-"
        + &std::process::id().to_string()
}
