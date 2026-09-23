# GPUI Kit Popup Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the iced notification window with GPUI Kit 0.6.6 and change `body` from the legacy HTML subset to bounded Markdown while preserving API fields, sizing, colors, lifecycle, service behavior, and platform window semantics.

**Architecture:** HTTP, CLI, SDK, validation, and system fallback stay outside the UI. A GPUI foreground controller owns at most one `WindowHandle<PopupView>` and consumes plain Rust messages from a cross-thread channel. `PopupView` renders Markdown with GPUI Base TextView; a platform adapter supplies right-bottom placement where GPUI lacks a portable move API.

**Tech Stack:** Rust 2024, `gpui-kit 0.6.6` with `default-features = false`, GPUI Base TextView, `markdown 1.0`, raw-window-handle 0.6, Win32/objc2/X11 APIs, tiny_http, JS SDK/Vite/TypeScript.

**Spec:** `docs/superpowers/specs/2026-09-23-gpui-kit-popup-migration-design.md`

## Global Constraints

- Depend on `gpui-kit = { version = "0.6.6", default-features = false }`; do not enable `gpui-component` or bundled assets.
- Remove iced completely from production code and the dependency graph.
- Preserve HTTP/SDK field names, validation ranges, authentication, CORS, and single-instance behavior.
- `body` is Markdown; legacy HTML must render literally, not remain a compatibility path.
- Default size stays 220×100; request width/height and four request colors remain authoritative.
- Service mode uses `QuitMode::Explicit`; closing the only window must not end the service.
- GPUI/GPU failure degrades to system notifications; do not retain an iced/software-rendered popup.
- Preserve the unrelated `.zcodeignore`; never stage it.

## Review Focus

- Raw HTML must display literally: Task 1 adds the regression test.
- Markdown images and links must not fetch or navigate: Task 2 adds render/click tests.
- Rapid Notify → Close → Notify must leave one clean current window: Task 4 adds state tests.
- GPUI initialization/window failure must leave HTTP alive and fall back once: Task 5 adds injected-failure tests.
- Visible resize must retain the 14px bottom-right margin on every backend: Task 3 adds geometry tests; Task 7 records runtime evidence.

---

## File Structure

- Create `src/markdown_body.rs`: Markdown sanitization and system-notification plain text.
- Create `src/notify/view.rs`: payload, GPUI rendering, Markdown style and interaction state.
- Create `src/notify/window.rs`: window options, backend selection and runtime geometry.
- Rewrite `src/notify/app.rs`: GPUI Application, controller, bridge and lifecycle.
- Simplify `src/notify/window_icon.rs`; delete `src/html/**` after consumers move.
- Modify lifecycle consumers, docs, SDK, CI and packaging only in their assigned tasks.

### Task 1: Introduce GPUI Kit and the Markdown body contract

**Files:**
- Modify: `Cargo.toml`, `Cargo.lock`, `src/main.rs`, `src/notify/fallback.rs`
- Create/Test: `src/markdown_body.rs`

**Interfaces:**
- Produces: `sanitize_for_text_view(source: &str) -> Cow<'_, str>`
- Produces: `to_plain_text(source: &str) -> String`
- Task 2 passes sanitized text to TextView; fallback always uses plain text.

- [ ] **Step 1: Write failing Markdown contract tests**

Add `mod markdown_body;` and create:

```rust
#[cfg(test)]
mod tests {
    use super::{sanitize_for_text_view, to_plain_text};

    #[test]
    fn raw_html_is_literal_markdown_input() {
        assert_eq!(sanitize_for_text_view("待办 <b>1</b> 条"), "待办 &lt;b&gt;1&lt;/b&gt; 条");
    }

    #[test]
    fn plain_text_drops_syntax_and_link_targets() {
        assert_eq!(
            to_plain_text("**紧急** [工单](https://example.invalid/1)\n\n- 第一项\n- `code`"),
            "紧急 工单\n第一项\ncode"
        );
    }

    #[test]
    fn image_uses_alt_text_without_url() {
        assert_eq!(to_plain_text("![流程图](https://example.invalid/a.png)"), "流程图");
    }
}
```

- [ ] **Step 2: Run focused tests and verify RED**

Run: `cargo test markdown_body::tests --locked`

Expected: compile failure because both functions are missing.

- [ ] **Step 3: Add dependencies and implement conversion**

Add temporarily alongside iced:

```toml
gpui-kit = { version = "0.6.6", default-features = false }
markdown = "1.0"
```

Parse with `markdown::to_mdast(source, &markdown::ParseOptions::gfm())`. `sanitize_for_text_view` collects only `Node::Html` source ranges and replaces those ranges in reverse order with `&lt;`/`&gt;`; do not globally escape `>` because that breaks block quotes.

