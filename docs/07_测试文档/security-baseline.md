# 供应链安全基线（cargo audit / npm audit）

| 项 | 值 |
|----|----|
| 采集日期 | 2026-08-18 |
| 工具 | cargo-audit v0.22.2 / npm audit |
| **cargo audit 结论** | 5 vulnerabilities + 2 unmaintained，**全部经 libsql v0.9.30 传递引入** |
| **npm audit 结论** | **0 漏洞**（vite 5→7 升级后） |

## cargo audit 明细与处置

| Advisory | 传递路径 | 处置 |
|----------|----------|------|
| RUSTSEC-2026-0258（h2 0.3.27） | libsql → hyper → h2 | 记录忽略（见下） |
| RUSTSEC-2026-0104（rustls-webpki 0.102.8） | libsql → hyper-rustls → rustls → webpki | 记录忽略（见下） |
| RUSTSEC-2026-0098（rustls-webpki） | 同上 | 同上 |
| RUSTSEC-2026-0099（rustls-webpki） | 同上 | 同上 |
| RUSTSEC-2026-0049（rustls-webpki 0.102.8） | libsql → hyper-rustls → rustls → webpki | 记录忽略（见下） |
| RUSTSEC-2025-0141（bincode 1.3.3） / 0134（rustls-pemfile 2.2.0） | libsql 内部依赖 | 记录忽略 |

### 忽略理由（可复核）

1. **不可升级**：本项目固定使用 libsql 0.9.30；当前可见的 0.10.0-pre.4 仍使用 hyper 0.14 / h2 0.3，不能通过小版本更新消除 h2 advisory。h2 修复版要求升级到 0.4.x，而 libsql 0.9.x 的网络栈不支持该替换；rustls-webpki 修复版需要 rustls 0.23+，libsql 0.9.x 锁死 rustls 0.22。
2. **不可达**：OpenAIP-OS 使用 libsql **本地文件库**（`LibSQLStorage::open(path)`），
   不配置 remote replica，不发起网络 TLS 连接——h2 的 HTTP/2 解析路径和 rustls/webpki 证书验证路径均不执行；
   bincode 仅用于本地序列化，不解析外部输入。
3. **升级路径**：待 libsql 发布新版本（或引入 `libsql-experimental`/新 API）后，
   `cargo update` 复查并移除 ignore 列表。

### CI 使用方式（白名单随本文件同步维护）

```bash
cargo audit --ignore RUSTSEC-2026-0258 --ignore RUSTSEC-2026-0104 \
            --ignore RUSTSEC-2026-0098 --ignore RUSTSEC-2026-0099 \
            --ignore RUSTSEC-2026-0049 --ignore RUSTSEC-2025-0141 \
            --ignore RUSTSEC-2025-0134
```

新出现（不在白名单）的 advisory 会使 CI 失败——这正是目的。

## npm audit

`npm audit --audit-level=high` 0 漏洞（vite 7.3.6 / plugin-react 5.2.0 / TS 5.9.3 / vitest 4）。
