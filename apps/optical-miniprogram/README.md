# DRPA 光学传输微信小程序

原生微信小程序接收和发送 DRPA2 动态二维码文件流。文件仅在设备内存和微信临时目录中处理，不上传服务器。

接收端优先使用 `CameraContext.onCameraFrame → ExperimentalWorker → ZXing-C++ WebAssembly`。支持 `Worker.getCameraFrameData` 的客户端直接由 Worker 读取当前相机帧，避免 RGBA 大数组跨线程复制；不支持时自动切换为兼容帧复制，WASM 初始化或运行失败时再回退到微信原生 `scanCode`。WASM 文件随代码包本地分发，不访问 CDN。

## 本地构建

```powershell
npm install
npm run build:miniprogram
npm run test:wasm --workspace @drpa/optical-miniprogram
```

随后在微信开发者工具中导入 `apps/optical-miniprogram`。项目 AppID 已写入 `project.config.json`，小程序根目录为自动生成的 `miniprogram/`。

`test:wasm` 会把文本二维码和原始二进制二维码渲染成 RGBA 像素，交给最终 Worker 产物解码，用于验证 WASM 装载与二进制数据保真。

正式上传前需要在微信公众平台配置“相机”隐私用途，并使用 Android 和 iOS 真机验证自动对焦、屏幕频闪、真实吞吐率、动态二维码接收与 `wx.shareFileMessage`。微信模拟器只能验证 Worker/WASM 运行时，不能替代真机摄像头测试。