```rust
pub fn to_plain_text(source: &str) -> String {
    let Ok(root) = markdown::to_mdast(source, &markdown::ParseOptions::gfm()) else {
        return source.to_owned();
    };
    let mut out = String::new();
    collect_text(&root, &mut out);
    normalize_newlines(out)
}
```

`collect_text` appends Text/Code/InlineCode values, Image alt text, Link child text without URL, and line endings for paragraph/heading/list-item/break nodes. Raw HTML is literal. It never loads a resource.

- [ ] **Step 4: Route fallback through Markdown plain text**

Replace `crate::html::to_plain_text(body_html)` with `crate::markdown_body::to_plain_text(body_markdown)` in both fallback paths. Keep the wire field named `body`.

- [ ] **Step 5: Verify GREEN**

Run:

```bash
cargo test markdown_body::tests --locked
cargo check --all-targets --locked
```

Expected: both pass; iced remains temporarily because the old popup still compiles.

- [ ] **Step 6: Commit Task 1**

```bash
git add Cargo.toml Cargo.lock src/main.rs src/markdown_body.rs src/notify/fallback.rs
git diff --cached --check
git commit -m "feat(markdown): 建立通知正文 Markdown 契约"
```

### Task 2: Build and test the GPUI PopupView

**Files:**
- Create/Test: `src/notify/view.rs`
- Modify: `Cargo.toml`, `Cargo.lock`, `src/notify/mod.rs`, `src/notify/popup.rs`

**Interfaces:**
- Consumes: `sanitize_for_text_view`, `popup::{Colors, Size, body_limits}`.
- Produces: `PopupPayload { title, body_markdown, size, colors, quit_on_close }`.
- Produces: `PopupView::{new, set_payload, reset_interaction}`.

- [ ] **Step 1: Enable GPUI tests and write failing layout tests**

Add:

```toml
[dev-dependencies]
gpui-kit = { version = "0.6.6", default-features = false, features = ["test-support"] }
```

Use `#[gpui_kit::test]` and `gpui_kit::test::{TestSupportExt, TestWindowExt}`:

```rust
#[gpui_kit::test]
fn popup_layout_keeps_header_and_close_cell_geometry(cx: &mut TestAppContext) {
    let handle = cx.open_window(size(px(220.), px(100.)), |_, _| {
        PopupView::new(PopupPayload::fixture("消息提醒", "待办 **1** 条"))
    });
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        assert_eq!(window.find("popup-root").bounds().size, size(px(220.), px(100.)));
        assert_eq!(window.find("popup-header").bounds().size.height, px(38.));
        assert_eq!(window.find("popup-close").bounds().size, size(px(38.), px(38.)));
    }).unwrap();
}
```

Add tests that reset hover and render `[链接](https://example.invalid)` plus `![图](https://example.invalid/a.png)` without invoking URL/image loading.

- [ ] **Step 2: Run tests and verify RED**

Run: `cargo test notify::view::tests --locked -- --test-threads=1`

Expected: missing `PopupView` and `PopupPayload`.

- [ ] **Step 3: Implement view state and rendering**

```rust
#[derive(Clone)]
pub struct PopupPayload {
    pub title: String,
    pub body_markdown: String,
    pub size: popup::Size,
    pub colors: popup::Colors,
    pub quit_on_close: bool,
}

pub struct PopupView {
    payload: PopupPayload,
    hover_close: bool,
    slide_px: f32,
}
```

Render stable IDs `popup-root`, `popup-header`, `popup-title`, `popup-close`, and `popup-body`. Build the close mark with `canvas` plus two stroked diagonal `PathBuilder` paths, never a text glyph.

```rust
TextView::markdown("popup-markdown", sanitize_for_text_view(&self.payload.body_markdown))
    .selectable(false)
    .scrollable(false)
    .max_lines(popup::body_limits(self.payload.size).max_lines)
    .style(compact_markdown_style(self.payload.colors))
    .on_link_click(|_, _, _, _| {})
```

Register an image renderer returning alt text only. Set heading sizes to 14px, paragraph gap to zero, and derive semantic colors from request colors.

- [ ] **Step 4: Implement foreground slide animation**

On first mount, one GPUI foreground task ticks `slide_px` from 26 to 0 over 220ms using the background executor timer and `cx.notify()`. Updating a visible payload does not restart it.

- [ ] **Step 5: Verify GREEN**

```bash
cargo test notify::view::tests --locked -- --test-threads=1
cargo check --all-targets --locked
```

