# Docker Compose 编排

## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-08-002 |
| 所属模块 | OpenAIP-OS-Ops |
| 版本 | V1.1 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 运行形态

Phase 3 采用单机 Local-First 形态：一个 OpenAIP-OS 容器、一个持久化 volume、一个
本地 libSQL 文件。数据库不暴露端口，HTTP 默认只绑定宿主机 `127.0.0.1:8080`；
生产对外暴露必须在前置 Nginx/TLS 终止层完成。

```bash
export WB_BOOTSTRAP_ADMIN_USER=admin
export WB_BOOTSTRAP_ADMIN_PWD='change-me-before-production'
docker compose up -d --build
docker compose ps
curl --fail http://127.0.0.1:8080/readyz
```

## 2. 固定版本与配置

- `docker-compose.yml` 使用固定镜像标签 `openaip-os:0.14.1`，禁止 `latest`。
- HTTP 默认绑定 `127.0.0.1:8080`；实机冒烟通过 `OAIP_HTTP_PORT` 使用临时宿主机端口，
  不改变容器内的 `8080`。
- `Dockerfile` 使用固定的 Node、Rust、Debian 版本标签，并在构建阶段安装仓库声明的
  `nightly-2026-07-17` 工具链。
- 管理员凭据只通过 `WB_BOOTSTRAP_ADMIN_USER` / `WB_BOOTSTRAP_ADMIN_PWD` 注入，
  不写入镜像、不提交 `.env`。
- 数据写入 named volume `oaip-data:/var/lib/oaip`。
- Compose 明确设置 `stop_grace_period: 20s`，给 SIGTERM 优雅回收 Fabric/后台进程留出固定窗口。

## 3. 探针

| 端点 | 用途 | 鉴权 | 成功条件 |
|---|---|---|---|
| `GET /healthz` | 进程存活 | 免鉴权 | HTTP 200 |
| `GET /readyz` | 服务可接流量 | 免鉴权 | schema 已初始化且审计链校验通过，HTTP 200 |
| `GET /metrics` | Prometheus 文本指标 | 免鉴权 | HTTP 200；`oaip_ready` 反映 readiness |

`/readyz` 在审计链损坏或存储读取失败时返回 HTTP 503，不能以“进程仍在”代替
“服务可用”。

## 4. 停止与数据

```bash
docker compose stop
# 备份前必须停止服务；Compose 已配置 /usr/local/bin/oaip entrypoint，不要重复传入二进制路径
mkdir -p backups
docker compose run --rm -v "$PWD/backups:/backup" app \
  --db /var/lib/oaip/openaip.db \
  backup /backup/openaip-$(date -u +%Y%m%dT%H%M%SZ).db
```

恢复流程遵循 `backup-restore.md`；不要在服务运行期间直接复制数据库、WAL 或 SHM 文件。

## 5. 关联产物

- `Dockerfile`
- `docker-compose.yml`
- `deploy-steps.md`
- `monitor-alert.md`
- `rollback-plan.md`
- `backup-restore.md`

## 6. 实机冒烟

```bash
make container-smoke
make compose-smoke
```

`container-smoke` 验证直接 `docker run` 路径；`compose-smoke` 验证真实 Compose 文件、
环境变量插值、镜像版本一致性、named volume、healthcheck、数据跨停启持久化和停止路径。
两个检查都会真实构建镜像。

该检查会真实构建镜像、启动临时容器和 named volume，验证 `/healthz`、`/readyz`、
`/metrics`，再发送停止信号并清理资源。完整步骤、返回码和故障排查见
[container-smoke.md](container-smoke.md)。
