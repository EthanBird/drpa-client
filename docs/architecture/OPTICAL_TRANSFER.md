# 光学文件传输架构

## 目标与来源边界

光学传输用于两台不能建立网络连接的设备：发送端将文件变成持续更新的二维码，接收端通过摄像头恢复文件。整个过程不访问网络，也不要求管理员权限。

桌面端额外提供固定的手机网页版入口二维码。手机可以联网加载 GitHub Pages 上的静态 PWA，从而零安装加入传输；网络只承载应用代码，文件数据仍只经过屏幕与摄像头组成的光学通道。

吞吐架构参考 [`decimen-optical-transfer`](https://github.com/bashalarmistalt/decimen-optical-transfer) 的公开[协议](https://github.com/bashalarmistalt/decimen-optical-transfer/blob/main/docs/technical/protocol.md)和[架构](https://github.com/bashalarmistalt/decimen-optical-transfer/blob/main/docs/technical/architecture.md)设计：高密度帧、系统帧加修复帧、固定 QR 掩码、Worker 解码池、忙时丢弃旧画面。该项目当前版本使用 AGPL-3.0-or-later，DRPA 没有复制或链接其代码及 WASM 解码器，而是独立实现 `DRPA2` 协议和浏览器流水线。

## DRPA2 帧协议

每个帧由 30 字节二进制头和固定长度数据块组成，直接写入 QR Byte 模式。接收端仍兼容上一版 `DRPA2:<base45(frame)>` 文本帧，但新发送端不再承担 Base45 的密度和转换开销。

```text
30-byte header | fixed-size fountain block
```

帧头包含魔数、协议版本、会话 ID、绝对序号、源块数、块大小、容器长度、容器 CRC32 和当前数据块 CRC32。最大帧 2953 B 对应 QR Version 40-L 的 Byte 模式上限。

文件容器保存文件名、MIME、原始大小、传输大小和 SHA-256。对于适合压缩的数据，发送端使用浏览器 `CompressionStream` 尝试 gzip，只有确实缩小时才采用；接收端有严格的展开大小上限。

## Fountain 恢复

一个周期由两部分组成：

1. 系统阶段逐个发送原始源块，低丢帧时无需额外开销；
2. 修复阶段发送由确定性伪随机源块集合 XOR 得到的修复块。

接收端对任意顺序的帧做 peeling 解码。漏掉一个二维码不再需要等待整个文件轮播，后续修复帧可以立即补齐；重复帧和 CRC 错误帧会被丢弃。重组后依次验证容器 CRC32、解压长度和文件 SHA-256。

## 发送热路径

- 默认 1465 B、24 FPS、单码，理论净载荷约 33.6 KB/s，优先保证普通设备稳定锁定；
- 可选 900/1465/1850/2331/2953 B、12–60 FPS、1/2/4 个二维码；
- 每个同屏二维码使用不同的周期相位，一个二维码被遮挡时其他二维码仍能推进；
- QR 掩码固定为 2，省去每帧八种掩码的评分；
- 以“一模块一像素”生成紧凑 RGBA 栅格，再由 CSS 整数化放大，避免反复绘制 520×520 Canvas；
- React 状态最多每 200 ms 更新一次，二维码画布更新不触发整页 30–60 次/秒重渲染。

二进制 QR 去除了 Base45 的 50% 文本膨胀，给相同净载荷留下更大的模块尺寸。发送端矩阵生成远高于 60 FPS，因此主要瓶颈转移到真实光学链路和接收解码。

## 接收热路径

- 使用 [`zxing-wasm`](https://github.com/Sec-ant/zxing-wasm) 3.1.3 的 reader-only ZXing-C++ WASM；包装层为 MIT，ZXing-C++ 为 Apache-2.0；
- 创建 1–3 个独立 Worker，WASM 初始化失败时才使用 `jsQR` 兼容路径；
- 初始阶段以全帧最多四码扫描获取二维码四角位置；锁定后给空闲 Worker 分配带 32% 前导边距的小区域裁剪；
- 健康跟踪时每 1.2 秒做一次全帧重定位，区域消失时恢复每 80 ms 的获取扫描；
- 使用 `requestVideoFrameCallback` 按真实相机新帧调度，旧浏览器才回退 `requestAnimationFrame`；
- 每个 Worker 只持有一个在途画面，全部忙时直接丢弃新捕获帧，不积压已经过时的相机画面；
- 全帧最长边限制为 1280 px，区域跟踪后大多数解码只处理二维码周围的小图；
- 自动请求连续对焦，并在授权后提供摄像头选择器，解决手机自动选错长焦/前置镜头的问题；
- Worker 和约 1.04 MiB 的 WASM 文件进入构建期预缓存清单，GitHub Pages/PWA 离线运行不依赖 CDN。

合成无噪声 QR 基准中，1465 B 单码 WASM 解码约 4.9 ms，2331 B 约 6.6 ms，2953 B 约 9.3 ms；四个 1465 B 二维码同帧解码约 11.7 ms。该结果用于确认前端解码器余量，不等同于真实摄像头成绩。

## 完整性与安全边界

- 光学隔离不是加密：任何能看到二维码的摄像头都可能接收文件；
- 文件在发送端只读入本机内存，不上传；
- 摄像头画面只在当前浏览器和 Worker 内解码，不录制、不上传；
- 单帧 CRC32 拒绝损坏块，容器 CRC32 拒绝错误重组，SHA-256 验证最终文件；
- 保存必须由用户通过系统保存对话框选择路径；
- 文件名会去除路径和 Windows 非法字符，二维码中的名称不会直接成为写入路径。
