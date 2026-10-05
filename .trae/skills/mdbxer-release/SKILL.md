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

## 发布流程（四步）

```powershell
# 1. 升版本号：编辑 Cargo.toml 的 version = "x.y.z"，Cargo.lock 中 mdbxer 包条目同步改
# 2. 提交版本号
git add Cargo.toml Cargo.lock
git commit -m "版本升至 v0.1.x"
# 3. 打 tag（annotated）
git tag -a v0.1.x -m "v0.1.x"
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

## 发布后验证

- GitHub Release：`https://github.com/lwq2015/mdbxer/releases/tag/v0.1.x` 应有三个附件
- 用户下载入口：README 中指向 GitHub Releases（主），Gitee Releases（镜像）

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
- 若需紧急补发 Gitee Release，用新 Gitee 令牌本地调 API 建 Release 并传附件（产物可从 GitHub Release 下载）
