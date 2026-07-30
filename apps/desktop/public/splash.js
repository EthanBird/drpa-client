(() => {
  const label = document.getElementById("startup-label");
  const percent = document.getElementById("startup-percent");
  const progress = document.getElementById("startup-progress");
  const file = document.getElementById("startup-file");
  let previousSignature = "";
  let displayedProgress = 2;

  const fallbackSteps = [
    { after: 320, progress: 8, label: "正在定位工作区", file: "workspace://registry" },
    { after: 760, progress: 18, label: "正在加载本地服务", file: "runtime://host" },
    { after: 1220, progress: 34, label: "正在准备知识与索引", file: "knowledge://catalog" },
    { after: 1680, progress: 48, label: "正在装载用户界面", file: "ui://index.html" },
  ];
  const startedAt = performance.now();

  const render = (status) => {
    if (!status) return;
    const next = Math.max(displayedProgress, Math.min(100, Number(status.progress) || 0));
    displayedProgress = next;
    label.textContent = status.label || "正在启动 DRPA";
    percent.textContent = `${Math.round(next)}%`;
    progress.style.width = `${Math.max(2, next)}%`;
    file.textContent = status.currentFile || "desktop://bootstrap";
  };

  const tick = () => {
    const status = window.__DRPA_STARTUP_STATUS__;
    if (status) {
      const signature = JSON.stringify(status);
      if (signature !== previousSignature) {
        previousSignature = signature;
        render(status);
      }
    } else {
      const elapsed = performance.now() - startedAt;
      const fallback = fallbackSteps.filter((step) => elapsed >= step.after).at(-1);
      if (fallback) render(fallback);
    }
    window.setTimeout(tick, 80);
  };

  tick();
})();
