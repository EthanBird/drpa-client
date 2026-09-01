import { describe, expect, it } from "vitest";

import { desktopGateway } from "./gateway";

describe("Studio mock project template", () => {
  it("creates the same AI-oriented README contract as the desktop host", async () => {
    const project = await desktopGateway.createStudioProject("Mock # AI\n`项目`");
    const readme = await desktopGateway.readProjectFile(project.id, "README.md");
    const manifest = await desktopGateway.readProjectFile(project.id, "manifest.yaml");

    expect(project.files).toContain("README.md");
    expect(readme).toMatch(/^# Mock \\# AI \\`项目\\`$/m);
    expect(readme).toContain("`manifest.yaml` → `main.py` → `README.md`");
    expect(readme).toContain("RPAZ schema 2");
    expect(readme).toContain("`ctx.params`");
    expect(readme).toContain("离线依赖");
    expect(readme).toContain("`import rpa as r`");
    expect(readme).toContain("RPA for Python");
    expect(readme).toContain("`offline/requirements/runtime.txt`");
    expect(readme).toContain("全量 sealed runtime");
    expect(readme).toContain("导出 RPAZ");
    expect(readme).toContain("Python Flow");
    expect(readme).toContain("唯一事实源（SSOT）");
    expect(readme).not.toContain("manifest 的 `dependencies`");
    expect(readme).not.toContain("wheel 放入 RPAZ 约定目录");
    expect(manifest).toContain(`name: ${JSON.stringify("Mock # AI\n`项目`")}`);
  });
});
