# DRPA 光学传输 Web 版

## 可行性结论

Web 版是纯静态 Vite 应用，可以直接托管到 GitHub Pages。文件压缩、Fountain 编码、二维码生成、摄像头识别、CRC32 与 SHA-256 都在浏览器中执行，不需要 API、数据库或服务端函数。

DRPA 桌面端的光学传输页会在左侧显示“手机零安装”二维码，指向 `https://ethanbird.github.io/drpa-client/`。手机联网只用于加载并缓存静态应用；选择的文件、摄像头画面和传输帧不会上传到 GitHub 或其他服务器。扫码后手机可以自行选择发送或接收方向，与桌面端使用相同的 DRPA2 协议。

GitHub Pages 默认提供 HTTPS，而 `getUserMedia()` 摄像头接口只在 HTTPS 或 localhost 安全上下文中可用，因此 Pages 的托管模型与接收端要求匹配。浏览器仍会在首次接收时要求用户明确授予摄像头权限。

默认地址为：

```text
https://ethanbird.github.io/drpa-client/
```

Vite 使用相对资源路径，同一构建也能放在自定义域名根目录或任意静态子目录。

## 本地开发与构建

```powershell
npm install
npm run dev:optical
npm run typecheck:optical
npm run build:optical
```

静态产物位于 `apps/optical-web/dist`。可以用任意静态 HTTP 服务器预览；不要直接双击 `index.html` 验证摄像头权限。

## GitHub Pages 发布

仓库包含 `.github/workflows/optical-pages.yml`，会构建 `@drpa/optical-web` 并只上传 `apps/optical-web/dist`：

1. 打开仓库 `Settings → Pages`；
2. 将 `Build and deployment → Source` 设为 `GitHub Actions`；
3. 推送涉及 Web 版的改动，或在 Actions 页面手动运行 `Deploy optical transfer web app`；
4. 部署完成后从 `github-pages` environment 或 Pages 设置页打开地址。

若不希望使用 Actions，也可以在本地执行 `npm run build:optical`，再把 `apps/optical-web/dist` 的内容发布到独立 `gh-pages` 分支。不要把整个仓库作为 Pages 站点公开目录。

## 离线能力

Web 版注册同源 Service Worker。首次打开时会预缓存入口、当前哈希版本的 JS/CSS、图标和 manifest；此后即使网络断开，仍可以重新打开页面并传输文件。这种缓存只保证已经成功打开过的浏览器与站点版本，清理浏览器站点数据后需要重新联网访问一次。

ZXing-C++ 解码 Worker、reader-only WASM、主程序和样式都写入构建期生成的预缓存清单。首次在线打开完成 Service Worker 安装后，这些资源可离线使用；应用升级时缓存版本会整体切换。

## 高速模式

默认设置是 1465 B、24 FPS、单二维码，理论净载荷约 33.6 KB/s；旧版 480 B、8 FPS、单二维码约为 3.75 KB/s。默认值以普通 60 Hz 显示器和手机摄像头的稳定锁定为先，高速档仍可达到 2953 B、60 FPS、四二维码。理论值不等于摄像头实际吞吐，实际速度取决于显示器刷新率、摄像头快门、距离、对焦和设备解码性能。

- 普通显示器：24 FPS、1465 B、单码；
- 近距离大屏：30 FPS、1850/2331 B、双码；
- 120 Hz 大屏和高性能手机：45–60 FPS、2953 B、四码；
- 接收端统一使用 reader-only ZXing-C++ WASM；若 WASM 无法初始化，才降级到 `jsQR`。

发送端使用二进制 QR，固定掩码并直接栅格化模块矩阵，避免 Base45 膨胀、每帧八种掩码评分和大尺寸 Canvas 重绘。接收端由相机新帧事件驱动：首次做全帧多码扫描，锁定后只解码二维码附近的小区域，并定期全帧重定位。Worker 全忙时直接丢弃旧画面，Fountain 修复帧负责恢复缺失数据。

接收面板实时显示捕获 FPS、解码 FPS、Worker 数、跟踪区域和忙丢帧数。授权摄像头后可以手动切换镜头，避免部分手机将 `environment` 自动映射到长焦或前置镜头。

## 安全与浏览器限制

- 文件不会被上传到 GitHub；GitHub Pages 只提供应用的静态代码。
- 光学二维码未加密，旁观摄像头仍可能读取内容。
- 摄像头权限由浏览器按站点 origin 管理，可随时在地址栏权限设置中撤销。
- 接收页必须保持前台；移动浏览器进入后台后通常会暂停摄像头和动画计时器。
- iOS Safari、Android Chrome、桌面 Chrome/Edge 的具体摄像头选择行为不同，页面使用 `environment` 作为后置摄像头偏好，但最终设备由浏览器决定。
- GitHub Pages 站点是公开可访问的，即使源仓库在某些付费计划下设为私有，也不应在静态资源中放置密钥或敏感配置。
