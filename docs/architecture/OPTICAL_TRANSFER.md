# 光学文件传输架构

## 目标与来源边界

光学传输用于两台不能建立网络连接的设备：发送端将文件变成持续更新的二维码，接收端通过摄像头恢复文件。整个过程不访问网络，也不要求管理员权限。

吞吐架构参考 [`decimen-optical-transfer`](https://github.com/bashalarmistalt/decimen-optical-transfer) 的公开[协议](https://github.com/bashalarmistalt/decimen-optical-transfer/blob/main/docs/technical/protocol.md)和[架构](https://github.com/bashalarmistalt/decimen-optical-transfer/blob/main/docs/technical/architecture.md)设计：高密度帧、系统帧加修复帧、固定 QR 掩码、Worker 解码池、忙时丢弃旧画面。该项目当前版本使用 AGPL-3.0-or-later，DRPA 没有复制或链接其代码及 WASM 解码器，而是独立实现 `DRPA2` 协议和浏览器流水线。

## DRPA2 帧协议

每个帧先组成 30 字节二进制头和固定长度数据块，再编码为 QR Alphanumeric 模式可承载的 Base45 文本：

```text
DRPA2:<base45(frame)>
```

帧头包含魔数、协议版本、会话 ID、绝对序号、源块数、块大小、容器长度、容器 CRC32 和当前数据块 CRC32。最大帧 2860 B 经 Base45 后正好不超过 QR Version 40-L 的 4296 个字母数字字符。

文件容器保存文件名、MIME、原始大小、传输大小和 SHA-256。对于适合压缩的数据，发送端使用浏览器 `CompressionStream` 尝试 gzip，只有确实缩小时才采用；接收端有严格的展开大小上限。

## Fountain 恢复

一个周期由两部分组成：

1. 系统阶段逐个发送原始源块，低丢帧时无需额外开销；
2. 修复阶段发送由确定性伪随机源块集合 XOR 得到的修复块。

接收端对任意顺序的帧做 peeling 解码。漏掉一个二维码不再需要等待整个文件轮播，后续修复帧可以立即补齐；重复帧和 CRC 错误帧会被丢弃。重组后依次验证容器 CRC32、解压长度和文件 SHA-256。

## 发送热路径

- 默认 2200 B、30 FPS、双码，理论净载荷约 127 KB/s；
- 可选 900/1465/2200/2860 B、12–60 FPS、1/2/4 个二维码；
- 每个同屏二维码使用不同的周期相位，一个二维码被遮挡时其他二维码仍能推进；
- QR 掩码固定为 2，省去每帧八种掩码的评分；
- 以“一模块一像素”生成紧凑 RGBA 栅格，再由 CSS 整数化放大，避免反复绘制 520×520 Canvas；
- React 状态最多每 200 ms 更新一次，二维码画布更新不触发整页 30–60 次/秒重渲染。

本机编码基准中，1 MB 随机数据、2200 B 帧的“Fountain 编码 + Base45 + QR 矩阵生成”约为 605 帧/秒，远高于 30–60 FPS 的显示需求，因此发送端不再是主要瓶颈。该数字是编码基准，不是摄像头实测吞吐。

## 接收热路径

- 支持浏览器原生 `BarcodeDetector` 时优先使用，可一次返回多个二维码；
- 否则创建 1–4 个内联 `jsQR` Worker，按逻辑 CPU 数自动选择；
- Worker 会擦除已识别区域并继续扫描同一画面，最多识别四码；
- 每个 Worker 只持有一个在途画面，全部忙时直接丢弃新捕获帧，不积压已经过时的相机画面；
- 摄像头画面最长边限制为 960 px，降低主线程像素读回和 Worker 传输成本；
- Worker 内联进 Web 构建，GitHub Pages/PWA 离线运行时不需要额外网络依赖。

## 完整性与安全边界

- 光学隔离不是加密：任何能看到二维码的摄像头都可能接收文件；
- 文件在发送端只读入本机内存，不上传；
- 摄像头画面只在当前浏览器和 Worker 内解码，不录制、不上传；
- 单帧 CRC32 拒绝损坏块，容器 CRC32 拒绝错误重组，SHA-256 验证最终文件；
- 保存必须由用户通过系统保存对话框选择路径；
- 文件名会去除路径和 Windows 非法字符，二维码中的名称不会直接成为写入路径。
