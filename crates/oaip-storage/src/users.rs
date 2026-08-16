use super::*;

impl LibSQLStorage {
    /// 创建本地账号（用户名唯一约束由 wb_user.username UNIQUE 保证）。
    pub async fn create_user(&self, u: &UserRecord) -> Result<(), StorageError> {
        self.conn
            .execute(
                "INSERT INTO wb_user (id, username, password_hash, role, display_name, disabled) VALUES (?1,?2,?3,?4,?5,?6)",
                params![
                    u.id.clone(),
                    u.username.clone(),
                    u.password_hash.clone(),
                    u.role.clone(),
                    u.display_name.clone(),
                    u.disabled as i64
                ],
            )
            .await?;
        Ok(())
    }

    /// 按用户名取账号（登录校验）；不存在返回 None。
    pub async fn get_user_by_username(
        &self,
        username: &str,
    ) -> Result<Option<UserRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, username, password_hash, role, display_name, disabled FROM wb_user WHERE username = ?1",
                params![username],
            )
            .await?;
        match rows.next().await? {
            Some(r) => Ok(Some(user_from_row(&r)?)),
            None => Ok(None),
        }
    }

    /// 账号总数（启动引导判定：空库且未配 env 则拒绝启动）。
    pub async fn count_users(&self) -> Result<i64, StorageError> {
        let mut rows = self
            .conn
            .query("SELECT COUNT(*) FROM wb_user", params![])
            .await?;
        match rows.next().await? {
            Some(r) => Ok(r.get(0)?),
            None => Ok(0),
        }
    }

    /// 按 id 取账号（会话校验反查）；不存在返回 None。
    pub async fn get_user_by_id(&self, id: &str) -> Result<Option<UserRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, username, password_hash, role, display_name, disabled FROM wb_user WHERE id = ?1",
                params![id],
            )
            .await?;
        match rows.next().await? {
            Some(r) => Ok(Some(user_from_row(&r)?)),
            None => Ok(None),
        }
    }

    /// 列出全部账号。
    pub async fn list_users(&self) -> Result<Vec<UserRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, username, password_hash, role, display_name, disabled FROM wb_user",
                params![],
            )
            .await?;
        let mut users = Vec::new();
        while let Some(r) = rows.next().await? {
            users.push(user_from_row(&r)?);
        }
        Ok(users)
    }

    /// 返回对象-关系图（GraphRAG 基础）：节点来自对象，边来自关系。
    pub async fn get_object_graph(
        &self,
    ) -> Result<(Vec<ObjectRecord>, Vec<RelationRecord>), StorageError> {
        Ok((
            self.list_all_objects().await?,
            self.list_all_relations().await?,
        ))
    }

    /// 删除账号（幂等：不存在则 NotFound）。
    pub async fn delete_user(&self, username: &str) -> Result<(), StorageError> {
        let Some(u) = self.get_user_by_username(username).await? else {
            return Err(StorageError::NotFound(format!("user:{username}")));
        };
        self.conn
            .execute("DELETE FROM wb_user WHERE username = ?1", params![username])
            .await?;
        self.delete_sessions_for_user(&u.id).await?;
        Ok(())
    }

    /// 启停账号（禁用时级联清其全部会话，应用层替代外键 ON DELETE CASCADE）。
    pub async fn set_user_disabled(
        &self,
        user_id: &str,
        disabled: bool,
    ) -> Result<(), StorageError> {
        self.conn
            .execute(
                "UPDATE wb_user SET disabled = ?2 WHERE id = ?1",
                params![user_id, disabled as i64],
            )
            .await?;
        if disabled {
            self.delete_sessions_for_user(user_id).await?;
        }
        Ok(())
    }

    /// 签发会话行（令牌由调用方生成，见 `SessionStore`）。
    pub async fn save_session(
        &self,
        token: &str,
        user_id: &str,
        expires_at: &str,
    ) -> Result<(), StorageError> {
        self.conn
            .execute(
                "INSERT INTO wb_session (token, user_id, expires_at) VALUES (?1,?2,?3)",
                params![token, user_id, expires_at],
            )
            .await?;
        Ok(())
    }

    /// 取会话行（含 user_id，用于反查用户与过期判定）。
    pub async fn get_session(&self, token: &str) -> Result<Option<SessionRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT token, user_id, created_at, expires_at FROM wb_session WHERE token = ?1",
                params![token],
            )
            .await?;
        match rows.next().await? {
            Some(r) => Ok(Some(SessionRecord {
                token: r.get(0)?,
                user_id: r.get(1)?,
                created_at: r.get(2)?,
                expires_at: r.get(3)?,
            })),
            None => Ok(None),
        }
    }

    /// 撤销单条会话（登出 / 禁用级联）。
    pub async fn delete_session(&self, token: &str) -> Result<(), StorageError> {
        self.conn
            .execute("DELETE FROM wb_session WHERE token = ?1", params![token])
            .await?;
        Ok(())
    }

    /// 撤销某用户全部会话（禁用账号时级联）。
    pub async fn delete_sessions_for_user(&self, user_id: &str) -> Result<(), StorageError> {
        self.conn
            .execute(
                "DELETE FROM wb_session WHERE user_id = ?1",
                params![user_id],
            )
            .await?;
        Ok(())
    }
}

/// 从 wb_user 行构造 `UserRecord`。
fn user_from_row(r: &libsql::Row) -> Result<UserRecord, StorageError> {
    Ok(UserRecord {
        id: r.get(0)?,
        username: r.get(1)?,
        password_hash: r.get(2)?,
        role: r.get(3)?,
        display_name: r.get(4)?,
        disabled: r.get::<i64>(5)? != 0,
    })
}
