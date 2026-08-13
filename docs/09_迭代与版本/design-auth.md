# 技术设计：认证与 RBAC（V0.7 · 对应 P0 风险「无认证授权」）

| 版本 | V0.1（设计稿，待评审） |
| --- | --- |
| 关联风险 | P0：server.rs 无任何 auth 中间件，三层视图仅前端 `?view=` 裁剪 |
| 目标版本 | V0.7 |
| 状态 | 设计稿 · 未动工 |
| 作者 | lushaohui |

---

## 1. 背景与问题

现状（`src/server.rs` 全文件核对）：

- `router()`（line 222）所有 `/api/v1/wb/agent/*` 路由**零鉴权**，任何人可 `POST /run`、读全量 trace、`POST /approval/:req_id` 裁决。
- `View::from_param`（trace.rs:36）按客户端 `?view=executive|operator|developer` 裁剪时间线；服务端**不校验调用者身份**，角色完全由前端任意指定。
- 无登录、无会话、无用户表，角色仅存在于显示层枚举。

本地单机可用；一旦对外部署即暴露全部能力。这是产品化的「第一道门」。

## 2. 目标 / 非目标

**目标**

- 服务端强制认证：受保护路由无有效凭证 → `401`。
- 服务端按角色（Executive/Operator/Developer）裁剪能力，**不再信任前端 `view` 参数**。
- 角色与能力在**服务端**定义与执行；前端仅消费服务端下发的角色/视图。
- 凭证可撤销、可过期（支持「踢人」与失效）。

**非目标（本设计不覆盖）**

- 不实现 OIDC/SSO 对接（抽象 `IdentityProvider` trait 预留，V0.7 仅本地账号）。
- 不做细粒度对象级 ACL（如「只能看某 tenant 的 run」）；本设计是角色级能力门禁。
- 不处理 TLS 终止（见第 4 节分叉 D4）。

## 3. 设计

### 3.1 身份模型

V0.7 用**本地账号**：`wb_user` 表 + `argon2id` 口令哈希。抽象 `trait IdentityProvider { async fn verify(&self, user, pwd) -> bool; async fn user(&self, id) -> Option<User>; }`，本地实现 `LocalIdentityProvider`；OIDC 实现留待后续（同 trait 接入，不动中间件）。

### 3.2 令牌模型（推荐：不透明会话令牌）

- 登录成功 → 服务端生成 **32B 随机令牌**（`base64url`），写入 `wb_session`，返回 `Authorization: Bearer <token>`。
- 中间件每次请求查 `wb_session` + join `wb_user` 得到 `AuthUser{id, username, role, view}`；令牌缺失/过期/用户被禁用 → `401`。
- 优势：**可即时撤销**（删 `wb_session`）、**无签名密钥管理**、天然支持 `expires_at` 续期。
- 备选：JWT（HS256，无状态、但吊销需黑名单）。**本设计默认采用会话令牌**；分叉见 §4 D2。

### 3.3 中间件与请求流

axum `from_fn` 鉴权层置于 `router()` 之前：

```
pub async fn auth_layer(req: Request<Body>, next: Next) -> Response {
    // 公共白名单：静态资源、/auth/login、/auth/me(无凭证)
    if is_public(&req) { return next.run(req).await; }
    let Some(token) = bearer_token(&req) else { return unauthorized(); };
    match SESSION_STORE.verify(token).await {           // 查 wb_session + wb_user
        Ok(auth_user) => {
            let mut req = req;
            req.extensions_mut().insert(auth_user);     // 注入 AuthUser
            next.run(req).await
        }
        Err(_) => unauthorized(),                        // 401
    }
}
```

受保护 handler 用 `AuthUser` 提取器取代（或叠加）原 `State<Arc<AppState>>`：

```rust
async fn run_start(State(st): State<Arc<AppState>>, user: AuthUser, Json(body): Json<RunBody>) -> Response {
    if !user.can(Permission::TriggerRun) { return forbidden(); }   // 403
    // ... 原有逻辑
}
```

### 3.4 角色能力矩阵（服务端强制）

| 能力 | Developer | Operator | Executive |
| --- | --- | --- | --- |
| 触发 run（`POST /run`） | ✅ | ✅ | ❌ |
| 查看 trace（最大视图） | Developer | Operator | Executive |
| 裁决审批（`POST /approval/:id`） | ✅ | ✅ | ❌ |
| 读 Evolution / Audit | ✅ | ✅ | ❌（仅 Executive 摘要） |
| 撤销 Learned / 切换 AutoApprove（写 Evolution） | ✅ | ❌ | ❌ |
| 读 Memory | ✅ | ✅ | ✅（Executive 视图） |

`Permission` 枚举 + `AuthUser::can()` 集中判定；新增端点必须显式声明所需 `Permission`，否则编译期缺判定（用 `const` 表或 match 穷尽）。

### 3.5 服务端视图裁剪（取代前端 `view` 参数）