- [ ] **Step 6: Commit Task 2**

```bash
git add Cargo.toml Cargo.lock src/notify/mod.rs src/notify/popup.rs src/notify/view.rs
git diff --cached --check
git commit -m "feat(popup): 实现 GPUI Kit 通知视图"
```

### Task 3: Implement cross-platform window options and geometry

**Files:**
- Create/Test: `src/notify/window.rs`
- Modify: `Cargo.toml`, `Cargo.lock`, `src/notify/mod.rs`, `src/screen.rs`

**Interfaces:**
- Produces: `window_options(area: WorkArea, size: Size) -> WindowOptions`.
- Produces: `sync_geometry(window: &mut Window, area: WorkArea, size: Size) -> Result<(), WindowSyncError>`.
- Produces: `BackendKind` and `backend_kind_from_env`.

- [ ] **Step 1: Write failing geometry tests**

```rust
#[test]
fn resizing_preserves_bottom_right_margin() {
    let area = WorkArea { x: 0.0, y: 30.0, w: 1920.0, h: 985.0, scale: 1.0 };
    assert_eq!(popup_bounds(area, Size { width: 220.0, height: 100.0 }), (1686, 901, 220, 100));
    assert_eq!(popup_bounds(area, Size { width: 400.0, height: 140.0 }), (1506, 861, 400, 140));
}
```

Also test Wayland selection when only `WAYLAND_DISPLAY` exists and X11 selection when only `DISPLAY` exists.

- [ ] **Step 2: Run tests and verify RED**

Run: `cargo test notify::window::tests --locked`

- [ ] **Step 3: Add raw handles and creation options**

Add `raw-window-handle = "0.6"`, required Win32 features, and retain objc2/x11rb. Windows/macOS/X11 use a non-focused, non-resizable `WindowKind::PopUp` with explicit bounds. Wayland uses `WindowKind::LayerShell` with Overlay, RIGHT|BOTTOM anchors, 14px margins, and `KeyboardInteractivity::None`.

- [ ] **Step 4: Implement runtime geometry sync**

- Windows: raw HWND + `SetWindowPos(HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW)`.
- macOS: raw AppKit view → NSWindow frame using existing coordinate conversion.
- X11: raw XID + one x11rb `configure_window`; never search by title.
- Wayland: `window.resize`; layer anchors preserve placement.

Return backend-rich errors; never panic.

- [ ] **Step 5: Verify**

```bash
cargo test notify::window::tests --locked
cargo check --all-targets --locked
cargo check --target x86_64-pc-windows-msvc --locked
```

- [ ] **Step 6: Commit Task 3**

```bash
git add Cargo.toml Cargo.lock src/notify/mod.rs src/notify/window.rs src/screen.rs
git diff --cached --check
git commit -m "feat(window): 接入 GPUI 跨平台通知窗口属性"
```

### Task 4: Replace iced lifecycle and bridge with GPUI

**Files:**
- Rewrite/Test: `src/notify/app.rs`
- Modify: `src/notify/mod.rs`

**Interfaces:**
- Consumes: `PopupPayload`, `PopupView`, `window_options`, `sync_geometry`.
- Produces: `run_service() -> Result<(), AppError>`, `run_single(PopupPayload) -> Result<(), AppError>`, `post(Message) -> bool`.

- [ ] **Step 1: Write failing state tests**

```rust
#[test]
fn notify_close_notify_has_one_clean_visible_window() {
    let state = transition(PopupState::Hidden, PopupEvent::Notify);
    let state = transition(state, PopupEvent::HoverClose(true));
    let state = transition(state, PopupEvent::Closed);
    let state = transition(state, PopupEvent::Notify);
    assert_eq!(state, PopupState::Visible { hover_close: false });
}
```

Add an A/B/C burst test asserting one handle and payload C.

- [ ] **Step 2: Run tests and verify RED**

Run: `cargo test notify::app::tests --locked`

- [ ] **Step 3: Implement GPUI application and bridge**

Use `gpui_kit::application().with_quit_mode(QuitMode::Explicit)`. Install a futures unbounded sender in the global bridge and consume its receiver in `cx.spawn`. The foreground `PopupController` owns `Option<WindowHandle<PopupView>>` and all entity/window updates.

- [ ] **Step 4: Implement lifecycle and terminal GUI failure**

Notify opens or updates one window and calls `sync_geometry`; Close removes it and calls `cx.quit()` only for single-shot payloads. `on_window_closed` clears the matching handle and interaction state. Window creation failure clears `POPUP_AVAILABLE`, clears the bridge, and sends the current payload to system fallback once.

