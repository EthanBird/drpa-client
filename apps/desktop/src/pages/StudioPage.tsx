import Editor, { loader } from "@monaco-editor/react";
import * as monaco from "monaco-editor/esm/vs/editor/editor.api.js";
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker.js?worker";
import "monaco-editor/esm/vs/basic-languages/python/python.contribution.js";
import "monaco-editor/esm/vs/basic-languages/yaml/yaml.contribution.js";
import { Box, Code2, FileCode2, FolderTree, PackageCheck, Plus, Save, TerminalSquare } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { useAppStore } from "../app/store";
import type { StudioProject } from "../domain/models";
import { desktopGateway } from "../infra/gateway";

// Force Monaco to use the npm-bundled editor. The default loader uses a CDN,
// which is unacceptable for DRPA's offline deployment model.
self.MonacoEnvironment = { getWorker: () => new EditorWorker() };
loader.config({ monaco });

export function StudioPage() {
  const setSnapshot = useAppStore((state) => state.setSnapshot);
  const setActiveNavigation = useAppStore((state) => state.setActiveNavigation);
  const selectPackage = useAppStore((state) => state.selectPackage);
  const [projects, setProjects] = useState<StudioProject[]>([]);
  const [selectedId, setSelectedId] = useState("");
  const [selectedFile, setSelectedFile] = useState("main.py");
  const [content, setContent] = useState("");
  const [projectName, setProjectName] = useState("Bing 每日一图");
  const [projectId, setProjectId] = useState("com.example.bing-daily-image");
  const [notice, setNotice] = useState("工作室已就绪");
  const [busy, setBusy] = useState(false);

  const selected = useMemo(() => projects.find((item) => item.id === selectedId), [projects, selectedId]);

  const refresh = async () => {
    const next = await desktopGateway.listStudioProjects();
    setProjects(next);
    if (!selectedId && next[0]) setSelectedId(next[0].id);
  };

  useEffect(() => { void refresh(); }, []);
  useEffect(() => {
    if (!selectedId || !selectedFile) return;
    void desktopGateway.readProjectFile(selectedId, selectedFile).then(setContent).catch((error: unknown) => setNotice(String(error)));
  }, [selectedFile, selectedId]);

  const createProject = async () => {
    setBusy(true);
    try {
      const project = await desktopGateway.createStudioProject(projectId.trim(), projectName.trim());
      await refresh();
      setSelectedId(project.id);
      setSelectedFile("main.py");
      setNotice(`已创建项目：${project.name}`);
    } catch (error) {
      setNotice(`创建失败：${String(error)}`);
    } finally { setBusy(false); }
  };

  const save = async () => {
    if (!selectedId) return;
    setBusy(true);
    try {
      await desktopGateway.writeProjectFile(selectedId, selectedFile, content);
      setNotice(`已保存 ${selectedFile}`);
    } catch (error) { setNotice(`保存失败：${String(error)}`); }
    finally { setBusy(false); }
  };

  const buildAndInstall = async () => {
    if (!selectedId) return;
    setBusy(true);
    try {
      await desktopGateway.writeProjectFile(selectedId, selectedFile, content);
      const archivePath = await desktopGateway.buildStudioProject(selectedId);
      const installed = await desktopGateway.installPackage(archivePath);
      setSnapshot(await desktopGateway.getWorkspaceSnapshot());
      selectPackage(installed.id, installed.profiles[0]?.id);
      setNotice(`已构建并安装：${archivePath}`);
      setActiveNavigation("workbench");
    } catch (error) { setNotice(`构建失败：${String(error)}`); }
    finally { setBusy(false); }
  };

  const language = selectedFile.endsWith(".py") ? "python" : selectedFile.endsWith(".yaml") ? "yaml" : "plaintext";

  return (
    <div className="page studio-page">
      <header className="page-header studio-header"><div><div className="eyebrow">RPaz 集成开发环境</div><h1>开发工作室</h1><p>编辑、校验、打包并直接安装脚本包，无需外部 IDE。</p></div><div className="header-actions"><button className="button secondary" type="button" onClick={save} disabled={!selectedId || busy}><Save size={15} /> 保存</button><button className="button primary" type="button" onClick={buildAndInstall} disabled={!selectedId || busy}><PackageCheck size={15} /> 构建并安装</button></div></header>
      <div className="studio-create-bar"><input value={projectName} onChange={(event) => setProjectName(event.target.value)} aria-label="项目名称" placeholder="项目名称" /><input value={projectId} onChange={(event) => setProjectId(event.target.value)} aria-label="项目 ID" placeholder="com.example.project" /><button className="button secondary" type="button" onClick={createProject} disabled={busy}><Plus size={15} /> 新建项目</button></div>
      <div className="studio-layout">
        <aside className="studio-projects"><div className="studio-pane-title"><FolderTree size={15} /> 项目</div>{projects.length === 0 && <p className="empty-hint">尚无项目，请在上方新建。</p>}{projects.map((project) => <button type="button" key={project.id} className={project.id === selectedId ? "selected" : ""} onClick={() => { setSelectedId(project.id); setSelectedFile(project.files[0] ?? "main.py"); }}><Box size={14} /><span><strong>{project.name}</strong><small>{project.id}</small></span></button>)}</aside>
        <aside className="studio-files"><div className="studio-pane-title"><Code2 size={15} /> 文件</div>{selected?.files.map((file) => <button type="button" key={file} className={file === selectedFile ? "selected" : ""} onClick={() => setSelectedFile(file)}><FileCode2 size={14} /> {file}</button>)}</aside>
        <section className="studio-editor"><div className="editor-tab"><FileCode2 size={14} /> {selectedFile || "未选择文件"}<span>{language}</span></div><Editor height="100%" language={language} value={content} onChange={(value) => setContent(value ?? "")} theme="vs-dark" options={{ fontSize: 14, minimap: { enabled: false }, automaticLayout: true, tabSize: 4, wordWrap: "on" }} /></section>
        <footer className="studio-console"><TerminalSquare size={15} /><strong>任务输出</strong><span>{notice}</span></footer>
      </div>
    </div>
  );
}
