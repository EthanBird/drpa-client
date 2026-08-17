import { GitFork, RadioTower, ShieldCheck } from "lucide-react";

import { OpticalTransferPage } from "../../desktop/src/pages/OpticalTransferPage";

export function WebApp() {
  return (
    <div className="optical-web-shell">
      <header className="optical-web-bar">
        <a className="optical-web-brand" href="./" aria-label="DRPA 光学传输主页"><span><RadioTower size={16} /></span><strong>DRPA</strong><i>OPTICAL</i></a>
        <div className="optical-web-trust"><ShieldCheck size={14} /><span>浏览器本地处理 · 可离线使用</span></div>
        <a className="optical-web-source" href="https://github.com/EthanBird/drpa-client" target="_blank" rel="noreferrer"><GitFork size={15} /> 源代码</a>
      </header>
      <OpticalTransferPage />
    </div>
  );
}
