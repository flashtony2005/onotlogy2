//! 本体基础契约测试（对应 docs/07_测试文档/test-case.md TC-001~008）。
//! 一条用例定义一个能力的边界（Red→Green）。

use openaip_os::abi::codegen;
use openaip_os::abi::ir::{
    ActorKind, DataLiteral, DataPropertyHierarchy, DataPropertyRestriction, DataRestrictionKind,
    DisjointUnionGroup, ObjectOneOfGroup, ObjectPropertyRestriction, ObjectRestrictionKind,
    OntologyIR, PropertyDef, RelationHierarchy, ScalarType,
};
use openaip_os::abi::odl::parse_odl;
use openaip_os::storage::{LibSQLStorage, ObjectEngine, ObjectRecord, StorageError};
use openaip_os::trace::{TraceEvent, TraceKind};
use serde_json::json;

// ── TC-001 / TC-001-b：ODL Parser（REQ-001） ──────────────────────────────
#[test]
fn tc001_parse_valid() {
    let odl = "object Customer:\n  properties:\n    name: string\n    level: string\n";
    let ir = parse_odl(odl).unwrap();
    let ot = ir.find("Customer").expect("应包含 Customer");
    assert_eq!(ot.properties.len(), 2);
    assert_eq!(
        ot.properties[0],
        PropertyDef {
            name: "name".into(),
            ty: ScalarType::String,
            constraints: Vec::new(),
        }
    );
    assert_eq!(ot.properties[1].ty, ScalarType::String);
}

#[test]
fn tc001b_parse_invalid() {
    // 未知类型 → 错误码 1002，不 panic
    let e = parse_odl("object Customer:\n  properties:\n    name: unknown\n").unwrap_err();
    assert_eq!(e.code, 1002);

    // 缺 property → 错误码 1001
    let e2 = parse_odl("object Empty:\n  properties:\n").unwrap_err();
    assert_eq!(e2.code, 1001);

    // 属性游离于 properties 块外 → 1005
    let e3 = parse_odl("object X:\n  name: string\n").unwrap_err();
    assert_eq!(e3.code, 1005);
}

#[test]
fn action_planning_requires_complete_metadata() {
    let prefix = [
        "object Customer:",
        "  properties:",
        "    name: string",
        "  actions:",
        "    ContactCustomer:",
        "      planning:",
    ];

    let empty = prefix.join("\n");
    assert_eq!(parse_odl(&empty).unwrap_err().code, 1011);

    let partial = [prefix.as_slice(), ["        cost: 100"].as_slice()]
        .concat()
        .join("\n");
    assert_eq!(parse_odl(&partial).unwrap_err().code, 1011);

    let complete = [
        prefix.as_slice(),
        [
            "        cost: 100",
            "        prior_confidence: 0.5",
            "        outcome_timing: immediate",
            "        resource: hours=2",
        ]
        .as_slice(),
    ]
    .concat()
    .join("\n");
    let ir = parse_odl(&complete).unwrap();
    let planning = ir.object_types[0].actions[0]
        .planning
        .as_ref()
        .expect("完整声明必须生成 planning 元数据");
    assert_eq!(planning.cost, 100.0);
    assert_eq!(planning.prior_confidence, 0.5);
    assert_eq!(planning.resources.get("hours"), Some(&2.0));

    let duplicate = [
        prefix.as_slice(),
        [
            "        cost: 100",
            "        prior_confidence: 0.5",
            "        outcome_timing: immediate",
            "        resource: hours=1",
            "        resource: hours=2",
        ]
        .as_slice(),
    ]
    .concat()
    .join("\n");
    let error = parse_odl(&duplicate).expect_err("重复资源必须解析失败");
    assert!(error.to_string().contains("资源 hours 重复声明"));
}

