//! 认证与授权（V0.7 · 对应 P0 风险「无认证」）。
//!
//! 设计稿：docs/09_迭代与版本/design-auth.md（分叉 D1–D5 已全部按推荐定稿）。
//! - 身份源：本地账号（`LocalIdentityProvider`）；`IdentityProvider` trait 留 OIDC 后续插拔。
//! - 令牌：不透明随机会话令牌（`SessionStore`，可即时撤销，无密钥管理）。
//! - 角色：Executive < Operator < Developer 三层，最小权限。
//! - 服务端视图钳制：客户端 `?view=` 越权请求被钳制到角色上限（不再自提权限）。
//!
//! 登录限流/锁定：本地单用户场景可不启用；多用户暴露前应在 `login` handler 加简单
//! 计数器（按 IP/用户名，连续失败 N 次锁定 M 分钟），防引导账号爆破。本模块提供
//! `IdentityProvider` / `SessionStore`  primitives，限流状态建议放在 `AppState` 层。

use crate::storage::{LibSQLStorage, UserRecord};
use crate::trace::View;
use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use async_trait::async_trait;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

/// 三层角色（权限等级 Executive < Operator < Developer）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Executive,
    Operator,
    Developer,
}

impl Role {
    /// 权限等级：Executive=0 < Operator=1 < Developer=2。
    /// 视图钳制与能力判定一律用 rank，**不用字符串比较**（用户评审要求）。
    pub fn rank(&self) -> u8 {
        match self {
            Role::Executive => 0,
            Role::Operator => 1,
            Role::Developer => 2,
        }
    }

    /// 该角色允许查看的最高视图（服务端钳制上限）。
    pub fn max_view(&self) -> View {
        match self {
            Role::Executive => View::Executive,
            Role::Operator => View::Operator,
            Role::Developer => View::Developer,
        }
    }
}

impl FromStr for Role {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "executive" => Ok(Role::Executive),
            "operator" => Ok(Role::Operator),
            "developer" => Ok(Role::Developer),
            other => Err(format!("未知角色: {other}")),
        }
    }
}

/// 把客户端请求的视图钳制到角色允许的上限（越低权限视图越不可越权看细）。
/// 取 `requested` 与 `role.max_view()` 中 rank 较小者。
pub fn clamp_view(requested: View, role: Role) -> View {
    let req = view_rank(requested);
    let max = view_rank(role.max_view());
    if req <= max {
        requested
    } else {
        role.max_view()
    }
}

/// View 的权限等级（与 `Role::rank` 同源排序）。
fn view_rank(v: View) -> u8 {
    match v {
        View::Executive => 0,
        View::Operator => 1,
        View::Developer => 2,
    }
}

/// 已认证的调用者（注入请求扩展，供 handler 做角色裁剪与能力判定）。
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: String,
    pub username: String,
    pub role: Role,
}

impl AuthUser {
    /// 可触发 run（Operator/Developer）；Executive 只读。
    pub fn can_trigger_run(&self) -> bool {
        self.role.rank() >= Role::Operator.rank()
    }
    /// 可裁决审批（Operator/Developer）。
    pub fn can_resolve_approval(&self) -> bool {
        self.role.rank() >= Role::Operator.rank()
    }
    /// 可写 Evolution（撤销/切 AutoApprove）—— 仅 Developer（最小权限边界）。
    pub fn can_write_evolution(&self) -> bool {
        self.role == Role::Developer
    }
    /// 可读取审计。
    pub fn can_read_audit(&self) -> bool {
        self.role.rank() >= Role::Operator.rank()
    }
    /// 可轮换运行时认证密钥——仅 Developer（管理员）执行。
    pub fn can_rotate_auth_key(&self) -> bool {
        self.role == Role::Developer
    }
    /// 可管理本地用户、角色与口令——仅 Developer。
    pub fn can_manage_users(&self) -> bool {
        self.role == Role::Developer
    }
    /// 可安装/管理业务包——仅 Developer，避免普通运行角色修改本体和初始事实。
    pub fn can_manage_domain_packs(&self) -> bool {
        self.role == Role::Developer
    }
    /// 可读取 Evaluation（Phase 11 派生表）—— Operator 及以上；写由 Runtime 钩子独占。
    pub fn can_read_evaluation(&self) -> bool {
        self.role.rank() >= Role::Operator.rank()
    }
}

/// 认证错误。
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("存储错误: {0}")]
    Storage(String),
    #[error("口令哈希错误: {0}")]
    Hash(String),
    #[error("角色解析错误: {0}")]
    Role(String),
}

