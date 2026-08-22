//! 公司业务包命令：validate / install。

use openaip_os::domain_pack::DomainPack;
use openaip_os::storage::LibSQLStorage;

pub async fn cmd_validate(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let pack = DomainPack::load(path)?;
    println!(
        "✅ 业务包有效：{} v{}（{} 个概念类型，{} 个初始对象，{} 条关系）",
        pack.manifest.name,
        pack.manifest.version,
        pack.ontology.object_types.len(),
        pack.bootstrap.objects.len(),
        pack.bootstrap.relations.len()
    );
    Ok(())
}

pub async fn cmd_install(db: &str, path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let pack = DomainPack::load(path)?;
    let storage = LibSQLStorage::open(db).await?;
    let report = pack.install(&storage).await?;
    println!(
        "✅ 业务包已安装：{} v{} → project={}；对象 +{}/跳过{}，关系 +{}/跳过{}",
        pack.manifest.name,
        pack.manifest.version,
        report.project_id,
        report.objects_created,
        report.objects_skipped,
        report.relations_created,
        report.relations_skipped
    );
    Ok(())
}
