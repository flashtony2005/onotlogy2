# src/action.rs · [[action-engine]] [[idempotency-stable-ids]]

- ActionRegistry · struct · L20-L22 — pub struct ActionRegistry
- empty · function · L26-L28 — pub fn empty() -> Self
- from_ir · function · L31-L41 — pub fn from_ir(ir: &OntologyIR) -> Self
- get · function · L44-L46 — pub fn get(&self, object_type: &str, action: &str) -> Option<&ActionDef>
- actions_of · function · L49-L54 — pub fn actions_of(&self, object_type: &str) -> Vec<&ActionDef>
- has_actions · function · L57-L59 — pub fn has_actions(&self, object_type: &str) -> bool
- is_empty · function · L62-L64 — pub fn is_empty(&self) -> bool
- ActionError · enum · L69-L78 — pub enum ActionError
- ActionEffect · struct · L82-L89 — pub struct ActionEffect
- ActionEngine · struct · L92-L94 — pub struct ActionEngine<'a>
- new · function · L97-L99 — pub fn new(kernel: &'a RuntimeKernel) -> Self
- invoke · function · L103-L182 — pub async fn invoke(
- resolve · function · L186-L193 — fn resolve(v: &ValueExpr, args: &HashMap<String, Value>, target: &Value, target_id: &str) -> Value
