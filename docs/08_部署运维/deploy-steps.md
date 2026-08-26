# 部署步骤

## 1. 从 release 产物启动

当前 Phase 3 交付两个可复制入口：

- 宿主机 release binary：由 `make release-manifest` 默认构建到仓库 `target/release/oaip`；
- 容器编排：仓库根目录 `Dockerfile` + `docker-compose.yml`。

该命令会先构建默认产物，再生成发布证据清单；显式提供 `OAIP_BIN` 时使用外部产物：

```bash
make release-manifest
```

清单默认写入 `target/release-manifest.json`，包含版本、schema、Git commit、工作区是否
dirty、binary SHA-256 和容器镜像标签。工作区有未提交修改时，清单状态必须是 `candidate`，
不能被当作正式 release。

容器入口是一条命令：

```bash
export WB_BOOTSTRAP_ADMIN_USER=admin
export WB_BOOTSTRAP_ADMIN_PWD='change-me-before-production'
docker compose up -d --build
```

服务必须通过 `GET /healthz`、`GET /readyz` 后才视为可接流量。`/readyz` 返回 503 时
不得通过重试绕过数据或审计链问题。

## 2. 上线检查清单

- [ ] release tag、changelog、部署文档、回滚方案版本一致
- [ ] `make ops-rehearsal` 已使用真实旧/新 binary 和升级前快照通过
- [ ] 管理员凭据从 secret 注入，未写入镜像或仓库
- [ ] 数据 volume 已挂载且有最近一次可读备份
- [ ] `curl --fail http://127.0.0.1:8080/healthz` 通过
- [ ] `curl --fail http://127.0.0.1:8080/readyz` 通过
- [ ] `/metrics` 已被监控系统抓取
- [ ] 前置代理启用 HTTPS/TLS，应用端口不直接对公网暴露
- [ ] 生产部署完成双人复核并留痕

## 3. 停机与恢复

发送 SIGTERM（`docker compose stop` 默认行为）后，服务进入优雅关闭路径：停止接收新请求，
回收 Fabric 在途适配器/后台进程，再退出。数据库事务本身由 libSQL 事务原子性保证；
恢复后 `recover` 会把崩溃残留的运行状态和挂起审批重新归类。

## 4. 发布冒烟

```bash
make ops-smoke
```

该脚本启动真实 release binary，验证三个 HTTP 端点并发送 SIGTERM；服务停止后还会对同一
数据库执行一次备份、恢复和审计事件读取，覆盖运维命令的最小闭环。它不使用 mock server，
也不会在服务运行期间复制数据库文件。

容器实机验证：

```bash
make container-smoke
make compose-smoke
```

前一个脚本验证直接 `docker run`，后一个脚本验证实际 `docker-compose.yml` 的配置解析、
环境变量、named volume、healthcheck、HTTP 探针和停止路径。Docker daemon 不可用时脚本
明确返回环境错误，不会伪造通过结果。

容器部署的实机证据与故障排查见 [container-smoke.md](container-smoke.md)。CI 成功后应
下载 `phase3-deployment-evidence` artifact，核对其中的 `target/release-manifest.json`。