#[test]
fn opportunity_rule_requires_complete_and_typed_metadata() {
    let base = [
        "relation targets:",
        "  source: Opportunity",
        "  target: Customer",
        "object Customer:",
        "  properties:",
        "    name: string",
        "    level: string",
        "    days_since_last_contact: int",
        "  opportunities:",
        "    Reactivation:",
        "      estimated_value: 180000",
        "      goal_metric: monthly_reactivations",
        "      evidence: \"客户已 {days_since_last_contact} 天未联系\"",
        "      subject_type: Customer",
        "      urgency_field: days_since_last_contact",
        "      urgency_horizon: 90",
        "      cost_budget: 25000",
        "      metric_budget: 80000",
        "      resource_budget: sales=3",
        "      link_edge: targets",
        "      when:",
        "        - field: self.level",
        "          op: eq",
        "          value: \"VIP\"",
        "        - field: self.days_since_last_contact",
        "          op: gt",
        "          value: 45",
        "object Opportunity:",
        "  properties:",
        "    subject: string",
        "    opportunity_type: string",
        "    status: string",
        "    estimated_value: decimal",
        "    evidence: string",
    ];

    let incomplete = [
        "object Customer:",
        "  properties:",
        "    name: string",
        "  opportunities:",
        "    Reactivation:",
        "      estimated_value: 180000",
    ]
    .join("\n");
    assert_eq!(parse_odl(&incomplete).unwrap_err().code, 1011);

    let when_index = base
        .iter()
        .position(|line| line.starts_with("      when:"))
        .expect("base 必须包含 when 块");
    let no_conditions = [base[..when_index].join("\n"), "      when:".to_owned()].join("\n");
    assert_eq!(parse_odl(&no_conditions).unwrap_err().code, 1011);

    let unknown_field = base
        .iter()
        .map(|line| line.replace("self.days_since_last_contact", "self.unknown"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(parse_odl(&unknown_field).unwrap_err().code, 1011);

    let ir = parse_odl(&base.join("\n")).expect("完整规则应可解析");
    let rule = &ir
        .find("Customer")
        .expect("Customer 应存在")
        .opportunity_rules[0];
    assert_eq!(rule.name, "Reactivation");
    assert_eq!(rule.estimated_value, 180_000.0);
    assert_eq!(rule.evidence, "客户已 {days_since_last_contact} 天未联系");
    assert_eq!(rule.subject_type, "Customer");
    assert_eq!(rule.urgency_field, "days_since_last_contact");
    assert_eq!(rule.urgency_horizon, 90.0);
    assert_eq!(rule.cost_budget, Some(25_000.0));
    assert_eq!(rule.metric_budget, Some(80_000.0));
    assert_eq!(rule.resource_budgets.get("sales"), Some(&3.0));
    assert_eq!(rule.link_edge, "targets");

    let negative = base
        .iter()
        .map(|line| line.replace("cost_budget: 25000", "cost_budget: -1"))
        .collect::<Vec<_>>()
        .join("\n");
    let error = parse_odl(&negative).expect_err("负数成本预算必须解析失败");
    assert!(error.to_string().contains("cost_budget 不能为负或无限"));

    let negative_metric = base
        .iter()
        .map(|line| line.replace("metric_budget: 80000", "metric_budget: -1"))
        .collect::<Vec<_>>()
        .join("\n");
    let error = parse_odl(&negative_metric).expect_err("负数指标预算必须解析失败");
    assert!(error.to_string().contains("metric_budget 不能为负或无限"));

    let negative_resource = base
        .iter()
        .map(|line| line.replace("resource_budget: sales=3", "resource_budget: sales=-1"))
        .collect::<Vec<_>>()
        .join("\n");
    let error = parse_odl(&negative_resource).expect_err("负数资源预算必须解析失败");
    assert!(error.to_string().contains("资源量不能为负或无限"));
    assert_eq!(rule.when.len(), 2);
}

#[test]
fn goal_metric_is_declared_on_rules_and_actions() {
    let odl = [
        "relation targets:",
        "  source: Opportunity",
        "  target: Customer",
        "object Customer:",
        "  properties:",
        "    name: string",
        "    days_since_last_contact: int",
        "  opportunities:",
        "    Reactivation:",
        "      estimated_value: 180000",
        "      goal_metric: monthly_reactivations",
        "      evidence: \"客户已 {name} 天未联系\"",
        "      subject_type: Customer",
        "      urgency_field: days_since_last_contact",
        "      urgency_horizon: 90",
        "      link_edge: targets",
        "      when:",
        "        - field: self.name",
        "          op: eq",
        "          value: \"VIP\"",
        "  actions:",
        "    ContactCustomer:",
        "      planning:",
        "        cost: 100",
        "        prior_confidence: 0.5",
        "        outcome_timing: immediate",
        "        goal_metric: monthly_reactivations",
        "object Opportunity:",
        "  properties:",
        "    subject: string",
        "    opportunity_type: string",
        "    status: string",
        "    estimated_value: decimal",
        "    evidence: string",
    ]
    .join("\n");

    let ir = parse_odl(&odl).expect("声明 goal_metric 的 ODL 应可解析");
    assert_eq!(
        ir.object_types[0].opportunity_rules[0].goal_metric,
        Some("monthly_reactivations".into())
    );
    assert_eq!(
        ir.object_types[0].actions[0]
            .planning
            .as_ref()
            .expect("planning 应存在")
            .goal_metric,
        Some("monthly_reactivations".into())
    );
}

#[test]
fn goal_metric_rejects_blank_declarations() {
    for line in [
        "object Customer:\n  properties:\n    name: string\n  actions:\n    ContactCustomer:\n      planning:\n        cost: 100\n        prior_confidence: 0.5\n        outcome_timing: immediate\n        goal_metric: ",
        "object Customer:\n  properties:\n    name: string\n  opportunities:\n    Reactivation:\n      estimated_value: 1\n      goal_metric: \n      evidence: \"证据\"\n      when:\n        - field: self.name\n          op: eq\n          value: \"VIP\"",
    ] {
        let error = parse_odl(line).expect_err("空 goal_metric 必须解析失败");
        assert_eq!(error.code, 1011);
        assert!(error.to_string().contains("goal_metric 不能为空"));
    }
}

#[test]
fn tc001c_parse_typed_relations() {
    let odl = [
        "object Customer:",
        "  properties:",
        "    name: string",
        "object Order:",
        "  properties:",
        "    total: int",
        "relation placed:",
        "  source: Customer",
        "  target: Order",
        "  functional: true",
        "  transitive: true",
        "  inverse_of: received_by",
        "relation received_by:",
        "  source: Order",
        "  target: Customer",
        "  asymmetric: true",
        "  irreflexive: true",
    ]
    .join("\n");
    let ir = parse_odl(&odl).unwrap();
    assert_eq!(2, ir.relations.len());
    assert!(ir.relations[0].functional);
    assert!(ir.relations[0].transitive);
    assert_eq!(Some("received_by"), ir.relations[0].inverse_of.as_deref());
    assert!(ir.relations[1].asymmetric);
    assert!(ir.relations[1].irreflexive);
    assert!(codegen::roundtrip_ok(&ir).unwrap());

    let non_reciprocal = [
        "object Customer:",
        "  properties:",
        "    name: string",
        "object Order:",
        "  properties:",
        "    total: int",
        "relation placed:",
        "  source: Customer",
        "  target: Order",
        "  inverse_of: received_by",
        "relation received_by:",
        "  source: Order",
        "  target: Customer",
        "  inverse_of: rejected_by",
        "relation rejected_by:",
        "  source: Customer",
        "  target: Order",
    ]
    .join("\n");
    let error = parse_odl(&non_reciprocal).unwrap_err();
    assert_eq!(1010, error.code);
    assert_eq!(7, error.line);

    let misdirected = [
        "object Customer:",
        "  properties:",
        "    name: string",
        "object Order:",
        "  properties:",
        "    total: int",
        "relation placed:",
        "  source: Customer",
        "  target: Order",
        "  inverse_of: received_by",
        "relation received_by:",
        "  source: Customer",
        "  target: Order",
    ]
    .join("\n");
    let error = parse_odl(&misdirected).unwrap_err();
    assert_eq!(1010, error.code);
    assert_eq!(7, error.line);

    let unknown = parse_odl(
        "object Customer:\n  properties:\n    name: string\nrelation placed:\n  source: Customer\n  target: Order\n",
    )
    .unwrap_err();
    assert_eq!(unknown.code, 1009);

    let duplicate = parse_odl(
        "object Customer:\n  properties:\n    name: string\nrelation placed:\n  source: Customer\n  target: Customer\nrelation placed:\n  source: Customer\n  target: Customer\n",
    )
    .unwrap_err();
    assert_eq!(duplicate.code, 1008);
}

#[test]
fn tc001g_parse_relation_property_chains() {
    let odl = [
        "object Person:",
        "  properties:",
        "    name: string",
        "relation has_parent:",
        "  source: Person",
        "  target: Person",
        "relation has_grandparent:",
        "  source: Person",
        "  target: Person",
        "  composed_of: has_parent has_parent",
    ]
    .join("\n");
    let ir = parse_odl(&odl).unwrap();
    assert_eq!(
        vec!["has_parent", "has_parent"],
        ir.relations[1].composed_of
    );
    assert!(codegen::roundtrip_ok(&ir).unwrap());

    let unknown = [
        "object Person:",
        "  properties:",
        "    name: string",
        "relation has_grandparent:",
        "  source: Person",
        "  target: Person",
        "  composed_of: has_parent has_parent",
    ]
    .join("\n");
    let error = parse_odl(&unknown).unwrap_err();
    assert_eq!(1011, error.code);
    assert_eq!(4, error.line);

    let singleton = [
        "object Person:",
        "  properties:",
        "    name: string",
        "relation has_parent:",
        "  source: Person",
        "  target: Person",
        "relation has_grandparent:",
        "  source: Person",
        "  target: Person",
        "  composed_of: has_parent",
    ]
    .join("\n");
    assert_eq!(1011, parse_odl(&singleton).unwrap_err().code);
}

#[test]
fn tc001h_parse_relation_hierarchy_and_disjointness() {
    let odl = [
        "object Person:",
        "  properties:",
        "    name: string",
        "relation biological_parent:",
        "  source: Person",
        "  target: Person",
        "relation parent:",
        "  source: Person",
        "  target: Person",
        "relation legal_parent:",
        "  source: Person",
        "  target: Person",
        "sub_relation biological_parent, parent",
        "disjoint_relation biological_parent, legal_parent",
    ]
    .join("\n");
    let ir = parse_odl(&odl).unwrap();
    assert_eq!(
        vec![RelationHierarchy {
            sub_relation: "biological_parent".into(),
            super_relation: "parent".into(),
        }],
        ir.relation_hierarchy
    );
    assert_eq!(
        vec![vec!["biological_parent", "legal_parent"]],
        ir.disjoint_relation_groups
    );
    assert!(codegen::roundtrip_ok(&ir).unwrap());

    let unknown = [
        "object Person:",
        "  properties:",
        "    name: string",
        "relation parent:",
        "  source: Person",
        "  target: Person",
        "sub_relation biological_parent, parent",
    ]
    .join("\n");
    let error = parse_odl(&unknown).unwrap_err();
    assert_eq!(1011, error.code);
    assert_eq!(7, error.line);

    let singleton = [
        "object Person:",
        "  properties:",
        "    name: string",
        "relation parent:",
        "  source: Person",
        "  target: Person",
        "disjoint_relation parent",
    ]
    .join("\n");
    assert_eq!(1011, parse_odl(&singleton).unwrap_err().code);
}

#[test]
fn tc001j_parse_equivalent_relation_group() {
    let odl = [
        "object Person:",
        "  properties:",
        "    name: string",
        "relation primary_contact:",
        "  source: Person",
        "  target: Person",
        "relation contact:",
        "  source: Person",
        "  target: Person",
        "equivalent_relation primary_contact, contact",
    ]
    .join("\n");
    let ir = parse_odl(&odl).unwrap();
    assert_eq!(
        vec![vec!["primary_contact", "contact"]],
        ir.equivalent_relation_groups
    );
    assert!(codegen::roundtrip_ok(&ir).unwrap());

    let unknown = [
        "object Person:",
        "  properties:",
        "    name: string",
        "relation contact:",
        "  source: Person",
        "  target: Person",
        "equivalent_relation primary_contact, contact",
    ]
    .join("\n");
    let error = parse_odl(&unknown).unwrap_err();
    assert_eq!(1011, error.code);
    assert_eq!(7, error.line);

    let singleton = [
        "object Person:",
        "  properties:",
        "    name: string",
        "relation contact:",
        "  source: Person",
        "  target: Person",
        "equivalent_relation contact",
    ]
    .join("\n");
    let error = parse_odl(&singleton).unwrap_err();
    assert_eq!(1011, error.code);
    assert_eq!(7, error.line);
}

#[test]
fn tc001i_parse_data_property_hierarchy_disjointness_and_equivalence() {
    let odl = [
        "object Person:",
        "  properties:",
        "    email: string",
        "    work_email: string",
        "    mobile: string",
        "    landline: string",
        "    primary_contact: string",
        "    contact: string",
        "sub_data_property work_email, email",
        "disjoint_data_property mobile, landline",
        "equivalent_data_property primary_contact, contact",
    ]
    .join("\n");
    let ir = parse_odl(&odl).unwrap();
    assert_eq!(
        vec![DataPropertyHierarchy {
            sub_property: "work_email".into(),
            super_property: "email".into(),
        }],
        ir.data_property_hierarchy
    );
    assert_eq!(
        vec![vec!["mobile", "landline"]],
        ir.disjoint_data_property_groups
    );
    assert_eq!(
        vec![vec!["primary_contact", "contact"]],
        ir.equivalent_data_property_groups
    );
    assert!(codegen::roundtrip_ok(&ir).unwrap());

    let unknown = [
        "object Person:",
        "  properties:",
        "    email: string",
        "sub_data_property work_email, email",
    ]
    .join("\n");
    let error = parse_odl(&unknown).unwrap_err();
    assert_eq!(1011, error.code);
    assert_eq!(4, error.line);

    let singleton = [
        "object Person:",
        "  properties:",
        "    email: string",
        "disjoint_data_property email",
    ]
    .join("\n");
    assert_eq!(1011, parse_odl(&singleton).unwrap_err().code);

    let equivalent_singleton = [
        "object Person:",
        "  properties:",
        "    email: string",
        "equivalent_data_property email",
    ]
    .join("\n");
    let error = parse_odl(&equivalent_singleton).unwrap_err();
    assert_eq!(1011, error.code);
    assert_eq!(4, error.line);
}

#[test]
fn tc001f_parse_relation_cardinalities() {
    let odl = [
        "object Customer:",
        "  properties:",
        "    name: string",
        "object Order:",
        "  properties:",
        "    total: int",
        "relation placed:",
        "  source: Customer",
        "  target: Order",
        "  min_cardinality: 1",
        "  max_cardinality: 2",
    ]
    .join("\n");
    let ir = parse_odl(&odl).unwrap();
    assert_eq!(Some(1), ir.relations[0].min_cardinality);
    assert_eq!(Some(2), ir.relations[0].max_cardinality);
    assert!(codegen::roundtrip_ok(&ir).unwrap());

    let invalid = [
        "object Customer:",
        "  properties:",
        "    name: string",
        "relation placed:",
        "  source: Customer",
        "  target: Customer",
        "  min_cardinality: 3",
        "  max_cardinality: 2",
    ]
    .join("\n");
    let error = parse_odl(&invalid).unwrap_err();
    assert_eq!(1011, error.code);
    assert_eq!(8, error.line);

    let non_numeric = [
        "object Customer:",
        "  properties:",
        "    name: string",
        "relation placed:",
        "  source: Customer",
        "  target: Customer",
        "  min_cardinality: many",
    ]
    .join("\n");
    assert_eq!(1011, parse_odl(&non_numeric).unwrap_err().code);
}

#[test]
fn tc001d_parse_object_extends() {
    let odl = [
        "object Animal:",
        "  properties:",
        "    name: string",
        "object Pet extends Animal:",
        "  properties:",
        "    owner_note: string",
        "object Dog extends Animal, Pet:",
        "  properties:",
        "    breed: string",
    ]
    .join("\n");
    let ir = parse_odl(&odl).unwrap();
    assert_eq!(Vec::<String>::new(), ir.find("Animal").unwrap().extends);
    assert_eq!(vec!["Animal"], ir.find("Pet").unwrap().extends);
    assert_eq!(vec!["Animal", "Pet"], ir.find("Dog").unwrap().extends);
    assert!(codegen::roundtrip_ok(&ir).unwrap());

    let unknown =
        parse_odl("object Dog extends Animal:\n  properties:\n    name: string\n").unwrap_err();
    assert_eq!(1012, unknown.code);
    assert_eq!(1, unknown.line);

    let cyclic = [
        "object Animal extends Dog:",
        "  properties:",
        "    name: string",
        "object Dog extends Animal:",
        "  properties:",
        "    breed: string",
    ]
    .join("\n");
    let cycle = parse_odl(&cyclic).unwrap_err();
    assert_eq!(1012, cycle.code);
    assert!(cycle.message.contains("继承关系形成环"));
}

#[test]
fn tc001e_parse_disjoint_and_equivalent_groups() {
    let odl = [
        "object Animal:",
        "  properties:",
        "    name: string",
        "object Dog extends Animal:",
        "  properties:",
        "    breed: string",
        "object Cat extends Animal:",
        "  properties:",
        "    indoor: bool",
        "object Pet:",
        "  properties:",
        "    nickname: string",
        "object Companion:",
        "  properties:",
        "    owner_note: string",
        "disjoint Dog, Cat",
        "equivalent Pet, Companion",
    ]
    .join("\n");
    let ir = parse_odl(&odl).unwrap();
    assert_eq!(vec![vec!["Dog", "Cat"]], ir.disjoint_groups);
    assert_eq!(vec![vec!["Pet", "Companion"]], ir.equivalent_groups);
    assert!(codegen::roundtrip_ok(&ir).unwrap());

    let unknown =
        parse_odl("object Dog:\n  properties:\n    name: string\ndisjoint Dog, Cat\n").unwrap_err();
    assert_eq!(1013, unknown.code);
    assert_eq!(4, unknown.line);

    let singleton =
        parse_odl("object Dog:\n  properties:\n    name: string\ndisjoint Dog\n").unwrap_err();
    assert_eq!(1013, singleton.code);

    let duplicate = [
        "object Dog:",
        "  properties:",
        "    name: string",
        "object Cat:",
        "  properties:",
        "    name: string",
        "disjoint Dog, Cat",
        "disjoint Cat, Dog",
    ]
    .join("\n");
    let duplicate = parse_odl(&duplicate).unwrap_err();
    assert_eq!(1013, duplicate.code);
    assert!(duplicate.message.contains("duplicate"));
}

#[test]
fn tc001f_parse_disjoint_union() {
    let odl = [
        "object Vehicle:",
        "  properties:",
        "    label: string",
        "object Car:",
        "  properties:",
        "    wheels: int",
        "object Boat:",
        "  properties:",
        "    draft: decimal",
        "disjoint_union Vehicle = Car, Boat",
    ]
    .join("\n");
    let ir = parse_odl(&odl).unwrap();
    assert_eq!(
        vec![DisjointUnionGroup {
            class: "Vehicle".to_owned(),
            members: vec!["Car".to_owned(), "Boat".to_owned()],
        }],
        ir.disjoint_unions
    );
    assert!(codegen::roundtrip_ok(&ir).unwrap());

    let unknown = [
        "object Vehicle:",
        "  properties:",
        "    label: string",
        "object Car:",
        "  properties:",
        "    wheels: int",
        "disjoint_union Vehicle = Car, Boat",
    ]
    .join("\n");
    let error = parse_odl(&unknown).unwrap_err();
    assert_eq!(1014, error.code);
    assert_eq!(7, error.line);
    assert!(error.message.contains("undeclared"));

    let singleton = parse_odl(
        "object Vehicle:\n  properties:\n    label: string\nobject Car:\n  properties:\n    wheels: int\ndisjoint_union Vehicle = Car\n",
    )
    .unwrap_err();
    assert_eq!(1014, singleton.code);

    let self_member = parse_odl(
        "object Vehicle:\n  properties:\n    label: string\nobject Car:\n  properties:\n    wheels: int\ndisjoint_union Vehicle = Vehicle, Car\n",
    )
    .unwrap_err();
    assert_eq!(1014, self_member.code);
}

#[test]
fn tc001k_parse_object_property_restrictions() {
    let odl = [
        "object Person:",
        "  properties:",
        "    name: string",
        "object Parent extends Person:",
        "  properties:",
        "    note: string",
        "  restrictions:",
        "    child: some Person",
        "    contact: only Person",
        "    guardian: value alice",
        "    child: min 2 Person",
        "    contact: max 1 Person",
        "relation child:",
        "  source: Person",
        "  target: Person",
        "relation contact:",
        "  source: Person",
        "  target: Person",
        "relation guardian:",
        "  source: Person",
        "  target: Person",
    ]
    .join("\n");
    let ir = parse_odl(&odl).unwrap();
    let expected = vec![
        ObjectPropertyRestriction {
            relation: "child".to_owned(),
            kind: ObjectRestrictionKind::Some {
                class: "Person".to_owned(),
            },
        },
        ObjectPropertyRestriction {
            relation: "contact".to_owned(),
            kind: ObjectRestrictionKind::Only {
                class: "Person".to_owned(),
            },
        },
        ObjectPropertyRestriction {
            relation: "guardian".to_owned(),
            kind: ObjectRestrictionKind::Value {
                individual: "alice".to_owned(),
            },
        },
        ObjectPropertyRestriction {
            relation: "child".to_owned(),
            kind: ObjectRestrictionKind::Min {
                cardinality: 2,
                class: "Person".to_owned(),
            },
        },
        ObjectPropertyRestriction {
            relation: "contact".to_owned(),
            kind: ObjectRestrictionKind::Max {
                cardinality: 1,
                class: "Person".to_owned(),
            },
        },
    ];
    assert_eq!(expected, ir.find("Parent").unwrap().restrictions);
    assert!(codegen::roundtrip_ok(&ir).unwrap());

    let unknown_relation = parse_odl(
        "object Person:\n  properties:\n    name: string\nobject Parent:\n  properties:\n    note: string\n  restrictions:\n    child: some Person\n",
    )
    .unwrap_err();
    assert_eq!(1015, unknown_relation.code);
    assert_eq!(8, unknown_relation.line);
    assert!(
        unknown_relation
            .message
            .contains("undeclared object property")
    );

    let unknown_class = parse_odl(
        "object Person:\n  properties:\n    name: string\nobject Parent:\n  properties:\n    note: string\n  restrictions:\n    child: only Robot\nrelation child:\n  source: Person\n  target: Person\n",
    )
    .unwrap_err();
    assert_eq!(1015, unknown_class.code);
    assert_eq!(8, unknown_class.line);
    assert!(unknown_class.message.contains("undeclared class"));

    let invalid_constructor = parse_odl(
        "object Person:\n  properties:\n    name: string\nobject Parent:\n  properties:\n    note: string\n  restrictions:\n    child: maybe Person\n",
    )
    .unwrap_err();
    assert_eq!(1015, invalid_constructor.code);

    let invalid_cardinality = parse_odl(
        "object Person:\n  properties:\n    name: string\nobject Parent:\n  properties:\n    note: string\n  restrictions:\n    child: min 0 Person\nrelation child:\n  source: Person\n  target: Person\n",
    )
    .unwrap_err();
    assert_eq!(1015, invalid_cardinality.code);
    assert!(
        invalid_cardinality
            .message
            .contains("cardinality restriction must be at least 1")
    );
}

#[test]
fn tc001l_parse_data_property_restrictions() {
    let odl = [
        "object Person:",
        "  properties:",
        "    name: string",
        "    age: int",
        "object TaggedPerson extends Person:",
        "  properties:",
        "    note: string",
        "    status: string",
        "  data_restrictions:",
        "    note: some string",
        "    note: only string",
        "    note: min 1 string",
        "    status: max 0 string",
        "    status: value \"active\"",
    ]
    .join("\n");
    let ir = parse_odl(&odl).unwrap();
    let expected = vec![
        DataPropertyRestriction {
            property: "note".to_owned(),
            kind: DataRestrictionKind::Some {
                datatype: ScalarType::String,
            },
        },
        DataPropertyRestriction {
            property: "note".to_owned(),
            kind: DataRestrictionKind::Only {
                datatype: ScalarType::String,
            },
        },
        DataPropertyRestriction {
            property: "note".to_owned(),
            kind: DataRestrictionKind::Min {
                cardinality: 1,
                datatype: ScalarType::String,
            },
        },
        DataPropertyRestriction {
            property: "status".to_owned(),
            kind: DataRestrictionKind::Max {
                cardinality: 0,
                datatype: ScalarType::String,
            },
        },
        DataPropertyRestriction {
            property: "status".to_owned(),
            kind: DataRestrictionKind::Value {
                literal: DataLiteral::String("active".to_owned()),
            },
        },
    ];
    assert_eq!(expected, ir.find("TaggedPerson").unwrap().data_restrictions);
    assert!(codegen::roundtrip_ok(&ir).unwrap());

    let undeclared_property = parse_odl(
        "object Person:\n  properties:\n    name: string\n  data_restrictions:\n    missing: some string\n",
    )
    .unwrap_err();
    assert_eq!(1016, undeclared_property.code);
    assert_eq!(5, undeclared_property.line);
    assert!(undeclared_property.message.contains("undeclared property"));

    let datatype_mismatch = parse_odl(
        "object Person:\n  properties:\n    name: string\n  data_restrictions:\n    name: some int\n",
    )
    .unwrap_err();
    assert_eq!(1016, datatype_mismatch.code);
    assert!(
        datatype_mismatch
            .message
            .contains("does not match declared type")
    );

    let literal_type_mismatch = parse_odl(
        "object Person:\n  properties:\n    name: string\n  data_restrictions:\n    name: value 42\n",
    )
    .unwrap_err();
    assert_eq!(1016, literal_type_mismatch.code);
    assert!(
        literal_type_mismatch
            .message
            .contains("literal does not match declared type")
    );

    let excessive_cardinality = parse_odl(
        "object Person:\n  properties:\n    age: int\n  data_restrictions:\n    age: min 2 int\n",
    )
    .unwrap_err();
    assert_eq!(1016, excessive_cardinality.code);
    assert!(
        excessive_cardinality
            .message
            .contains("data cardinality restriction must be 0 or 1")
    );

    let invalid_constructor = parse_odl(
        "object Person:\n  properties:\n    name: string\n  data_restrictions:\n    name: maybe string\n",
    )
    .unwrap_err();
    assert_eq!(1016, invalid_constructor.code);
    assert!(
        invalid_constructor
            .message
            .contains("unknown data restriction constructor")
    );
}

#[test]
fn tc001m_parse_object_one_of() {
    let odl = [
        "object Planet:",
        "  properties:",
        "    name: string",
        "one_of Planet = Mercury, Venus, Earth",
    ]
    .join("\n");
    let ir = parse_odl(&odl).unwrap();
    assert_eq!(
        vec![ObjectOneOfGroup {
            class: "Planet".to_owned(),
            individuals: vec!["Mercury".to_owned(), "Venus".to_owned(), "Earth".to_owned(),],
        }],
        ir.object_one_ofs
    );
    assert!(codegen::roundtrip_ok(&ir).unwrap());

    let undeclared_class =
        parse_odl("object Planet:\n  properties:\n    name: string\none_of Star = Sun\n")
            .unwrap_err();
    assert_eq!(1017, undeclared_class.code);
    assert_eq!(4, undeclared_class.line);
    assert!(undeclared_class.message.contains("undeclared"));

    let missing_equals =
        parse_odl("object Planet:\n  properties:\n    name: string\none_of Planet\n").unwrap_err();
    assert_eq!(1017, missing_equals.code);

    let empty_individuals =
        parse_odl("object Planet:\n  properties:\n    name: string\none_of Planet =\n")
            .unwrap_err();
    assert_eq!(1017, empty_individuals.code);

    let duplicate = parse_odl(
        "object Planet:\n  properties:\n    name: string\none_of Planet = Mercury, Mercury\n",
    )
    .unwrap_err();
    assert_eq!(1017, duplicate.code);
}

// ── TC-002：IR 保真（REQ-002） ───────────────────────────────────────────
#[test]
fn tc002_ir_fidelity() {
    let odl =
        "object Customer:\n  properties:\n    name: string\n    level: string\n    score: int\n";
    let ir = parse_odl(odl).unwrap();
    assert!(codegen::roundtrip_ok(&ir).unwrap());
    let back: OntologyIR = serde_json::from_str(&codegen::to_json(&ir).unwrap()).unwrap();
    assert_eq!(ir, back);
}

// ── TC-003：Codegen（REQ-003） ──────────────────────────────────────────
#[test]
fn tc003_codegen() {
    let odl = "object Customer:\n  properties:\n    name: string\n    level: string\n";
    let ir = parse_odl(odl).unwrap();
    let out = codegen::codegen(&ir).unwrap();
    assert!(out.rust.contains("pub struct Customer"));
    assert!(out.rust.contains("pub name: String"));
    assert!(out.rust.contains("pub level: String"));
    assert!(out.rust.contains("pub const CUSTOMER_DEF"));
    assert!(out.rust.contains("ObjectDef"));
    assert!(out.schema_sql.contains("wb_ontology_object"));
    // ontology.json 与 IR 一致
    let back: OntologyIR = serde_json::from_str(&out.ontology_json).unwrap();
    assert_eq!(ir, back);
}

// ── TC-004：Object CRUD（REQ-004） ──────────────────────────────────────
#[tokio::test]
async fn tc004_object_crud() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let r = s
        .create("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await
        .unwrap();
    assert_eq!(r.version, 1);

    let got = s.get("001").await.unwrap().unwrap();
    assert_eq!(got.data, json!({"name": "Apple", "level": "VIP"}));

    let u = s
        .update("001", json!({"name": "Apple", "level": "GOLD"}), 1)
        .await
        .unwrap();
    assert_eq!(u.version, 2, "update 必须使 version 自增");

    s.delete("001").await.unwrap();
    assert!(s.get("001").await.unwrap().is_none(), "删除后不可见");
}

// ── TC-005：LibSQLStorage 读写 Turso（REQ-005） ─────────────────────────
#[tokio::test]
async fn tc005_turso_roundtrip() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    s.create("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await
        .unwrap();
    s.create("Customer", "002", json!({"name": "Tesla", "level": "GOLD"}))
        .await
        .unwrap();

    let q = s.query("Customer").await.unwrap();
    assert_eq!(q.len(), 2, "落库数据应可被 query 取回");
    assert_eq!(q[0].id, "001");
    assert_eq!(q[1].data, json!({"name": "Tesla", "level": "GOLD"}));
}

// ── TC-006：CLI 渲染（REQ-006） ─────────────────────────────────────────
#[test]
fn tc006_render_list() {
    let recs = vec![
        ObjectRecord {
            id: "001".into(),
            r#type: "Customer".into(),
            data: json!({"name": "Apple", "level": "VIP"}),
            version: 1,
        },
        ObjectRecord {
            id: "002".into(),
            r#type: "Customer".into(),
            data: json!({"name": "Tesla", "level": "GOLD"}),
            version: 1,
        },
    ];
    let out = LibSQLStorage::render_object_list(&recs);
    assert_eq!(out, "001 Apple VIP\n002 Tesla GOLD");
}

// ── TC-007：世界存储 CRUD + 显式 world 事件（M1d：RuntimeKernel 自动发射删除，
//    事件由调用方 append；观察层契约见 v2_parity g9a）────────────────────
#[tokio::test]
async fn tc007_world_trace() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    s.create("Customer", "001", json!({"name": "Apple"}))
        .await
        .unwrap();
    s.update("001", json!({"name": "Apple", "level": "VIP"}), 1)
        .await
        .unwrap();
    let rec = s.get("001").await.unwrap().unwrap();
    assert_eq!(rec.data["level"], "VIP", "CRUD 存活");

    // 世界事件显式落库（TraceEvent::world 绑定 object_refs）
    for (ty, ref_) in [
        ("ObjectCreated", "001"),
        ("ObjectUpdated", "001"),
        ("ObjectDeleted", "001"),
    ] {
        s.append_trace(&TraceEvent::world(ty, ref_)).await.unwrap();
    }
    let events = s.list_world_events().await.unwrap();
    assert_eq!(events.len(), 3, "3 条显式 world 事件");
    for e in &events {
        assert_eq!(e.kind, TraceKind::World);
        assert_eq!(
            e.object_refs,
            vec!["001".to_string()],
            "必须绑定 object_refs"
        );
    }
    assert_eq!(events[0].event_type, "ObjectCreated");
    assert_eq!(events[2].event_type, "ObjectDeleted");
}

#[tokio::test]
async fn tc007b_missing_refs_blocked() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let ev = TraceEvent {
        id: "x".into(),
        kind: TraceKind::World,
        event_type: "ObjectCreated".into(),
        object_refs: vec![], // 缺 object_refs
        project_id: None,
        run_id: None,
        agent_id: None,
        reasoning: None,
        input: None,
        output: None,
        confidence: None,
        timestamp: "0".into(),
    };
    let err = s.append_trace(&ev).await.unwrap_err();
    match err {
        StorageError::Constraint { code, .. } => assert_eq!(code, "WB2006"),
        _ => panic!("缺 object_refs 应返回 WB2006"),
    }
}

