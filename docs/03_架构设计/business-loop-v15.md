# Business Loop v15

## 目标

业务发现、决策和执行不再只校验单个对象；必须先通过项目级整状态推理。

## 运行时契约

1. 项目世界按 `project_id` 投影为 DL ABox，不混入其他项目事实。
2. ODL `one_of` 使用 `<Class>.<Property>` 作用域投影，同名属性不再跨类交叉收窄。
3. 发现、显式决策和执行前都执行：
   - closed-world 数据一致性检查；
   - DL Reasoner 整状态一致性检查。
4. 任一检查失败时 fail-closed，不创建 Decision，不改写企业状态。

## 验证

E2E 注入绕过单对象写入的非法 RetentionTask 状态，证明 Selected 行动在执行前被整状态门禁阻止。
