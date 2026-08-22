//! S1g：本体数据与动作效果的行为兼容契约。
//!
//! 兼容边界明确为：新增属性不阻断旧数据；已存在属性的类型变化必须显式失败；
//! 动作效果变化必须反映到 RuntimeDefinition/World，未知动作必须 fail-closed。

use openaip_os::abi::{DataValidationError, parse_odl, validate_object_data};
use openaip_os::compose::defs::{EffectDefinition, ir_to_runtime};
use openaip_os::kernel::effect::{Effect, Reversibility};
use openaip_os::kernel::{Kernel, Scope};
use openaip_os::world_exec::{EffectExecutor, RelationEffectExecutor};
use serde_json::json;

const OLD_ODL: &str = r#"
object Customer:
  properties:
    name: string
  actions:
    retain:
      creates:
        - type: RetentionTask
          data:
            title: "客户挽留计划"
          link: needs_retention
"#;

const NEW_ODL: &str = r#"
object Customer:
  properties:
    name: string
    score: int
  actions:
    retain:
      updates:
        - field: status
          value: "retention_pending"
"#;

fn first_object(odl: &str) -> openaip_os::abi::ObjectType {
    parse_odl(odl)
        .expect("ODL 解析")
        .object_types
        .into_iter()
        .next()
        .expect("object 定义")
}

fn first_runtime(odl: &str) -> openaip_os::compose::defs::RuntimeDefinition {
    ir_to_runtime(&parse_odl(odl).expect("ODL 解析"))
        .into_iter()
        .next()
        .expect("runtime definition")
}

fn retain_effects(def: &openaip_os::compose::defs::RuntimeDefinition) -> Vec<EffectDefinition> {
    def.capabilities
        .iter()
        .find(|cap| cap.name == "Customer.retain")
        .expect("Customer.retain 能力")
        .effects
        .clone()
}

#[test]
fn additive_property_change_accepts_old_json_without_rewrite() {
    let old_definition = first_object(OLD_ODL);
    let new_definition = first_object(NEW_ODL);
    let old_row = json!({"name": "Ada"});

    assert!(validate_object_data(&old_definition, &old_row).is_ok());
    assert!(validate_object_data(&new_definition, &old_row).is_ok());
    assert_eq!(old_row, json!({"name": "Ada"}));
    assert!(new_definition.properties.iter().any(|p| p.name == "score"));
}

#[test]
fn changing_existing_property_type_rejects_old_json() {
    let definition = first_object(
        r#"
object Customer:
  properties:
    name: int
"#,
    );
    let old_row = json!({"name": "Ada"});

    assert!(matches!(
        validate_object_data(&definition, &old_row),
        Err(DataValidationError::InvalidPropertyType { field, .. }) if field == "name"
    ));
}

#[test]
fn action_effect_evolution_is_explicit_and_unknown_actions_fail_closed() {
    let old_runtime = first_runtime(OLD_ODL);
    let new_runtime = first_runtime(NEW_ODL);

    assert_eq!(
        retain_effects(&old_runtime),
        vec![EffectDefinition::Create {
            object_type: "RetentionTask".into(),
            link_edge: Some("needs_retention".into()),
        }]
    );
    assert_eq!(
        retain_effects(&new_runtime),
        vec![EffectDefinition::Update {
            field: "status".into(),
            value: openaip_os::compose::defs::EffectValue::Literal(json!("retention_pending")),
        }]
    );
    assert_ne!(retain_effects(&old_runtime), retain_effects(&new_runtime));

    let mut kernel = Kernel::new();
    let context = kernel
        .create_context(None, Scope::Tenant, "compatibility")
        .expect("context");
    let fiber = kernel
        .spawn(context, None, "compatibility-task", json!({}))
        .expect("fiber");

    let mut evolved = Effect::new(
        "RetainCustomer",
        json!({"target": "cust-01"}),
        Reversibility::Compensatable,
        None,
        kernel.now(),
    );
    evolved.fiber = fiber;
    let evolved_result = {
        let mut executor = RelationEffectExecutor::new(&mut kernel, vec![new_runtime]);
        executor.execute(&evolved).expect("新效果执行成功")
    };
    assert_eq!(
        evolved_result.relations,
        vec![(
            "cust-01".to_string(),
            "status".to_string(),
            "retention_pending".to_string()
        )]
    );

    let before_unknown = kernel.relations().filter(|r| r.context == context).count();
    let mut unknown = Effect::new(
        "ExplodePlanet",
        json!({"target": "earth"}),
        Reversibility::Compensatable,
        None,
        kernel.now(),
    );
    unknown.fiber = fiber;
    {
        let mut executor = RelationEffectExecutor::new(&mut kernel, vec![first_runtime(NEW_ODL)]);
        assert!(executor.execute(&unknown).is_err());
    }
    let after_unknown = kernel.relations().filter(|r| r.context == context).count();
    assert_eq!(
        after_unknown, before_unknown,
        "未知 action 不得产生世界变化"
    );
}
