# Tauri Dev 启动 Panic 排查复盘

日期：2026-05-22

## 现象

运行桌面端开发命令：

```bash
pnpm --filter @syncmind/desktop tauri dev
```

Rust 编译完成后，应用在启动阶段退出，终端只显示：

```text
thread 'main' panicked at library/core/src/panicking.rs:225:5:
panic in a function that cannot unwind
thread caused non-unwinding panic. aborting.
```

该 panic 发生在 macOS 的 Objective-C 回调边界内，普通 `cargo check` 和部分单元测试无法复现。

## 背景

问题最初被怀疑由提交 `684466fc75cb8d0490b11190e742f2be82e4d63a` 引起。该提交主要改动了文件监听链路：

- watcher 从 `Vec<PathBuf>` 改为 `Vec<FileEvent>`
- 增加删除和重命名事件处理
- indexing pipeline 对 `Remove` 事件执行索引清理
- storage 增加 `delete_file_by_path`

排查过程中确实发现 watcher 代码存在独立问题，但它不是最终导致 `tauri dev` 启动 panic 的直接根因。

## 排查过程

1. 先检查提交 diff，重点关注 `core/file-watcher`、`core/syncmind-indexing`、`core/storage`、`apps/desktop/src-tauri`。
2. 跑 `cargo check` 时发现当前工作区还有两个构建阻塞：
   - `syncmind-rag-engine` 使用了 `tree_sitter_go`，但依赖和 lock 中缺少 `tree-sitter-go`
   - `syncmind-core::Config` 缺少调用方已经使用的日志和 ONNX 字段
3. 修复上述构建阻塞后，跑 watcher 测试发现真实 OS 文件事件测试不稳定，并进一步发现 `FileWatcher::new` 是同步 API，但内部依赖 `tokio::spawn` 当前 runtime。
4. 对 watcher 做了独立修复：
   - 有当前 Tokio runtime 时使用当前 runtime
   - 没有 runtime 时创建专用后台 runtime
   - 底层 watch 父目录，同时用注册文件集合过滤事件
   - 删除和重命名事件保留正确路径语义
5. 但用户再次运行 `tauri dev` 后仍然 panic，说明前面的验证不充分：只验证了编译和单元测试，没有验证真实启动路径。
6. 使用完整命令复现并打开 backtrace：

```bash
RUST_BACKTRACE=full pnpm --filter @syncmind/desktop tauri dev
```

backtrace 显示 panic 穿过：

```text
tao::platform_impl::macos::app_delegate::did_finish_launching
```

这说明 panic 发生在 Tauri/macOS `applicationDidFinishLaunching` 的 Objective-C 回调边界内。

7. 加定阶段日志位 `setup` 内部执行进度，最终确认崩溃发生在窗口创建成功之后、设置 macOS collection behavior 时：

```rust
let _: () = msg_send![ns_window, setCollectionBehavior: behavior];
```

## 根因

最终根因是 `apps/desktop/src-tauri/src/lib.rs` 中对 `NSWindow` 调用：

```rust
setCollectionBehavior:
```

该调用位于 macOS Objective-C FFI 边界内。它在 Tauri dev 启动的 `applicationDidFinishLaunching` 回调中触发不可展开的异常/panic，Rust 无法跨 `extern "C"` 边界 unwind，于是表现为：

```text
panic in a function that cannot unwind
```

这段代码的目的只是防止 palette window 在显示时切换 Space，属于增强体验，不是启动必要路径。

## 修复

修复方式是移除这段不安全的 `setCollectionBehavior` 调用，避免在 macOS delegate 启动阶段触碰容易 abort 的 ObjC 调用。

同时保留 watcher 相关的独立修复，因为它们确实修正了删除/重命名事件处理里的问题：

- `FileWatcher::new` 不再假设调用线程已有 Tokio runtime
- watcher 以父目录为底层 watch 目标，避免删除/重命名事件丢失
- `classify_event` 只转发注册文件相关事件，避免同目录无关文件触发索引
- 删除/重命名路径同时处理 raw path 和 canonical path 差异

另外修复当前工作区构建问题：

- `core/rag-engine/Cargo.toml` 补回 `tree-sitter-go`
- `core/syncmind-core/src/config.rs` 补回日志、ONNX URL 等字段

## 验证

完整启动验证：

```bash
RUST_BACKTRACE=full pnpm --filter @syncmind/desktop tauri dev
```

结果：应用成功启动并保持运行超过 45 秒，没有再出现 panic。

其他验证：

```bash
cd core && cargo check
cd core && cargo test -p syncmind-file-watcher -- --nocapture
cd apps/desktop/src-tauri && cargo check
```

结果：

- core `cargo check` 通过
- `syncmind-file-watcher` 测试 5/5 通过
- desktop Tauri crate `cargo check` 通过
- 仍有 `objc` crate 的 `unexpected cfg condition value: cargo-clippy` warning，但它不是启动 panic 的根因

## 经验教训

- 对 GUI/Tauri/macOS 启动问题，`cargo check` 不足以证明修复；必须跑真实启动命令。
- `panic in a function that cannot unwind` 往往意味着 panic/异常穿过 FFI 回调边界，需要从 delegate/callback 方向排查。
- Objective-C `msg_send!` 调用应尽量收敛在必要路径内，启动阶段尤其要避免可选增强逻辑导致整个应用 abort。
- 对平台相关 GUI 行为，不应只用单元测试验证，至少要跑一次 `tauri dev` 或等价启动流程。
