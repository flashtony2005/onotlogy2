#!/usr/bin/env python3
"""M6 unwrap 预算门禁：src/ 中生产代码（非测试）的 `.unwrap()` 数量不得超过预算。
测试代码中的 unwrap 不计入。预算文件：scripts/unwrap-budget.txt（只降不升）。"""
import os, re, sys

root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
budget_file = os.path.join(root, "scripts", "unwrap-budget.txt")
with open(budget_file) as f:
    budget = int(f.read().strip())

count = 0
for dirpath, _, filenames in os.walk(os.path.join(root, "src")):
    for fname in filenames:
        if not fname.endswith(".rs"):
            continue
        path = os.path.join(dirpath, fname)
        with open(path) as f:
            content = f.read()
        # Mark test module starts, then remove them via brace tracking
        cleaned = re.sub(r"#\[cfg\(test\)\]\s*\nmod\s+\w+\s*\{", "MODTEST{", content)
        depth = 0
        prod_lines = []
        for line in cleaned.split("\n"):
            if "MODTEST{" in line:
                depth += 1
                continue
            if depth > 0:
                depth += line.count("{") - line.count("}")
                if depth <= 0:
                    depth = 0
                continue
            prod_lines.append(line)
        count += "\n".join(prod_lines).count(".unwrap()")

if count > budget:
    print(f"❌ unwrap 预算超限：当前 {count} > 预算 {budget}（请清理后下调 scripts/unwrap-budget.txt）")
    sys.exit(1)
print(f"✅ unwrap 预算内：{count} ≤ {budget}")
