## 变更说明

<!-- 说明改了什么、为什么改，以及影响的领域。 -->

## 验证

- [ ] `pnpm check`
- [ ] `cargo fmt --check --manifest-path src-tauri/Cargo.toml`（涉及 Rust 时）
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings`（涉及 Rust 时）
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --all-features`（涉及 Rust 或数据契约时）
- [ ] 手动验证 Tauri 模式（涉及文件、系统能力、同步或 AI 时）

## 文档与数据

- [ ] 已更新相关文档或确认无需更新
- [ ] 已补充/更新回归测试
- [ ] 未提交构建产物、用户数据、密钥或机器专属路径
- [ ] 如涉及持久化格式，已说明兼容和备份恢复影响

## 风险与回滚

<!-- 说明已知风险、兼容性影响和回滚方式。无风险请写“无”。 -->