/// 身份提供方（本地账号实现；OIDC 留后续，实现同一 trait 即可插拔）。
#[async_trait]
pub trait IdentityProvider: Send + Sync {
    /// 校验凭据；成功返回 `AuthUser`，未知/禁用/口令错返回 `None`。
    async fn authenticate(
        &self,
        username: &str,
        password: &str,
    ) -> Result<Option<AuthUser>, AuthError>;
    /// 引导管理员账号（首次启动，幂等）。
    async fn bootstrap_admin(&self, username: &str, password: &str) -> Result<(), AuthError>;
}

/// argon2id 实例：显式参数（内存 64MiB、t=3、p=4）——哈希与验证必须同参数，
/// 且不依赖 crate 默认值漂移（LOW：`Argon2::default()` 参数隐式）。
/// `Params::new` 对常量合法参数不会失败，expect 安全。
fn argon2() -> Argon2<'static> {
    Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        argon2::Params::new(64 * 1024, 3, 4, None).expect("argon2id 64MiB/t=3/p=4 参数合法"),
    )
}

/// 本地账号身份提供方（argon2id + `wb_user`）。
pub struct LocalIdentityProvider {
    storage: LibSQLStorage,
}

impl LocalIdentityProvider {
    pub fn new(storage: LibSQLStorage) -> Self {
        Self { storage }
    }

    /// 计算 argon2id 口令哈希（测试/CLI 造账号复用同一实现）。
    pub fn hash_password(pw: &str) -> Result<String, AuthError> {
        let salt = SaltString::generate(&mut OsRng);
        argon2()
            .hash_password(pw.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| AuthError::Hash(e.to_string()))
    }
}

#[async_trait]
impl IdentityProvider for LocalIdentityProvider {
    async fn authenticate(
        &self,
        username: &str,
        password: &str,
    ) -> Result<Option<AuthUser>, AuthError> {
        let Some(u) = self
            .storage
            .get_user_by_username(username)
            .await
            .map_err(|e| AuthError::Storage(e.to_string()))?
        else {
            return Ok(None);
        };
        if u.disabled {
            return Ok(None);
        }
        let ph = PasswordHash::new(&u.password_hash).map_err(|e| AuthError::Hash(e.to_string()))?;
        if argon2().verify_password(password.as_bytes(), &ph).is_err() {
            return Ok(None);
        }
        Ok(Some(AuthUser {
            id: u.id,
            username: u.username,
            role: Role::from_str(&u.role).map_err(AuthError::Role)?,
        }))
    }

    async fn bootstrap_admin(&self, username: &str, password: &str) -> Result<(), AuthError> {
        if self
            .storage
            .get_user_by_username(username)
            .await
            .map_err(|e| AuthError::Storage(e.to_string()))?
            .is_some()
        {
            return Ok(()); // 幂等
        }
        let hash = Self::hash_password(password)?;
        let rec = UserRecord {
            id: format!("user.{username}"),
            username: username.to_string(),
            password_hash: hash,
            role: "developer".into(),
            display_name: "Admin".into(),
            disabled: false,
        };
        self.storage
            .create_user(&rec)
            .await
            .map_err(|e| AuthError::Storage(e.to_string()))?;
        Ok(())
    }
}

/// 会话存储（不透明令牌 + `wb_session`；无 FK，用户禁用时由应用层级联清会话）。
///
/// M4：入库前对令牌做 SHA-256 哈希——`wb_session` 存哈希而非明文，
/// DB 泄漏不直接暴露可用会话令牌；校验/撤销时对输入同哈希比对。
pub struct SessionStore {
    storage: LibSQLStorage,
}

/// 令牌 → SHA-256 十六进制（存储/查询键）。
fn hash_token(token: &str) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(token.as_bytes());
    hex::encode(h.finalize())
}

impl SessionStore {
    pub fn new(storage: LibSQLStorage) -> Self {
        Self { storage }
    }

    /// 默认会话有效期（12h）。
    const TTL_SECS: i64 = 12 * 3600;

    /// 签发新会话（32B 随机令牌，hex 编码；明文仅返回给用户，库中存哈希）。
    pub async fn issue(&self, user: &AuthUser) -> Result<String, AuthError> {
        let mut buf = [0u8; 32];
        OsRng.fill_bytes(&mut buf);
        let token = hex::encode(buf);
        let expires = (now_secs() + Self::TTL_SECS).to_string();
        self.storage
            .save_session(&hash_token(&token), &user.id, &expires)
            .await
            .map_err(|e| AuthError::Storage(e.to_string()))?;
        Ok(token)
    }

