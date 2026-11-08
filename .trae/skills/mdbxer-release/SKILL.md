---
name: mdbxer-release
description: 发布 mdbxer 新版本——升版本号、提交、打 tag、推送触发 GitHub Actions 自动构建三平台并发布 Release。当用户说"发版/发布新版本/release"时使用。不要在用户未明确说发版时执行。
---

# mdbxer 版本发布

工作区 `k:\mdbx\mdbxer`。发布采用 GitHub Actions 全自动流程，**每次发版必须递增版本号**。

## 前置确认

1. 用户明确说了"发版 / 发布 / release"才执行
2. 本地工作区干净（`git status --short` 无未提交改动）
3. 本地 master 与 Gitee origin/master 同步

## 发布流程（四步推送 + 结果检查 + 同步 Gitee）

```powershell
# 1. 升版本号：编辑 Cargo.toml 的 version = "x.y.z"，Cargo.lock 中 mdbxer 包条目同步改
# 2. 提交版本号
git add Cargo.toml Cargo.lock
git commit -m "版本升至 v0.1.x"
# 3. 打 tag（annotated）：tag message = Release 正文（GitHub/Gitee 均自动取用）
#    必须加 --cleanup=verbatim：git 默认剥掉 # 开头的行（markdown 标题会丢失）
#    message 用 PowerShell here-string 写功能清单（markdown：## 小节 + - 列表）
git tag -a v0.1.x -m $msg --cleanup=verbatim
# 4. 推送 master + tag 到 Gitee
git push origin master
git push origin v0.1.x
```

## 自动链路（推送后无需人工）

```
Gitee 收到 tag → 镜像自动同步到 GitHub → Actions 触发构建与发布 workflow
  ├─ windows-latest → mdbxer-x86_64-pc-windows-msvc.zip
  ├─ ubuntu-latest → mdbxer-x86_64-unknown-linux-gnu.tar.gz
  └─ macos-latest → mdbxer-aarch64-apple-darwin.tar.gz
  → github-release 任务建 GitHub Release 并挂三个产物
```

workflow 已不再回传 Gitee：**GitHub Release 成功后必须手动同步 Gitee Release**（见「同步 Gitee Release（每版必做）」）。

构建耗时约 5-10 分钟（GitHub 首次下载 Rust 依赖慢，有缓存后快）。

## 结果检查（必做，推送后立即启动，不等用户问）

仓库是 Public，GitHub REST API 匿名即可查（无需 gh 登录、无需 token）。

**流程：推送完成后立刻开始轮询，直到构建结束并验证 Release，最后向用户报告结论（成功=三个附件链接；失败=失败步骤与日志要点）。**

```powershell
# 1) 查最近 workflow run（注意：Gitee 镜像同步到 GitHub 有 1-2 分钟延迟，
#    刚推送时查不到对应 run 属正常，继续轮询）
$runs = (Invoke-RestMethod "https://api.github.com/repos/lwq2015/mdbxer/actions/runs?per_page=3").workflow_runs
$runs | Select-Object display_title, status, conclusion, created_at, html_url | Format-List

# 2) 轮询直到 status == "completed"（每 60-90 秒查一次，总上限约 20 分钟；
#    conclusion 应为 "success"）
# 3) 构建成功后验证 Release 挂了三个附件：
$rel = Invoke-RestMethod "https://api.github.com/repos/lwq2015/mdbxer/releases/tags/v0.1.x"
$rel.assets | Select-Object name, size, browser_download_url
```

**判定标准：**
- ✅ 成功：run conclusion = `success`，且 Release `v0.1.x` 恰有三个附件（windows .zip / linux .tar.gz / macos .tar.gz）
- ❌ 失败：conclusion = `failure` 或 20 分钟仍 `in_progress`——用 jobs 接口定位失败步骤：
  ```powershell
  (Invoke-RestMethod "https://api.github.com/repos/lwq2015/mdbxer/actions/runs/<run_id>/jobs").jobs |
    Select-Object name, status, conclusion
  ```
  向用户报告失败 job / step 名称和 run 链接，**不要静默重试**。

**注意事项：**
- 若 GitHub Release 缺附件但 run success，检查 workflow 中 upload/release 步骤输出后再报告
- 用户下载入口：GitHub Releases（主）、Gitee Releases（镜像，每版必同步）

## 关键配置（已就绪，勿重复配）