Wrap GPUI construction/run in `catch_unwind(AssertUnwindSafe(...))` and return `AppError::PlatformInit` without retrying GPUI per message.

- [ ] **Step 5: Verify**

```bash
cargo test notify::app::tests --locked -- --test-threads=1
cargo test notify::view::tests --locked -- --test-threads=1
cargo check --all-targets --locked
```

- [ ] **Step 6: Commit Task 4**

```bash
git add src/notify/app.rs src/notify/mod.rs
git diff --cached --check
git commit -m "feat(popup): 迁移 GPUI 应用生命周期"
```

### Task 5: Integrate service startup and fallback behavior

**Files:**
- Modify: `src/main.rs`, `src/send.rs`, `src/server.rs`, `src/tests_http.rs`
- Modify: `src/notify/mod.rs`, `src/notify/fallback.rs`

**Interfaces:**
- Consumes: `app::{run_service, run_single, post}`.
- Produces: a service that remains reachable with `via=system` after GPUI failure.

- [ ] **Step 1: Write failing fallback integration tests**

Introduce a private startup seam:

```rust
trait GuiRunner {
    fn run_service(&self) -> Result<(), notify::app::AppError>;
}
```

With a failing fake, assert the port file is written once, `/health` stays 200, `/notify` reports `via=system` once, no second bind occurs, and `POPUP_AVAILABLE` stays false.

- [ ] **Step 2: Run tests and verify RED**

Run: `cargo test tests_http::gui_failure --locked`

- [ ] **Step 3: Refactor startup and single-shot fallback**

Start HTTP once with popup unavailable, write the port file, then run GPUI on the main thread. GPUI's ready callback flips availability true. If GPUI exits/errors/panics, flip it false and park the main thread while HTTP workers remain alive.

Single-shot mode builds `PopupPayload`; if GPUI fails before display, call `SystemPresenter` exactly once and exit according to its result.

- [ ] **Step 4: Rename internal body variables**

Rename `body_html` to `body_markdown` in Rust internals, logs and comments. The wire field stays `body`.

- [ ] **Step 5: Verify**

```bash
cargo test tests_http --locked
cargo test notify::fallback --locked
cargo check --all-targets --locked
```

- [ ] **Step 6: Commit Task 5**

```bash
git add src/main.rs src/send.rs src/server.rs src/tests_http.rs src/notify/mod.rs src/notify/fallback.rs
git diff --cached --check
git commit -m "fix(service): GPUI 失败时保持系统通知服务"
```

### Task 6: Remove iced and legacy HTML/X11 compatibility code

**Files:**
- Delete: `src/html/mod.rs`, `src/html/parser.rs`, `src/html/wrap.rs`
- Modify: `src/notify/popup.rs`, `src/notify/window_icon.rs`, `build.rs`, `Cargo.toml`, `Cargo.lock`
- Create: `scripts/ci/check-no-iced.sh`

**Interfaces:**
- Consumes: completed GPUI app/view/window and Markdown modules.
- Produces: zero iced references in production source/dependencies.

- [ ] **Step 1: Add a failing migration guard**

```bash
#!/usr/bin/env bash
set -euo pipefail
! rg -n '\biced(::|\s*=)|iced_' Cargo.toml Cargo.lock src build.rs
```

Run `scripts/ci/check-no-iced.sh` and confirm failure while iced remains.

- [ ] **Step 2: Remove legacy code**

Delete `mod html;`, `src/html/**`, iced, and iced-only features. Move all GPUI geometry to `notify/window.rs`. Simplify Linux `window_icon.rs` to decode `Arc<image::RgbaImage>` only; delete title lookup, EWMH mutation, retries, and flush logic.

- [ ] **Step 3: Audit migration**

```bash
scripts/ci/check-no-iced.sh
cargo tree -i iced || test $? -eq 101
rg -n 'html::|body_html|HTML 子集|<b>|<font|<span style' src README.md assets sdk/js --glob '!**/node_modules/**' --glob '!**/dist/**'
```

Expected: no production iced or legacy HTML path; explicit migration tests/notes may mention old syntax.

- [ ] **Step 4: Run Rust verification**

```bash
cargo fmt --all -- --check
cargo test --all-targets --locked -- --test-threads=1
cargo clippy --all-targets --all-features --locked -- -D warnings
git diff --check
```

- [ ] **Step 5: Commit Task 6**

```bash
git add Cargo.toml Cargo.lock build.rs src scripts/ci/check-no-iced.sh
git diff --cached --check
git commit -m "refactor(gui): 删除 iced 与旧 HTML 窗口链路"
```

### Task 7: Update SDK, demos, docs, packaging, and runtime evidence

