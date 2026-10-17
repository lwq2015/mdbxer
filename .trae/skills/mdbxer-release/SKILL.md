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

## 发布流程（四步推送 + 必做结果检查）

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
  └─ macos-14 → mdbxer-aarch64-apple-darwin.tar.gz
  → github-release 任务建 GitHub Release 并挂三个产物
  → gitee-release 任务尝试回传 Gitee Release（continue-on-error，网络超时失败不阻塞）
```

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
- Gitee-release 回传任务失败不算整体失败（continue-on-error），只要 GitHub Release 三附件齐就算成功
- 若 GitHub Release 缺附件但 run success，检查 workflow 中 upload/release 步骤输出后再报告
- 用户下载入口：README 指向 GitHub Releases（主）、Gitee Releases（镜像）

## 关键配置（已就绪，勿重复配）

| 项 | 位置 | 说明 |
|---|---|---|
| GitHub 仓库 | `lwq2015/mdbxer`（Public） | 镜像仓库 |
| workflow | `.github/workflows/build-release.yml` | 触发条件：push tag `v*` |
| Gitee 镜像 | Gitee 仓库 → 管理 → 仓库镜像 → Push 镜像 | tag 自动同步到 GitHub |
| GitHub Secret `GITEE_TOKEN` | GitHub → Settings → Secrets → Actions | Gitee Release 回传用（可选） |

## 注意事项

- **版本号必须递增**：用户明确要求，见 Cargo.toml
- macOS 只有 ARM 包（GitHub 无 Intel macOS runner），Intel Mac 走 Rosetta 2
- GitHub Actions runner 访问 Gitee API 经常超时，`gitee-release` 已设为可选，失败不影响整体发布
- 若需紧急补发 Gitee Release（用户说"回传 Gitee"）：令牌在用户环境变量 `GITEE_TOKEN`（未设置时向用户要）。流程：① `curl.exe -sL` 从 GitHub Release 下三附件到 `%TEMP%\gitee_upload`（GitHub 直连慢，用 `--retry`，校验字节数）② POST `/api/v5/repos/lwq_yu/mdbxer/releases?access_token=` 建 Release（JSON 无 BOM、必带 target_commitish=master）③ `curl.exe -F access_token= -F file=@` 传到 `/releases/{id}/attach_files` ④ 验证用 GET `/releases` 列表过滤 tag_name（按 tag 单查返回空）；Gitee 会自动附 2 个源码包（v*.zip/tar.gz），共 5 个 assets 属正常