// ── TC-005-b：旧 schema 检测（WB2101，root cause 防护） ─────────────────────
#[tokio::test]
async fn tc005b_open_rejects_legacy_schema() {
    // 真实文件旧 schema（任意外部工具创建的旧列：object_type/from_id/to_id/edge_type，无 wb_meta）。
    // open 必须返 WB2101，而非下游 cryptic 的 no such column。
    let path = std::env::temp_dir().join(format!("oaip_legacy_{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);

    // 用 libsql 造一个“旧库”，模拟外部创建的旧 schema 文件（含 WAL 刷盘）
    {
        let db = libsql::Builder::new_local(&path).build().await.unwrap();
        let conn = db.connect().unwrap();
        conn.execute_batch(
            "CREATE TABLE wb_ontology_object (id TEXT PRIMARY KEY, object_type TEXT, data TEXT);\
             CREATE TABLE wb_relation_edge (id TEXT PRIMARY KEY, edge_type TEXT, from_id TEXT, to_id TEXT);",
        )
        .await
        .unwrap();
        let _ = conn
            .query("PRAGMA wal_checkpoint(TRUNCATE)", libsql::params![])
            .await;
    } // db/conn drop，旧库落盘

    let res = LibSQLStorage::open(path.to_str().unwrap()).await;
    match res {
        Ok(_) => panic!("旧 schema 应返回 WB2101，但实际打开成功"),
        Err(StorageError::Constraint { code, .. }) => assert_eq!(code, "WB2101"),
        Err(other) => panic!("旧 schema 应返回 WB2101，实际: {other:?}"),
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn action_actor_is_declared_and_fails_closed() {
    let odl = [
        "object Customer:",
        "  properties:",
        "    name: string",
        "  actions:",
        "    ContactCustomer:",
        "      planning:",
        "        cost: 100",
        "        prior_confidence: 0.5",
        "        outcome_timing: immediate",
    ]
    .join("\n");

    let default = parse_odl(&odl).expect("缺省 actor 应可解析");
    assert_eq!(
        default.object_types[0].actions[0]
            .planning
            .as_ref()
            .expect("planning 应存在")
            .actor,
        ActorKind::Agent
    );

    let agent = parse_odl(&format!("{odl}\n        actor: agent")).expect("agent 应可解析");
    assert_eq!(
        agent.object_types[0].actions[0]
            .planning
            .as_ref()
            .expect("planning 应存在")
            .actor,
        ActorKind::Agent
    );

    let human = parse_odl(&format!("{odl}\n        actor: human")).expect("human 应可解析");
    assert_eq!(
        human.object_types[0].actions[0]
            .planning
            .as_ref()
            .expect("planning 应存在")
            .actor,
        ActorKind::Human
    );

    let error =
        parse_odl(&format!("{odl}\n        actor: robot")).expect_err("未知 actor 必须解析失败");
    assert!(error.to_string().contains("actor 只能是 agent 或 human"));
}