**Files:**
- Modify: `README.md`, `assets/demo.html`, `assets/sdk-使用手册.md`
- Modify: `sdk/js/packages/demo/index.html`, `sdk/js/packages/demo/src/main.ts`
- Modify: `sdk/js/packages/sdk/src/types.ts`, `sdk/js/packages/sdk/test/sdk.test.mjs`
- Modify if required by observed build errors: `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `scripts/pack-linux.sh`, `scripts/pack-windows.sh`

**Interfaces:**
- Produces: public examples and packages consistently using Markdown and documenting GPUI runtime requirements.

- [ ] **Step 1: Write failing SDK and embedded-demo assertions**

Change the SDK test payload from `<b>紧急</b>` to `**紧急**` and assert exact passthrough. Add HTTP static-page assertions that the embedded demo contains Markdown examples and no HTML-subset help text.

- [ ] **Step 2: Run tests and verify RED**

```bash
cd sdk/js
pnpm -F @hexinfo/x-notify-service-sdk test
cd ../..
cargo test tests_http::embedded --locked
```

Expected: embedded-copy assertion fails before demo/docs edits.

- [ ] **Step 3: Update examples and documentation**

Use:

```js
await svc.notify({
  title: '工单提醒',
  body: '待办通知 **1** 条\n\n15:21:05',
  width: 220,
  height: 100,
})
```

Document CommonMark/GFM structures, disabled navigation/remote images, unified body color, HTML incompatibility, GPU fallback, and platform runtime caveats.

- [ ] **Step 4: Adjust packaging only from evidence**

Build every target and add only system packages GPUI actually reports missing. Linux must preserve the glibc 2.28 floor. Document the graphics-runtime requirement in the bundled guide. Do not add speculative packages.

- [ ] **Step 5: Run full local verification**

```bash
cargo fmt --all -- --check
cargo test --all-targets --locked -- --test-threads=1
cargo clippy --all-targets --all-features --locked -- -D warnings
scripts/ci/check-no-iced.sh
cd sdk/js
pnpm -F @hexinfo/x-notify-service-sdk build
pnpm -F @hexinfo/x-notify-service-sdk test
pnpm typecheck
pnpm lint
cd ../..
git diff --check
```

- [ ] **Step 6: Perform macOS runtime acceptance**

Capture ignored evidence under `target/visual-evidence/` for:

1. default 220×100 Markdown popup;
2. custom size and four colors;
3. bold, link text, list, inline code, raw `<b>` literal, and image alt text;
4. hover, close, reopen without hover residue;
5. rapid notifications leaving only the last payload;
6. service staying live after close.

- [ ] **Step 7: Run cross-platform CI**

Wait for Linux x86_64, Linux aarch64, and Windows packaging. Record Windows taskbar/topmost and Linux X11/Wayland runtime as unverified until target-machine evidence exists; CI is not runtime proof.

- [ ] **Step 8: Commit Task 7**

```bash
git add README.md assets sdk/js .github scripts/pack-linux.sh scripts/pack-windows.sh
git diff --cached --check
git commit -m "docs: 更新 GPUI Markdown 接入与打包说明"
```

### Task 8: Whole-branch review and completion audit

**Files:**
- Review: all changes since `9f0c148`
- Test: all commands below

**Interfaces:**
- Produces: reviewed implementation ready for integration/release.

- [ ] **Step 1: Request a fresh whole-branch review**

Give the reviewer the spec, this plan, base `9f0c148`, and current HEAD. Require review of lifecycle races, foreground ownership, raw handles, Markdown safety, fallback exactly-once behavior, and packaging dependencies.

- [ ] **Step 2: Fix every Critical and Important finding test-first**

For each accepted issue: add a failing regression test, confirm RED, implement the smallest fix, then rerun the affected suite. Do not bundle unrelated cleanup.

- [ ] **Step 3: Run completion audit**

```bash
git diff 9f0c148...HEAD --check
scripts/ci/check-no-iced.sh
cargo test --all-targets --locked -- --test-threads=1
cargo clippy --all-targets --all-features --locked -- -D warnings
git status --short
```

Independently confirm API/SDK fields, Markdown semantics, one-window lifecycle, fallback, GPUI dependency, default/custom size/colors, local screenshots, and cross-platform CI.

- [ ] **Step 4: Commit review fixes only when needed**

Stage the exact files changed for each accepted review finding while implementing Step 2, then run:

```bash
git diff --cached --check
git commit -m "fix(gpui): 修复迁移审查问题"
```

Omit this commit when the reviewer reports no actionable findings. Preserve `.zcodeignore` throughout.
