# Hexinfo Path Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Put the application's private paths under `Hexinfo` on Windows, Linux and macOS, preserve user data on upgrade, and remove successfully migrated legacy directories.

**Architecture:** Keep the public executable name, protocol and platform-standard registration locations. Centralize private path construction and non-overwriting directory migration in Rust; let the Windows and Linux installers place program files, stop the previous instance, and invoke the new binary's migration before starting it. A conflict leaves the legacy entry in place with a visible diagnostic.

**Tech Stack:** Rust 2024, `dirs`, `fd-lock`, POSIX `sh`, NSIS, Cargo, GitHub Actions Windows MSVC/Linux packaging.

**Spec:** [2026-09-24-hexinfo-path-migration-design.md](../specs/2026-09-24-hexinfo-path-migration-design.md)

## Global Constraints

- `Hexinfo` is the directory component, with that exact case; Windows uses `%LOCALAPPDATA%\Programs`, not `%APPDATA%\Local` or `Programes`.
- Preserve `x-notify-service`, `x-notify://`, HTTP/SDK behavior, port 17320, Markdown, sizes, colors and popup appearance.
- Do not move standard entrypoints (`~/.local/bin`, XDG desktop/autostart/icons, macOS LaunchAgents, Windows Run/Classes/Uninstall); update executable targets instead.
- New config wins over legacy config, which wins over the package template. Never overwrite a user file during migration.
- Delete only exact legacy default paths after a successful move; a conflict or error keeps the old entry recoverable. Never delete a custom old install directory recursively.
- This work produces test packages only; no tag or release. Preserve the unrelated `.zcodeignore` file.
- Linux `XDG_DATA_HOME` means the per-user data root (default `~/.local/share`); no user configuration is required. Double-click may launch `install.sh` with **no terminal window**: visible success/failure must come from a GUI popup/notification, not `echo`, and require no extra package.
- Current `scripts/pack-windows.nsi` has an **uncommitted provisional edit** with `RMDir /r`; replace that block under Task 3 instead of treating it as verified implementation.

## Review Focus

1. Existing new config and different old config: keep the new file, report the old conflict, and leave the old directory; a repeat run changes nothing. Task 2 tests both.
2. Old executable still running: installer stops it before migration; direct startup detects the legacy lock and defers migration rather than creating a second instance. Tasks 2–4 test this.
3. Custom XDG roots with spaces: Linux package and runtime paths honor each root without splitting or moving the standard command entrypoint. Task 4 tests this.
4. Old custom Windows install location: upgrade leaves it untouched; only the exact default legacy path may be cleaned. Task 3 tests this.
5. Linux installer launched without a terminal: success and pre-copy failure invoke the package binary's visible notification or an existing GUI fallback, with the original exit code preserved. Task 4 tests this.

---

### Task 1: Centralized private paths

**Files:** Modify `src/config.rs`, `src/single.rs`; add focused tests beside existing tests in those files; update `scripts/templates/config.toml` comments.

**Interfaces:** Produce `config::ORG_DIR_NAME: &str = "Hexinfo"`, `config::private_dir(base: PathBuf) -> PathBuf`, `config::legacy_private_dir(base: PathBuf) -> PathBuf`. `single::data_dir()` uses `private_dir`; Task 2 consumes the same helpers. Do not change `APP_DIR_NAME` because it is also a protocol/desktop/icon identity.

- [ ] **Step 1: Add a failing path test.** In `src/config.rs`:

  ```rust
  #[test]
  fn private_paths_include_organization_without_changing_app_name() {
      let base = std::path::PathBuf::from("root");
      assert_eq!(super::private_dir(base.clone()), base.join("Hexinfo/x-notify-service"));
      assert_eq!(super::legacy_private_dir(base.clone()), base.join("x-notify-service"));
      assert_eq!(super::APP_DIR_NAME, "x-notify-service");
  }
  ```