| 项 | 位置 | 说明 |
|---|---|---|
| GitHub 仓库 | `lwq2015/mdbxer`（Public） | 镜像仓库 |
| workflow | `.github/workflows/build-release.yml` | 触发条件：push tag `v*` |
| Gitee 镜像 | Gitee 仓库 → 管理 → 仓库镜像 → Push 镜像 | tag 自动同步到 GitHub |
| 本地环境变量 `GH_TOKEN` | 用户机器（已设置） | 可直接向 GitHub 推 tag（见下方「镜像不触发时的兜底」） |
| 本地环境变量 `GITEE_TOKEN` | 用户机器（已设置） | 同步 Gitee Release 用 |

## 注意事项

- **版本号必须递增**：用户明确要求，见 Cargo.toml
- macOS 只有 ARM 包（GitHub 无 Intel macOS runner），Intel Mac 走 Rosetta 2
- **`macos-14` 镜像已不可用**：作业会在 0 秒被 cancelled（无 runner、无日志），必须用 `macos-latest`
- **Gitee 镜像对「tag 删除后重建」不产生 GitHub push 事件**：改了 tag 想重新构建时，镜像不会触发 workflow

### 镜像不触发时的兜底：直接推 GitHub

```powershell
# GH_TOKEN 有 repo 写权限；用 x-access-token + 禁用 credential helper
$u = "https://x-access-token:$env:GH_TOKEN@github.com/lwq2015/mdbxer.git"
git -c credential.helper= push $u refs/tags/v0.1.x
```

推送后 workflow 正常触发。注意不要把 token 打进日志/输出（回显前先 `-replace` 掉）。

## 同步 Gitee Release（每版必做，编码坑必读）

令牌在用户环境变量 `GITEE_TOKEN`（未设置时向用户要）。

```powershell
# 1) 下载三附件到 %TEMP%\gitee_upload（GitHub 直连慢，加 --retry，完成后校验字节数）
$files = @("mdbxer-x86_64-pc-windows-msvc.zip","mdbxer-x86_64-unknown-linux-gnu.tar.gz","mdbxer-aarch64-apple-darwin.tar.gz")
foreach ($f in $files) { curl.exe -sL --retry 3 "https://github.com/lwq2015/mdbxer/releases/download/v0.1.x/$f" -o "$env:TEMP\gitee_upload\$f" }

# 2) 正文 body 从 GitHub Release API 取（天然 UTF-8 正确），禁止在本地 PowerShell 拼中文
$gh = Invoke-RestMethod "https://api.github.com/repos/lwq2015/mdbxer/releases/tags/v0.1.x"

# 3) 建 Release（已存在则改为 PATCH /releases/{id} 更新 body）：
#    【致命坑】PS 5.1 的 Invoke-RestMethod 字符串请求体默认非 UTF-8，中文全变 ? 且不可逆丢失！
#    中文 JSON 必须写 UTF-8 无 BOM 文件后用 curl --data-binary 发送
$body = @{ access_token = $t; tag_name = "v0.1.x"; name = "v0.1.x"; body = $gh.body; target_commitish = "master" } | ConvertTo-Json
[IO.File]::WriteAllText("$env:TEMP\rel.json", $body, [Text.UTF8Encoding]::new($false))
curl.exe -s -X POST "https://gitee.com/api/v5/repos/lwq_yu/mdbxer/releases" -H "Content-Type: application/json; charset=utf-8" --data-binary "@$env:TEMP\rel.json"

# 4) 传附件（PS 5.1 无 -Form，必须 curl.exe -F）
curl.exe -s -X POST "https://gitee.com/api/v5/repos/lwq_yu/mdbxer/releases/<id>/attach_files?access_token=$t" -F "file=@$env:TEMP\gitee_upload\<文件>"

# 5) 验证（两个假象都别信）：
#    假象 A：Invoke-RestMethod 读 Gitee 响应显示 mojibake（ãé«çº§…）——只是 PS 按 Latin-1 解码响应的显示问题，
#            真实存储要看原始字节：curl.exe -s <url> -o file 后 [Text.Encoding]::UTF8.GetString([IO.File]::ReadAllBytes(file))
#    假象 B：assets 的 size 字段显示 0——不代表附件坏，实际下载一份比对 SHA256 与 GitHub 侧一致即完好
```

- 验证 Release 列表用 GET `/releases` 过滤 tag_name（按 tag 单查返回空）
- Gitee 自动附 2 个源码包（v*.zip/tar.gz），共 5 个 assets 属正常
- GitHub 直连下载偶发 **504 Gateway Time-out**（92 字节 HTML 假文件）：改用 API 资产接口兜底
  `curl -L "https://api.github.com/repos/lwq2015/mdbxer/releases/assets/<asset_id>" -H "Authorization: Bearer $env:GH_TOKEN" -H "Accept: application/octet-stream"`
- 同步完必须下载 Gitee 侧三个附件算 SHA256 与 GitHub 侧一致（`Get-FileHash`），别只看 size 字段
