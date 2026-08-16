//! 用户命令（M5 拆分）：认证与账号管理。

use openaip_os::auth::{IdentityProvider, LocalIdentityProvider};
use openaip_os::storage::{LibSQLStorage, UserRecord};

pub async fn cmd_user_create_admin(
    db: &str,
    username: &str,
    password: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let storage = LibSQLStorage::open(db).await?;
    let idp = LocalIdentityProvider::new(storage);
    idp.bootstrap_admin(username, password).await?;
    println!("✅ 管理员账号已就绪：{username}（角色 Developer）");
    Ok(())
}

/// 通用创建账号（默认 Developer 角色）
pub async fn cmd_user_create(
    db: &str,
    username: &str,
    password: &str,
    role: &str,
    display_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let role_str = role.to_lowercase();
    // 角色白名单校验：非法角色会让该用户登录时永久失败（Role::from_str 报错），必须前置拦截。
    if !matches!(role_str.as_str(), "executive" | "operator" | "developer") {
        return Err(format!("未知角色: {role}（可选 executive / operator / developer）").into());
    }
    let storage = LibSQLStorage::open(db).await?;
    let rec = UserRecord {
        id: format!("user.{username}"),
        username: username.to_string(),
        password_hash: LocalIdentityProvider::hash_password(password)?,
        role: role_str,
        display_name: display_name.to_string(),
        disabled: false,
    };
    storage.create_user(&rec).await?;
    println!("✅ 用户账号已创建：{username}（角色 {role}）");
    Ok(())
}

/// 列出所有用户账号
pub async fn cmd_user_list(db: &str) -> Result<(), Box<dyn std::error::Error>> {
    let storage = LibSQLStorage::open(db).await?;
    let users = storage.list_users().await?;
    if users.is_empty() {
        println!("（无用户账号，先运行 `oaip user create-admin <用户名> --password <口令>`）");
        return Ok(());
    }
    for u in users {
        println!(
            "{} | {} | {} | disabled={}",
            u.username, u.role, u.display_name, u.disabled
        );
    }
    Ok(())
}

/// 删除账号（幂等）
pub async fn cmd_user_delete(db: &str, username: &str) -> Result<(), Box<dyn std::error::Error>> {
    let storage = LibSQLStorage::open(db).await?;
    storage.delete_user(username).await?;
    println!("✅ 用户账号已删除：{username}");
    Ok(())
}