- [ ] **Step 2: Verify red.** Run `cargo test private_paths_include_organization_without_changing_app_name`; expect missing helper errors.
- [ ] **Step 3: Implement only the paths.** Add the two helpers below; use them in `default_log_dir`, `user_config_path`, and `single::data_dir`. Keep Linux's `XDG_RUNTIME_DIR/x-notify-service.lock` name unchanged because it is a standard runtime entrypoint. Update platform path comments in the config template.

  ```rust
  pub const ORG_DIR_NAME: &str = "Hexinfo";
  pub fn private_dir(base: PathBuf) -> PathBuf {
      base.join(ORG_DIR_NAME).join(APP_DIR_NAME)
  }
  pub fn legacy_private_dir(base: PathBuf) -> PathBuf {
      base.join(APP_DIR_NAME)
  }
  ```
- [ ] **Step 4: Verify green.** Run `cargo test private_paths_include_organization_without_changing_app_name`, `cargo test`, and `cargo check --target x86_64-pc-windows-gnu`.
- [ ] **Step 5: Commit only Task 1 files.** `git add src/config.rs src/single.rs scripts/templates/config.toml && git commit -m "refactor(paths): namespace private directories under Hexinfo"`.

### Task 2: Preserve legacy private data

**Files:** Create `src/path_migration.rs`; modify `src/main.rs` and `src/single.rs`; tests in `src/path_migration.rs`.

**Interfaces:** `path_migration::migrate_private_dirs() -> std::io::Result<MigrationReport>`; report contains `conflicts: Vec<PathBuf>` and `moved: Vec<PathBuf>`. `single::legacy_is_locked() -> bool` checks the old lock path. `main` calls migration after parsing CLI but before `config::resolve` for `install`, `serve`, and protocol launch; never mutate on `info`. Old config remains a read-only fallback candidate until successfully moved. On Windows, the same module also handles the old **default** program directory after the new binary has been installed; generated package files are excluded, unknown files are preserved.

- [ ] **Step 1: Add red tests using `tempfile::tempdir`** (`tempfile = "3"` under `[dev-dependencies]`). In `src/path_migration.rs`, test the core `migrate_tree(old: &Path, new: &Path, excluded: &[&str]) -> io::Result<MigrationReport>`:

  ```rust
  #[test]
  fn conflict_keeps_both_configs_and_legacy_directory() {
      let root = tempfile::tempdir().unwrap();
      let old = root.path().join("old");
      let new = root.path().join("new");
      std::fs::create_dir_all(&old).unwrap();
      std::fs::create_dir_all(&new).unwrap();
      std::fs::write(old.join("config.toml"), "old").unwrap();
      std::fs::write(new.join("config.toml"), "new").unwrap();
      let report = super::migrate_tree(&old, &new, &[]).unwrap();
      assert_eq!(std::fs::read_to_string(new.join("config.toml")).unwrap(), "new");
      assert_eq!(std::fs::read_to_string(old.join("config.toml")).unwrap(), "old");
      assert_eq!(report.conflicts, vec![old.join("config.toml")]);
  }
  ```

  Add tests for: an absent destination moves an unknown file and removes the old directory; a second call changes nothing; `port`/`instance.lock` are excluded and removed only after the old instance is confirmed stopped; an existing symlink is moved as a link and never traversed.

- [ ] **Step 2: Verify red.** Run `cargo test path_migration`; expect missing migration module/functions.
- [ ] **Step 3: Implement non-overwriting migration.** Start with these types and exact merge rule. Try `std::fs::rename` when the destination is absent. Otherwise walk with `read_dir`/`symlink_metadata`, move only absent entries, recurse into existing directories, record existing-file conflicts, and `remove_dir` only when empty. Never use `remove_dir_all` for a conflicting private data directory. Keep `port`/`instance.lock` ephemeral: after the old lock is free, remove those exact files rather than carrying stale instance identity forward.

  ```rust
  #[derive(Debug, Default)]
  pub struct MigrationReport {
      pub moved: Vec<PathBuf>,
      pub conflicts: Vec<PathBuf>,
  }
  // `migrate_tree(old, new, excluded)` keeps both files on a destination
  // collision and records the old path in `conflicts`.
  ```
