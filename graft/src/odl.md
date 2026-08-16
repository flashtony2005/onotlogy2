# src/odl.rs · [[ontology-ir-odl-parsing]]

- OdlError · struct · L37-L42 — pub struct OdlError
- new · function · L45-L52 — pub fn new(code: u32, message: impl Into<String>, line: u32, col: u32) -> Self
- E_OBJECT_NAME · constant · L56-L56 — const E_OBJECT_NAME: u32 = 1004; // object 声明缺少名称
- E_DUP_OBJECT · constant · L57-L57 — const E_DUP_OBJECT: u32 = 1006; // 重复对象定义（同名生成重复 struct）
- E_NO_PROP_BLOCK · constant · L58-L58 — const E_NO_PROP_BLOCK: u32 = 1005; // 属性出现在 properties: 块外
- E_EMPTY_PROP_NAME · constant · L59-L59 — const E_EMPTY_PROP_NAME: u32 = 1003; // 属性名为空 / 无法解析行
- E_UNKNOWN_TYPE · constant · L60-L60 — const E_UNKNOWN_TYPE: u32 = 1002; // 未知标量类型
- E_MISSING_PROPERTY · constant · L61-L61 — const E_MISSING_PROPERTY: u32 = 1001; // object 无任何 property
- Section · enum · L65-L77 — enum Section
- parse_odl · function · L80-L361 — pub fn parse_odl(src: &str) -> Result<OntologyIR, OdlError>
- parse_header · function · L364-L374 — fn parse_header(line: &str) -> Option<&str>
- list_item_value · function · L378-L383 — fn list_item_value<'a>(content: &'a str, key: &str) -> Option<&'a str>
- parse_value_expr · function · L386-L416 — fn parse_value_expr(v: &str, param_names: &[String]) -> ValueExpr
- flush_update · function · L419-L423 — fn flush_update(u: &mut Option<FieldUpdate>, a: &mut Option<(u32, ActionDef)>)
- flush_create · function · L426-L430 — fn flush_create(c: &mut Option<CreateEffect>, a: &mut Option<(u32, ActionDef)>)
- flush_action · function · L433-L437 — fn flush_action(a: &mut Option<(u32, ActionDef)>, o: &mut Option<(u32, ObjectType)>)
- flush_clause · function · L440-L444 — fn flush_clause(c: &mut Option<Clause>, a: &mut Option<(u32, ActionDef)>)
- parse_compare_op · function · L447-L456 — fn parse_compare_op(v: &str) -> CompareOp
