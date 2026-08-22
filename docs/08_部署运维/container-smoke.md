# 容器实机冒烟

## 1. 目的

验证 `Dockerfile`、运行时镜像、持久化 volume 和运维探针能在真实 Docker daemon
上端到端工作。该检查不使用 mock server，也不把“本机没有 Docker”判定为通过。

## 2. 执行

在 Docker 主机或 CI runner 上执行：

```bash
make container-smoke
make compose-smoke
```

`container-smoke` 验证直接 `docker run` 路径；`compose-smoke` 验证真实
`docker-compose.yml` 路径。脚本执行以下步骤：

直接容器检查会：

1. 构建仓库根目录 `Dockerfile`，默认标签为 `openaip-os:0.1.0`；
2. 创建临时 named volume，并断言容器以非 root 用户 `oaip` 运行；
3. 将容器端口临时映射到宿主机 `127.0.0.1:18081`；
4. 注入一次性 smoke 管理员凭据；
5. 等待 `/readyz` 通过，然后验证 `/healthz`、`/readyz` 和 `oaip_ready 1`；
6. 断言 Docker HEALTHCHECK 最终为 `healthy`；
7. 发送容器停止信号并清理容器、临时 volume。

Compose 检查额外会：

1. 执行 `docker compose config --quiet`，验证变量插值和编排文件；
2. 使用独立 project name、临时端口和 named volume 执行 `docker compose up --build`；
3. 断言 `/var/lib/oaip` 是 volume 挂载且 Compose healthcheck 最终为 `healthy`；
4. 执行 `docker compose stop` 和 `down --volumes`，确保停止与清理路径可用。

可以使用环境变量覆盖镜像和端口：

```bash
OAIP_IMAGE=openaip-os:phase3-candidate \
OAIP_CONTAINER_SMOKE_PORT=18181 \
make container-smoke

OAIP_COMPOSE_SMOKE_PORT=18182 make compose-smoke
```

## 3. 结果判定

- **通过**：镜像构建成功、容器在 60 秒内 ready、三个探针断言通过、Docker HEALTHCHECK 为 `healthy`、容器正常停止；
- **环境错误**：脚本返回码 `2`，表示 Docker CLI、Docker daemon 或 curl 不可用；
- **产品失败**：脚本返回码非 `0` 且非 `2`，必须保留容器日志并调查镜像、启动参数、数据库
  初始化或审计链问题。

CI 的 `Phase 3 Docker runtime preflight`、`Phase 3 container smoke` 和 `Phase 3 Compose smoke`
步骤必须保留为真实 Docker 验证，不得改成只做 `docker build` 或静态 YAML 检查。
preflight 日志记录 Docker client/server、daemon 和 Compose v2 版本，随后执行
`make release-manifest` 并上传 `phase3-deployment-evidence`，保留 binary、schema、Git、
容器运行时和容器配置的证据清单。

## 4. 故障排查

```bash
docker ps -a --filter name=oaip-container-smoke-
docker logs <container>
docker image inspect openaip-os:0.1.0
docker volume ls --filter name=oaip-container-smoke-
```

脚本失败时会自动清理临时容器和 volume；如 Docker daemon 在清理阶段异常退出，需人工删除
残留资源。
