//! SkillGraph —— Skill 依赖关系图（DAG + Feedback Loop）。

use super::asset::SkillAsset;

/// Skill 图（节点 + 组合边 + 反馈边）。
#[derive(Debug, Clone, Default)]
pub struct SkillGraph {
    pub nodes: Vec<String>,
    /// 组合/阶段边（DAG 判定用）。
    pub edges: Vec<(String, String)>,
    /// 反馈环（不计入环检测）。
    pub feedback: Vec<(String, String)>,
}

impl SkillGraph {
    /// 从 Assets 构建：节点 = asset id + 子技能（workflow 阶段名）；边 = 阶段顺序；
    /// 反馈 = asset.feedback_loops。
    pub fn build(assets: &[SkillAsset]) -> Self {
        let mut g = SkillGraph::default();
        for a in assets {
            if !g.nodes.contains(&a.id) {
                g.nodes.push(a.id.clone());
            }
            for w in &a.workflow {
                if !g.nodes.contains(&w.name) {
                    g.nodes.push(w.name.clone());
                }
            }
            for pair in a.workflow.windows(2) {
                let (f, t) = (&pair[0].name, &pair[1].name);
                if !g.edges.contains(&(f.clone(), t.clone())) {
                    g.edges.push((f.clone(), t.clone()));
                }
            }
            for fl in &a.feedback_loops {
                let (f, t) = (&fl.from, &fl.to);
                if !g.feedback.contains(&(f.clone(), t.clone())) {
                    g.feedback.push((f.clone(), t.clone()));
                }
            }
        }
        g
    }

    /// 邻居节点（出边 + 入边）。
    pub fn neighbors(&self, id: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (f, t) in &self.edges {
            if f == id && !out.contains(t) {
                out.push(t.clone());
            }
            if t == id && !out.contains(f) {
                out.push(f.clone());
            }
        }
        out
    }

    /// DAG 环检测（反馈边不参与——反馈是语义环，非执行依赖环）。
    pub fn has_cycle(&self) -> bool {
        // 拓扑排序（Kahn）
        let mut indegree: std::collections::HashMap<&str, usize> =
            self.nodes.iter().map(|n| (n.as_str(), 0)).collect();
        let mut adj: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
        for (f, t) in &self.edges {
            *indegree.entry(t).or_insert(0) += 1;
            adj.entry(f).or_default().push(t);
        }
        let mut queue: Vec<&str> = indegree
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(n, _)| *n)
            .collect();
        let mut visited = 0usize;
        while let Some(n) = queue.pop() {
            visited += 1;
            if let Some(nexts) = adj.get(n) {
                for t in nexts {
                    let d = indegree.get_mut(t).expect("node");
                    *d -= 1;
                    if *d == 0 {
                        queue.push(t);
                    }
                }
            }
        }
        visited < self.nodes.len()
    }
}