- `AuthUser.role` 推导**允许的最大视图**：Developer→Developer，Operator→Operator，Executive→Executive。
- 前端 `?view=` 仍可用，但被服务端**钳制到 ≤ 角色视图**：客户端请求更简视图（如 Developer 请求 Operator）允许；请求更细视图（如 Executive 请求 Developer）被静默降级为角色视图，**不报错、不越权**。
- 即：`effective_view = min(client_view, role_max_view)`。前端不再能「自提权限」。

### 3.6 登录 / 会话生命周期

| 端点 | 方法 | 说明 |
| --- | --- | --- |
| `/api/v1/wb/auth/login` | POST | `{username, password}` → `argon2id` 校验 → 建 `wb_session` → 返回 `{token, user:{role, view}}` |
| `/api/v1/wb/auth/logout` | POST | 删当前 `wb_session`（需鉴权） |
| `/api/v1/wb/auth/me` | GET | 返回当前 `AuthUser`（需鉴权，供前端初始化角色） |

- 会话 TTL 默认 12h，每次请求续期 `expires_at`。
- 启动引导：若 `wb_user` 为空，必须用 `WB_BOOTSTRAP_ADMIN_USER` / `WB_BOOTSTRAP_ADMIN_PWD` 环境变量建默认 admin（Developer）；未设则启动**拒绝**并提示。另提供 `oaip --create-admin` 子命令离线建账号。

### 3.7 数据存储（追加到 `canonical_schema()`）

```sql
CREATE TABLE IF NOT EXISTS wb_user (
  id           TEXT PRIMARY KEY,
  username     TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL,          -- argon2id encoded
  role         TEXT NOT NULL,           -- Developer | Operator | Executive
  display_name TEXT,
  disabled     INTEGER NOT NULL DEFAULT 0,
  created_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_user_name ON wb_user(username);

CREATE TABLE IF NOT EXISTS wb_session (
  token      TEXT PRIMARY KEY,
  user_id    TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  expires_at TEXT NOT NULL,
  FOREIGN KEY(user_id) REFERENCES wb_user(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_session_user ON wb_session(user_id);
```

`ensure_schema` 版本匹配分支追加上述 DDL（幂等 `IF NOT EXISTS`），旧库无需 ALTER（全为新表）。

### 3.8 API 变更

- 新增 §3.6 三个 auth 端点；`router()` 包一层 `auth_layer`。
- 现有 handler 签名加 `AuthUser`，在入口做 `can()` 判定（403）。
- trace/timeline 的 `view` 入参改为「建议视图」，由 `AuthUser` 钳制。
- 静态资源（`serve_static`）与登录端点保持公开；其余一律 401。

### 3.9 测试矩阵（建议 TC-auth-a~g）

| 用例 | 预期 |
| --- | --- |
| a 无 token 访问 `/run` | 401 |
| b 错误/过期 token | 401 |
| c Executive 调 `POST /run` | 403 |
| d Executive 请求 `?view=developer` | 服务端钳制为 Executive（响应视图=executive） |
| e 正确登录 | 200 + token + role |
| f logout 后复用旧 token | 401 |
| g 禁用用户后其 token | 401 |

## 4. 设计分叉（已定稿 · 2026-08-13 评审结论）

| # | 分叉 | 结论 | 理由 |
| --- | --- | --- | --- |
| D1 | 身份源 | 本地账号 + `IdentityProvider` trait（OIDC 留后续） | 单机产品化阶段，OIDC 直连拖慢主线；trait 已留插拔 |
| D2 | 令牌 | 不透明会话令牌 | 可即时撤销、无密钥管理；单机场景 JWT 无状态收益用不上 |
| D3 | Operator 写边界 | 可裁决审批、不可写 Evolution | 最小权限；Evolution 写属"系统学习策略"边界，留 Developer |
| D4 | TLS | Nginx/网关终止，应用内网 HTTP | 生产由前置网关管 TLS；**文档明令：对外暴露必须 TLS** |
| D5 | 口令哈希 | argon2id | OWASP 推荐，Rust argon2 crate 成熟 |

**评审补充建议（已吸收进实现）：**

- `?view=` 钳制用角色 rank（Executive<Operator<Developer 显式排序），**不用字符串比较**。
- `wb_session` **不依赖外键**（libsql 默认 `foreign_keys=OFF`）；用户禁用时由应用层 `delete_sessions_for_user` 级联清会话。
- 登录接口建议加简单限流/锁定（本地单用户可不做，handler 留注释），防引导账号爆破。

## 5. 修订日志

| 版本 | 作者 | 日期 | 说明 |
| --- | --- | --- | --- |
| V0.2 | lushaohui | 2026-08-13 | 评审定稿：D1–D5 结论落表；补充建议（视图 rank / 去 FK / 登录限流）吸收；地基实现 `src/auth.rs` 完成（Role/View 钳制/LocalIdentityProvider/SessionStore/能力矩阵 + 单测），服务端中间件与路由待接 | --- | --- |
| V0.1 | lushaohui | 2026-08-13 | 初稿：本地身份 + 会话令牌 + axum 中间件 + 三层角色能力矩阵 + 服务端视图钳制 + wb_user/wb_session DDL + TC 矩阵；列 5 项待确认分叉 |
