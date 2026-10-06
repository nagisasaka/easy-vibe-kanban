# Lucky Vibe Kanban（LVK）

通过看板规划任务，运行编码代理，并审查代码变更。

LVK 基于 Easy Vibe Kanban 和 Vibe Kanban，支持 Git 工作树、多会话、可视化代理工作流、
变更整合验证以及 OpenWiki 仓库记忆。可以在本机运行，也可以部署到自己的服务器。
代理账户及模型费用需要单独配置。

[English](README.md) · [服务器部署指南](docs/self-hosting/server-container.mdx)

## 发布方式

LVK **仅发布 Docker 镜像**，通过 GHCR 分发，当前平台为 Linux amd64。
此分支不发布 npm 包。本机开发请使用下方的源码开发方式。

## 服务器运行

`deploy/server` 提供单用户 Docker 部署配置。使用固定公网 IP 即可，无需购买域名。
支持 Let's Encrypt 自动证书更新、HTTPS、Basic 认证和持久化数据卷。
服务器重启会停止正在执行的进程，持久化数据不等于自动恢复执行。

服务器上的源码开发使用独立开发容器，前端热更新、Rust 增量编译，日常测试无需重建镜像。
项目浏览器预览需要额外配置；Android/ADB 远程连接尚未提供。

## 源码开发

使用 `package.json` 中的 Node.js/pnpm 版本和 `rust-toolchain.toml` 中的 Rust 版本，
安装 `cargo-watch` 后执行：

```bash
pnpm install --frozen-lockfile
pnpm run dev
```

开发和验证范围见 [AGENTS.md](AGENTS.md)。独立的 `crates/remote` Rust 工作区包含上游私有依赖，
本地 LVK 开发不需要运行该工作区。

## 兼容性与来源

产品名称已变更为 Lucky Vibe Kanban。历史数据路径、`.evk-shared`、`EVK_*`/`VK_*`
环境变量和底层二进制名称保留，以维护兼容性。旧 npm 安装不会自动迁移到 Docker。

感谢 [Easy Vibe Kanban](https://github.com/toby1123yjh/easy-vibe-kanban)
和 [Vibe Kanban](https://github.com/BloopAI/vibe-kanban) 的贡献者。
项目使用 [Apache-2.0](LICENSE) 许可证，归属说明见 [NOTICE](NOTICE)。
