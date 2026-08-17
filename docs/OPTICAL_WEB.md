# DRPA 光学传输 Web 版

## 可行性结论

Web 版是纯静态 Vite 应用，可以直接托管到 GitHub Pages。文件分片、二维码生成、摄像头识别、CRC32 与 SHA-256 都在浏览器中执行，不需要 API、数据库或服务端函数。

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

## 安全与浏览器限制

- 文件不会被上传到 GitHub；GitHub Pages 只提供应用的静态代码。
- 光学二维码未加密，旁观摄像头仍可能读取内容。
- 摄像头权限由浏览器按站点 origin 管理，可随时在地址栏权限设置中撤销。
- 接收页必须保持前台；移动浏览器进入后台后通常会暂停摄像头和动画计时器。
- iOS Safari、Android Chrome、桌面 Chrome/Edge 的具体摄像头选择行为不同，页面使用 `environment` 作为后置摄像头偏好，但最终设备由浏览器决定。
- GitHub Pages 站点是公开可访问的，即使源仓库在某些付费计划下设为私有，也不应在静态资源中放置密钥或敏感配置。