- [ ] **Step 4: Connect startup safely.** Add `legacy_is_locked`; before path migration, return a deferred error if the old process owns its lock. In `main`, call the migration before resolving config for the three launch modes above; on migration error, show a diagnostic and do not silently start with default config. Keep `--config` explicit override highest priority, new user config next, old user config last.

  ```rust
  let cli = config::Cli::parse();
  if matches!(&cli.cmd, Some(config::Command::Install | config::Command::Serve))
      || (cli.cmd.is_none() && cli.url_arg.is_some())
  {
      if let Err(error) = path_migration::migrate_private_dirs() {
          eprintln!("Hexinfo 路径迁移失败，旧目录已保留: {error}");
          std::process::exit(1);
      }
  }
  let cfg = config::resolve(&cli);
  ```
- [ ] **Step 5: Verify green and platform builds.** Run `cargo test path_migration`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo check --target x86_64-pc-windows-gnu`, and `cargo fmt --all -- --check`.
- [ ] **Step 6: Commit Task 2 files.** `git add Cargo.toml Cargo.lock src/path_migration.rs src/main.rs src/single.rs src/config.rs && git commit -m "feat(paths): migrate legacy private data without overwrites"`.

### Task 3: Windows installer and upgrade

**Files:** Modify `scripts/pack-windows.nsi`; create `scripts/ci/test-windows-upgrade.ps1`; modify `.github/workflows/ci.yml` Windows build job to run the upgrade test after packing. Keep `scripts/pack-windows.sh` as the single packaging entrypoint.

**Interfaces:** Windows installer uses Task 2 migration through `x-notify-service.exe install`; the NSIS install path and registry key are `...\Programs\Hexinfo\x-notify-service` and `HKCU\Software\Hexinfo\x-notify-service`.

- [ ] **Step 1: Add a failing Windows upgrade acceptance test.** In PowerShell create the exact legacy default install directory under `$env:LOCALAPPDATA\Programs\x-notify-service`, place `config.toml` with a distinctive value and an unknown `user-note.txt`, then invoke the built NSIS setup with `/S`. Assert that the new install directory exists, the config and unknown file survive, the old default directory is gone, and the new registry InstallDir points to the new path. In a second case create a distinct old *custom* directory and assert setup does not delete it. Use a test-specific profile/runner; do not target an actual user's profile.

  ```powershell
  param([Parameter(Mandatory=$true)][string]$setup)
  $old = Join-Path $env:LOCALAPPDATA 'Programs\x-notify-service'
  $new = Join-Path $env:LOCALAPPDATA 'Programs\Hexinfo\x-notify-service'
  New-Item -ItemType Directory -Force $old | Out-Null
  Set-Content (Join-Path $old 'config.toml') 'port = 17321'
  Set-Content (Join-Path $old 'user-note.txt') 'keep me'
  $process = Start-Process -FilePath $setup -ArgumentList '/S' -Wait -PassThru
  if ($process.ExitCode -ne 0) { throw "installer exit $($process.ExitCode)" }
  if ((Get-Content (Join-Path $new 'config.toml')) -ne 'port = 17321') { throw 'config lost' }
  if ((Get-Content (Join-Path $new 'user-note.txt')) -ne 'keep me') { throw 'user file lost' }
  if (Test-Path $old) { throw 'legacy default directory remains' }
  ```
- [ ] **Step 2: Verify red on a Windows runner.** Run the test against the current package script; expect the missing `Hexinfo` path or unsafe cleanup behavior to fail. Local `makensis` syntax compilation is supplementary, not a substitute for the upgrade test.
- [ ] **Step 3: Replace the provisional NSIS edit.** Set the new default `InstallDir` and `InstallDirRegKey`. Stop the old service, migrate the exact old default directory **before** creating the new destination when possible, preserve a preexisting new `config.toml`, and install program/SDK files over regenerated files. Use `ExecWait` for the new binary's `install` command; on nonzero exit do not clean legacy directories. For an already-existing new destination, preserve unknown old files or leave the old directory with a visible conflict message; never `RMDir /r` a directory containing unexamined user files. Remove the legacy registry key only after the new InstallDir registration succeeds. Leave the standard Uninstall registry identity unchanged.

  ```nsis
  !define APPKEY "Software\Hexinfo\${APPNAME}"
  InstallDir "$LOCALAPPDATA\Programs\Hexinfo\${APPNAME}"
  InstallDirRegKey HKCU "${APPKEY}" "InstallDir"
  ; After files/config are in place and before deleting any old directory:
  ExecWait '"$INSTDIR\${APPNAME}.exe" install' $R1
  StrCmp $R1 "0" installSucceeded 0
  Abort "新程序安装失败；旧目录已保留"
  installSucceeded:
  ```
- [ ] **Step 4: Verify green.** Run `makensis` through `scripts/pack-windows.sh x86_64-pc-windows-msvc` on the Windows runner, then `scripts/ci/test-windows-upgrade.ps1`, followed by `x-notify-service.exe --version` and `info`.
- [ ] **Step 5: Commit Task 3 files.** `git add scripts/pack-windows.nsi scripts/ci/test-windows-upgrade.ps1 .github/workflows/ci.yml && git commit -m "fix(windows): migrate installer into Hexinfo directory"`.

### Task 4: Linux installation and XDG migration

**Files:** Modify `scripts/templates/install-linux.sh`, `assets/sdk-使用手册.md`; create `scripts/ci/test-install-linux.sh`; modify `.github/workflows/ci.yml` Linux build job to run the install test against the assembled package.

**Interfaces:** New binary is `$XDG_DATA_HOME/Hexinfo/x-notify-service/bin/x-notify-service`; `~/.local/bin/x-notify-service` is an atomic replacement symlink. Task 2 migrates old share/config/state contents when `install` is invoked.

- [ ] **Step 1: Add a failing shell acceptance test.** Use `mktemp -d` to set isolated `HOME`, `XDG_DATA_HOME`, `XDG_CONFIG_HOME`, and `XDG_STATE_HOME` (including a path with spaces). Seed old config/SDK/log/unknown file, run the package's `install.sh`, and assert the new locations, preserved config, symlink target and absent successfully migrated old directories. Seed a second run with a new-path config to assert it is not overwritten. Test a fake legacy PID file that names an unrelated process and assert it is not killed. Invoke the script with stdin redirected from `/dev/null` to simulate double-click without a terminal; the package-binary mock must record a `notify` call carrying success/error text. Make that mock fail and put a fake `zenity` in `PATH`; assert the dialog fallback is invoked and the script keeps its original nonzero exit code.

  ```sh
  package=${1:?pass extracted Linux package directory}
  test_root=$(mktemp -d)
  export HOME="$test_root/home"
  export XDG_DATA_HOME="$test_root/data root"
  export XDG_CONFIG_HOME="$test_root/config root"
  export XDG_STATE_HOME="$test_root/state root"
  mkdir -p "$HOME" "$XDG_DATA_HOME/x-notify-service" "$XDG_CONFIG_HOME/x-notify-service"
  printf 'port = 17321\n' > "$XDG_CONFIG_HOME/x-notify-service/config.toml"
  sh "$package/install.sh" </dev/null
  test "$(readlink "$HOME/.local/bin/x-notify-service")" = \
      "$XDG_DATA_HOME/Hexinfo/x-notify-service/bin/x-notify-service"
  test "$(cat "$XDG_CONFIG_HOME/Hexinfo/x-notify-service/config.toml")" = 'port = 17321'
  ```
- [ ] **Step 2: Verify red.** Run `sh scripts/ci/test-install-linux.sh`; expect missing new paths.
- [ ] **Step 3: Update the installer.** Derive XDG roots with `${XDG_DATA_HOME:-$HOME/.local/share}`, `${XDG_CONFIG_HOME:-$HOME/.config}`, `${XDG_STATE_HOME:-$HOME/.local/state}`. Stop only the verified old service, create the organized application root, copy the binary to its `bin` directory, and atomically replace the `~/.local/bin` entry with a symlink. Copy current SDK files, invoke the new binary's `install` to migrate old private data and refresh autostart/protocol paths, then add the template config only if no migrated config exists. Leave hicolor/desktop/autostart entrypoint directories at their standard locations. Add an `EXIT` trap that captures `$?`, disables `errexit` inside the handler, first calls the package binary's `notify` to show a persistent popup (even before installation finishes, use `$SRC_DIR/bin/x-notify-service`), then falls back to already-installed `zenity`/`kdialog`/`notify-send`; a terminal message is diagnostic only. The trap must exit with the captured original code.

  ```sh
  report_install_result() {
      result=$?
      trap - EXIT
      set +e
      if [ "$result" -eq 0 ]; then kind=info; message='安装成功';
      else kind=error; message='安装失败，请查看终端输出'; fi
      if [ -x "$SRC_DIR/bin/x-notify-service" ] &&
          "$SRC_DIR/bin/x-notify-service" notify -t "$message" -b "$message"; then
          :
      elif command -v zenity >/dev/null 2>&1 && zenity --"$kind" --text="$message"; then
          :
      elif command -v kdialog >/dev/null 2>&1 && {
          if [ "$result" -eq 0 ]; then kdialog --msgbox "$message";
          else kdialog --error "$message"; fi
      }; then
          :
      elif command -v notify-send >/dev/null 2>&1; then
          notify-send "$message"
      fi
      exit "$result"
  }
  trap report_install_result EXIT
  ```
- [ ] **Step 4: Verify green.** Run `sh -n scripts/templates/install-linux.sh`, `sh scripts/ci/test-install-linux.sh`, and package smoke tests for x86_64 and aarch64 (the existing `pack-linux.sh` retains the Debian 10 / glibc 2.28 floor).
- [ ] **Step 5: Commit Task 4 files.** `git add scripts/templates/install-linux.sh scripts/ci/test-install-linux.sh assets/sdk-使用手册.md .github/workflows/ci.yml && git commit -m "fix(linux): install under Hexinfo and migrate XDG data"`.

### Task 5: macOS migration and full package verification

**Files:** `src/path_migration.rs` tests/entrypoint from Task 2, `scripts/templates/config.toml`/documentation if Task 1 did not cover all examples; no new macOS installer. Verify `scripts/pack-macos.sh` still emits the same `.app` name and bundle ID.

**Interfaces:** `x-notify-service install` and a safe first `serve` invoke Task 2 migration before config resolution; LaunchAgent remains in `~/Library/LaunchAgents` and is refreshed to the current executable path.

- [ ] **Step 1: Add/run macOS path tests.** Verify new Application Support and Logs paths end with `Hexinfo/x-notify-service`, old config/log files migrate and can be read, and a conflicting new config keeps both files. Keep the test's filesystem root in a `tempfile::tempdir()`; do not mutate the real `~/Library`.

  ```rust
  #[cfg(target_os = "macos")]
  #[test]
  fn mac_private_paths_are_namespaced() {
      let support = std::path::PathBuf::from("Library/Application Support");
      assert_eq!(
          crate::config::private_dir(support),
          std::path::PathBuf::from("Library/Application Support/Hexinfo/x-notify-service")
      );
  }
  ```
- [ ] **Step 2: Verify full local quality gates.** Run `cargo fmt --all -- --check`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cd sdk/js && pnpm -F @hexinfo/x-notify-service-sdk test`, and `git diff --check`.
- [ ] **Step 3: Verify cross-platform packages.** Push the test branch and manually dispatch `.github/workflows/release.yml` on it. That workflow uploads Linux x86_64/aarch64 and Windows MSVC/NSIS artifacts but its `publish` job must remain skipped. Inspect each artifact, run the Windows smoke/upgrade tests, and link only the resulting **installer** to the user; do not hand off a raw cross-compiled exe as an installer.
- [ ] **Step 4: Final scope review.** Compare the code and package contents to every spec path; verify `.zcodeignore` remains untouched and no unrelated changes were staged. Report runtime visual/upgrade checks not executed on the user's own Windows/Deepin machines separately from CI evidence.
