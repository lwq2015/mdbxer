---
name: mdbxer-restart
description: Stop the running mdbxer app, build the release binary, and relaunch it in the k:\mdbx\mdbxer workspace, reporting the new PID. Use when the user asks to 重启、重新构建重启、重建并运行 mdbxer, or after finishing Rust/egui code changes they want to verify. Do not use without an explicit restart request — never restart merely because code was edited.
---

# mdbxer 构建重启

工作区固定为 `k:\mdbx\mdbxer`，环境为 Windows PowerShell（不要用 bash heredoc 语法）。

## 标准流程（一条命令完成）

```powershell
Get-Process -Name mdbxer -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep -Milliseconds 600; cargo build --release 2>&1 | Select-Object -Last 3; Start-Process -FilePath "K:\mdbx\mdbxer\target\release\mdbxer.exe" -WorkingDirectory "K:\mdbx\mdbxer"; Start-Sleep -Milliseconds 700; Get-Process -Name mdbxer | Select-Object Id, ProcessName
```

- `cwd` 必须是 `k:\mdbx\mdbxer`，`timeout` 给 600000。
- PowerShell 把 cargo 的 stderr 包装成红色 NativeCommandError 是正常现象，只要末尾出现 `Finished release profile` 即为成功。
- 最后一行输出新进程 PID，向用户回报该 PID。

## 规则

1. **只有用户明确要求重启时才执行**。用户只是评价、闲聊或未要求运行时，改完代码即停止，不要自行构建/重启。
2. 重启前确认 `cargo check` 已通过（单独跑，`timeout` 300000）。
3. 若改动了测试数据生成器，需要重新生成测试库：**必须先停掉 mdbxer**（Windows 下单文件被占用会导致覆盖失败），再执行：
   ```powershell
   Get-Process -Name mdbxer -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep -Milliseconds 700; cargo run --example make_test_db 2>&1 | Select-Object -Last 4
   ```
   成功输出"已生成：testdata\dir_db / testdata\file_db.mdbx"，之后再按标准流程构建重启。
4. 提交代码属于独立动作，用户没要求 `git commit` 时不得顺手提交；提交信息用 PowerShell here-string（`@' ... '@`），不要用 bash 的 `<<'EOF'`。