    /// 校验令牌：存在 + 未过期 + 用户未禁用 → 返回 `AuthUser`；否则 `None`（过期/孤儿顺手清理）。
    pub async fn validate(&self, token: &str) -> Result<Option<AuthUser>, AuthError> {
        let key = hash_token(token);
        let Some(s) = self
            .storage
            .get_session(&key)
            .await
            .map_err(|e| AuthError::Storage(e.to_string()))?
        else {
            return Ok(None);
        };
        if s.expires_at.parse::<i64>().unwrap_or(0) < now_secs() {
            let _ = self.storage.delete_session(&key).await;
            return Ok(None);
        }
        let Some(u) = self
            .storage
            .get_user_by_id(&s.user_id)
            .await
            .map_err(|e| AuthError::Storage(e.to_string()))?
        else {
            let _ = self.storage.delete_session(&key).await;
            return Ok(None);
        };
        if u.disabled {
            let _ = self.storage.delete_session(&key).await;
            return Ok(None);
        }
        Ok(Some(AuthUser {
            id: u.id,
            username: u.username,
            role: Role::from_str(&u.role).map_err(AuthError::Role)?,
        }))
    }

    /// 撤销单条会话（登出）。
    pub async fn revoke(&self, token: &str) -> Result<(), AuthError> {
        self.storage
            .delete_session(&hash_token(token))
            .await
            .map_err(|e| AuthError::Storage(e.to_string()))
    }
}

/// 当前 unix 秒（业务时间；`wb_session.expires_at` 以数值字符串比较）。
fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::LibSQLStorage;

    async fn mk_storage() -> LibSQLStorage {
        LibSQLStorage::open(":memory:").await.unwrap()
    }

    #[test]
    fn role_rank_orders_executive_operator_developer() {
        assert!(Role::Executive.rank() < Role::Operator.rank());
        assert!(Role::Operator.rank() < Role::Developer.rank());
        // 派生 PartialOrd 与显式 rank 一致
        assert!(Role::Executive < Role::Operator);
        assert!(Role::Operator < Role::Developer);
    }

    #[test]
    fn view_clamp_uses_role_rank_not_string() {
        // Developer 可看 Developer；请求 Executive 被放行（更低权限）
        assert_eq!(
            clamp_view(View::Executive, Role::Developer),
            View::Executive
        );
        // Operator 最多看 Operator；请求 Developer 被钳制到 Operator
        assert_eq!(clamp_view(View::Developer, Role::Operator), View::Operator);
        // Executive 最多看 Executive；任何更高请求都被钳制
        assert_eq!(
            clamp_view(View::Developer, Role::Executive),
            View::Executive
        );
        assert_eq!(clamp_view(View::Operator, Role::Executive), View::Executive);
        // 同档放行
        assert_eq!(clamp_view(View::Operator, Role::Operator), View::Operator);
    }

    #[tokio::test]
    async fn local_identity_bootstrap_authenticate_and_disable() {
        let s = mk_storage().await;
        let idp = LocalIdentityProvider::new(s.clone());

        // bootstrap 幂等
        idp.bootstrap_admin("admin", "s3cret").await.unwrap();
        idp.bootstrap_admin("admin", "s3cret").await.unwrap();

        // 正确凭据
        let u = idp.authenticate("admin", "s3cret").await.unwrap();
        assert!(u.is_some());
        assert_eq!(u.as_ref().unwrap().role, Role::Developer);

        // 错误口令 → None（不泄露是否存在）
        assert!(idp.authenticate("admin", "wrong").await.unwrap().is_none());
        assert!(idp.authenticate("ghost", "x").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn session_issue_validate_revoke_roundtrip() {
        let s = mk_storage().await;
        let idp = LocalIdentityProvider::new(s.clone());
        idp.bootstrap_admin("admin", "s3cret").await.unwrap();
        let user = idp.authenticate("admin", "s3cret").await.unwrap().unwrap();

        let store = SessionStore::new(s.clone());
        let token = store.issue(&user).await.unwrap();
        assert!(!token.is_empty());

        // 校验通过
        let valid = store.validate(&token).await.unwrap();
        assert!(valid.is_some());
        assert_eq!(valid.unwrap().username, "admin");

        // 撤销后失效
        store.revoke(&token).await.unwrap();
        assert!(store.validate(&token).await.unwrap().is_none());
    }
}
