import QRCode from "qrcode";

import {
  createOpticalTransfer,
  DEFAULT_OPTICAL_FRAME_BYTES,
  MAX_OPTICAL_FILE_BYTES,
  OpticalReceiver,
  type OpticalReceivedFile,
  type OpticalTransfer,
} from "../../runtime/optical";

const fileSystem = wx.getFileSystemManager();
let receiver = new OpticalReceiver();
let sendTransfer: OpticalTransfer | null = null;
let sendTimer: ReturnType<typeof setInterval> | null = null;
let sendSequence = 0;
let verifying = false;
let receivedPath = "";
let qrCanvas: any = null;
let qrContext: any = null;
let decodeWorker: WechatMiniprogram.Worker | null = null;
let cameraFrameListener: WechatMiniprogram.CameraFrameListener | null = null;
let decoderReady = false;
let directCameraFrames = false;
let frameInFlight = false;
let frameId = 0;
let lastFrameDispatchedAt = 0;
let lastDecoderMetricAt = 0;
let decoderErrors = 0;

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(2)} MB`;
}

function safeFileName(name: string): string {
  return (name.split(/[\\/]/).pop() || "received-file.bin").replace(/[\u0000-\u001f<>:"|?*]/g, "_").slice(0, 160);
}

function readFile(path: string): Promise<Uint8Array> {
  return new Promise((resolve, reject) => {
    fileSystem.readFile({
      filePath: path,
      success: ({ data }) => resolve(new Uint8Array(data as ArrayBuffer)),
      fail: reject,
    });
  });
}

function writeReceivedFile(file: OpticalReceivedFile): Promise<string> {
  const path = `${wx.env.USER_DATA_PATH}/drpa-${Date.now()}-${safeFileName(file.name)}`;
  const stable = Uint8Array.from(file.bytes);
  return new Promise((resolve, reject) => {
    fileSystem.writeFile({
      filePath: path,
      data: stable.buffer,
      success: () => resolve(path),
      fail: reject,
    });
  });
}

function removeTemporaryFile(path: string): void {
  if (!path) return;
  fileSystem.unlink({ filePath: path, fail: () => undefined });
}

function stopSender(): void {
  if (sendTimer) clearInterval(sendTimer);
  sendTimer = null;
}

function stopCameraFrames(): void {
  if (cameraFrameListener) {
    try {
      cameraFrameListener.stop();
    } catch {
      // The camera may already have been released by WeChat.
    }
  }
  cameraFrameListener = null;
  frameInFlight = false;
}

function terminateDecoder(): void {
  stopCameraFrames();
  const currentWorker = decodeWorker;
  decodeWorker = null;
  if (currentWorker) {
    try {
      currentWorker.terminate();
    } catch {
      // A reclaimed ExperimentalWorker is already terminated.
    }
  }
  decoderReady = false;
  directCameraFrames = false;
  decoderErrors = 0;
}

const page: any = {
  data: {
    mode: "receive",
    scanning: false,
    preparing: false,
    sending: false,
    fps: 8,
    notice: "文件仅在本机处理，不上传服务器。",
    receiveName: "等待扫描…",
    receivePercent: 0,
    receivedChunks: 0,
    totalChunks: 0,
    acceptedFrames: 0,
    receivedBytes: "0 B",
    decoderState: "fallback",
    decoderLabel: "原生扫码待命",
    decoderFps: 0,
    decoderLatency: 0,
    receivedReady: false,
    receivedName: "",
    receivedSize: "",
    receivedSha256: "",
    sendReady: false,
    sendFileName: "",
    sendFileSize: "",
    sendSha256: "",
    sendChunks: 0,
    sendCompression: "未压缩",
    sendFrameLabel: "0 / 0",
  },

  onLoad() {
    receiver = new OpticalReceiver();
    sendTransfer = null;
    verifying = false;
    receivedPath = "";
    terminateDecoder();
  },

  onHide() {
    stopSender();
    terminateDecoder();
    this.setData({ sending: false, scanning: false });
  },

  onUnload() {
    stopSender();
    terminateDecoder();
    removeTemporaryFile(receivedPath);
  },

  switchMode(event: WechatMiniprogram.BaseEvent) {
    const mode = String((event.currentTarget.dataset as { mode?: string }).mode || "receive");
    if (mode === this.data.mode) return;
    stopSender();
    if (mode !== "receive") terminateDecoder();
    this.setData({ mode, sending: false, scanning: false });
    if (mode === "send" && sendTransfer) wx.nextTick(() => void this.renderSendFrame());
  },

  startReceive() {
    stopSender();
    wx.authorize({
      scope: "scope.camera",
      success: () => {
        this.ensureDecoderWorker();
        const workerStarted = Boolean(decodeWorker);
        this.setData({
          scanning: true,
          decoderState: workerStarted ? (decoderReady ? "ready" : "loading") : "fallback",
          decoderLabel: workerStarted
            ? (decoderReady ? (directCameraFrames ? "ZXing-C++ WASM · 零拷贝帧" : "ZXing-C++ WASM · 兼容帧") : "正在启动 WASM 解码器…")
            : "原生扫码回退",
          notice: workerStarted ? "请让电脑端动态二维码完整进入取景框。" : "WASM 解码器不可用，已启用原生扫码回退。",
        });
      },
      fail: () => {
        wx.showModal({
          title: "需要相机权限",
          content: "相机画面只用于本机识别 DRPA 二维码，不会上传。",
          confirmText: "去设置",
          success: ({ confirm }) => {
            if (confirm) wx.openSetting();
          },
        });
      },
    });
  },

  stopReceive() {
    stopCameraFrames();
    this.setData({ scanning: false, notice: "扫描已暂停，当前恢复进度已保留。" });
  },

  ensureDecoderWorker() {
    if (decodeWorker) return;
    directCameraFrames = Boolean(wx.canIUse?.("Worker.getCameraFrameData"));
    decoderReady = false;
    decoderErrors = 0;
    try {
      const instance = wx.createWorker("workers/optical-decoder.js", {
        useExperimentalWorker: directCameraFrames,
      });
      decodeWorker = instance;
      instance.onMessage((event) => this.onDecoderMessage((event as any)?.message ?? event));
      instance.onError((event) => {
        this.disableWasmDecoder(`WASM Worker 异常：${(event as any)?.error?.message || (event as any)?.message || "未知错误"}`);
      });
      instance.onProcessKilled(() => {
        this.disableWasmDecoder("WASM Worker 被系统回收，已切换原生扫码。");
      });
    } catch (error) {
      this.disableWasmDecoder(`无法启动 WASM Worker：${String(error)}`);
    }
  },

  disableWasmDecoder(notice: string) {
    terminateDecoder();
    this.setData({
      decoderState: "fallback",
      decoderLabel: "原生扫码回退",
      decoderFps: 0,
      decoderLatency: 0,
      notice,
    });
  },

  onDecoderMessage(message: any) {
    if (!message || typeof message !== "object") return;
    if (message.type === "ready") {
      decoderReady = true;
      decoderErrors = 0;
      this.setData({
        decoderState: "ready",
        decoderLabel: directCameraFrames ? "ZXing-C++ WASM · 零拷贝帧" : "ZXing-C++ WASM · 兼容帧",
      });
      return;
    }
    if (message.type === "wasm-error") {
      this.disableWasmDecoder(`WASM 初始化失败，已回退原生扫码：${message.message || "未知错误"}`);
      return;
    }

    frameInFlight = false;
    this.updateDecoderMetrics(message);
    if (message.type === "frame-unavailable" && directCameraFrames) {
      directCameraFrames = false;
      this.setData({ decoderState: "ready", decoderLabel: "ZXing-C++ WASM · 兼容帧" });
      if (this.data.scanning) this.startCameraFrames();
      return;
    }
    if (message.type === "decode-error") {
      decoderErrors += 1;
      if (decoderErrors >= 3) this.disableWasmDecoder(`WASM 连续解码失败，已回退原生扫码：${message.message || "未知错误"}`);
      return;
    }
    if (message.type !== "decoded") return;

    decoderErrors = 0;
    const text = typeof message.text === "string" ? message.text : "";
    const bytes = message.bytes instanceof ArrayBuffer ? new Uint8Array(message.bytes) : undefined;
    void this.acceptDecodedFrame(text, bytes);
  },

  updateDecoderMetrics(message: any) {
    const now = Date.now();
    const fps = Number(message.fps) || 0;
    if (!fps && now - lastDecoderMetricAt < 600) return;
    lastDecoderMetricAt = now;
    this.setData({
      decoderFps: fps || this.data.decoderFps,
      decoderLatency: Math.max(0, Math.round(Number(message.decodeMs) || 0)),
    });
  },

  onCameraInitDone() {
    if (this.data.scanning) this.startCameraFrames();
  },

  startCameraFrames() {
    stopCameraFrames();
    if (!decodeWorker) return;
    const context = wx.createCameraContext();
    cameraFrameListener = context.onCameraFrame((frame) => {
      if (!this.data.scanning || !decodeWorker || !decoderReady || frameInFlight) return;
      const now = Date.now();
      if (now - lastFrameDispatchedAt < 45) return;
      lastFrameDispatchedAt = now;
      frameInFlight = true;
      frameId = (frameId + 1) >>> 0;
      const message: WechatMiniprogram.IAnyObject = {
        type: "decode",
        frameId,
        width: frame.width,
        height: frame.height,
        direct: directCameraFrames,
      };
      if (!directCameraFrames) message.data = frame.data;
      try {
        decodeWorker.postMessage(message);
      } catch (error) {
        frameInFlight = false;
        this.disableWasmDecoder(`相机帧无法送入 WASM：${String(error)}`);
      }
    });
    const options: WechatMiniprogram.CameraFrameListenerStartOption = directCameraFrames && decodeWorker
      ? { worker: decodeWorker }
      : {};
    options.fail = ({ errMsg }) => {
      if (directCameraFrames) {
        directCameraFrames = false;
        this.setData({ decoderState: "ready", decoderLabel: "ZXing-C++ WASM · 兼容帧" });
        this.startCameraFrames();
      } else {
        this.disableWasmDecoder(`无法读取相机原始帧：${errMsg}`);
      }
    };
    cameraFrameListener.start(options);
  },

  onCameraError(event: WechatMiniprogram.CustomEvent) {
    stopCameraFrames();
    this.setData({ scanning: false, notice: `无法使用相机：${event.detail?.errMsg || "请检查权限"}` });
  },

  onCameraStop() {
    stopCameraFrames();
    if (this.data.scanning) this.setData({ scanning: false, notice: "相机已被系统暂停，请重新开始扫描。" });
  },

  async onScanCode(event: WechatMiniprogram.CustomEvent) {
    const value = String(event.detail?.result || "");
    const rawData = String(event.detail?.rawData || "");
    let bytes: Uint8Array | undefined;
    if (rawData) {
      try {
        bytes = new Uint8Array(wx.base64ToArrayBuffer(rawData));
      } catch {
        bytes = undefined;
      }
    }
    await this.acceptDecodedFrame(value, bytes);
  },

  async acceptDecodedFrame(value: string, bytes?: Uint8Array) {
    if (verifying || this.data.receivedReady) return;
    let accepted = value.startsWith("DRPA2:") && receiver.accept(value);
    if (!accepted && bytes?.byteLength) accepted = receiver.accept(bytes);
    if (!accepted) return;
    const progress = receiver.progress();
    this.setData({
      receiveName: progress.name,
      receivePercent: progress.percent,
      receivedChunks: progress.receivedChunks,
      totalChunks: progress.totalChunks,
      acceptedFrames: progress.acceptedFrames,
      receivedBytes: formatBytes(progress.receivedBytes),
    });
    if (!receiver.isComplete()) return;

    verifying = true;
    stopCameraFrames();
    this.setData({ scanning: false, notice: "正在校验文件完整性…" });
    try {
      const file = await receiver.complete();
      if (!file) throw new Error("文件尚未恢复完成");
      removeTemporaryFile(receivedPath);
      receivedPath = await writeReceivedFile(file);
      this.setData({
        receiveName: file.name,
        receivePercent: 100,
        receivedReady: true,
        receivedName: file.name,
        receivedSize: formatBytes(file.bytes.byteLength),
        receivedSha256: file.sha256,
        notice: "文件校验完成，可直接转发给微信好友或群聊。",
      });
    } catch (error) {
      this.setData({ notice: `校验失败：${String(error)}` });
    } finally {
      verifying = false;
    }
  },

  shareReceivedFile() {
    if (!receivedPath || !this.data.receivedReady) return;
    wx.shareFileMessage({
      filePath: receivedPath,
      fileName: this.data.receivedName,
      success: () => this.setData({ notice: "文件已交给微信转发面板。" }),
      fail: ({ errMsg }) => {
        const canceled = /cancel/i.test(errMsg || "");
        this.setData({ notice: canceled ? "已取消转发，文件仍保留在当前页面。" : `转发失败：${errMsg}` });
      },
    });
  },

  resetReceiver() {
    stopCameraFrames();
    removeTemporaryFile(receivedPath);
    receivedPath = "";
    receiver.reset();
    verifying = false;
    this.setData({
      scanning: false,
      receiveName: "等待扫描…",
      receivePercent: 0,
      receivedChunks: 0,
      totalChunks: 0,
      acceptedFrames: 0,
      receivedBytes: "0 B",
      decoderFps: 0,
      decoderLatency: 0,
      receivedReady: false,
      receivedName: "",
      receivedSize: "",
      receivedSha256: "",
      notice: "接收状态已清空，可以扫描新文件。",
    });
  },

  chooseSendFile() {
    if (this.data.preparing || this.data.sending) return;
    wx.chooseMessageFile({
      count: 1,
      type: "all",
      success: async ({ tempFiles }) => {
        const selected = tempFiles[0];
        if (!selected) return;
        if (!selected.size || selected.size > MAX_OPTICAL_FILE_BYTES) {
          this.setData({ notice: "请选择 1 B–64 MB 的文件。" });
          return;
        }
        this.setData({ preparing: true, notice: "正在读取、压缩并计算文件摘要…" });
        wx.showLoading({ title: "准备文件", mask: true });
        try {
          const bytes = await readFile(selected.path);
          sendTransfer = await createOpticalTransfer(bytes, selected.name, "application/octet-stream", DEFAULT_OPTICAL_FRAME_BYTES);
          sendSequence = 0;
          this.setData({
            sendReady: true,
            sendFileName: sendTransfer.name,
            sendFileSize: formatBytes(sendTransfer.size),
            sendSha256: sendTransfer.sha256,
            sendChunks: sendTransfer.totalChunks,
            sendCompression: sendTransfer.compression === "gzip" ? `${formatBytes(sendTransfer.transmittedSize)} gzip` : "未压缩",
            sendFrameLabel: `1 / ${sendTransfer.totalChunks * 2}`,
            notice: "文件已就绪，点击开始后让接收端摄像头对准二维码。",
          });
          wx.nextTick(() => void this.renderSendFrame());
        } catch (error) {
          sendTransfer = null;
          this.setData({ sendReady: false, notice: `文件准备失败：${String(error)}` });
        } finally {
          wx.hideLoading();
          this.setData({ preparing: false });
        }
      },
      fail: ({ errMsg }) => {
        if (!/cancel/i.test(errMsg || "")) this.setData({ notice: `选择文件失败：${errMsg}` });
      },
    });
  },

  onFpsChange(event: WechatMiniprogram.CustomEvent) {
    const fps = Number(event.detail.value) || 8;
    this.setData({ fps });
    if (this.data.sending) {
      stopSender();
      this.startSendTimer();
    }
  },

  async ensureQrCanvas() {
    if (qrCanvas && qrContext) return;
    await new Promise<void>((resolve, reject) => {
      wx.createSelectorQuery().in(this).select("#qrCanvas").fields({ node: true, size: true }).exec((result) => {
        const entry = result[0] as { node?: any; width?: number; height?: number } | undefined;
        if (!entry?.node || !entry.width) {
          reject(new Error("二维码画布尚未就绪"));
          return;
        }
        const pixelRatio = wx.getWindowInfo?.().pixelRatio || wx.getSystemInfoSync().pixelRatio || 1;
        qrCanvas = entry.node;
        qrContext = qrCanvas.getContext("2d");
        qrCanvas.width = Math.round(entry.width * pixelRatio);
        qrCanvas.height = Math.round((entry.height || entry.width) * pixelRatio);
        resolve();
      });
    });
  },

  async renderSendFrame() {
    if (!sendTransfer) return;
    try {
      await this.ensureQrCanvas();
      const text = sendTransfer.createFrameText(sendSequence);
      const qr = QRCode.create(text, { errorCorrectionLevel: "L", maskPattern: 2 });
      const modules = qr.modules.size;
      const quiet = 4;
      const canvasSize = Math.min(qrCanvas.width, qrCanvas.height);
      const cell = Math.max(1, Math.floor(canvasSize / (modules + quiet * 2)));
      const drawnSize = cell * (modules + quiet * 2);
      const origin = Math.floor((canvasSize - drawnSize) / 2);
      qrContext.fillStyle = "#ffffff";
      qrContext.fillRect(0, 0, qrCanvas.width, qrCanvas.height);
      qrContext.fillStyle = "#0b1220";
      for (let row = 0; row < modules; row += 1) {
        for (let column = 0; column < modules; column += 1) {
          if (qr.modules.data[row * modules + column]) {
            qrContext.fillRect(origin + (column + quiet) * cell, origin + (row + quiet) * cell, cell, cell);
          }
        }
      }
      const cycle = sendTransfer.totalChunks * 2;
      this.setData({ sendFrameLabel: `${(sendSequence % cycle) + 1} / ${cycle}` });
    } catch (error) {
      stopSender();
      this.setData({ sending: false, notice: `二维码绘制失败：${String(error)}` });
    }
  },

  startSendTimer() {
    if (!sendTransfer) return;
    sendTimer = setInterval(() => {
      sendSequence = (sendSequence + 1) >>> 0;
      void this.renderSendFrame();
    }, Math.round(1000 / this.data.fps));
  },

  toggleSending() {
    if (!sendTransfer) return;
    if (this.data.sending) {
      stopSender();
      this.setData({ sending: false, notice: "光学发送已暂停。" });
      return;
    }
    this.setData({ sending: true, notice: "正在循环发送源帧和 Fountain 修复帧。" });
    this.startSendTimer();
  },

  openPrivacy() {
    wx.navigateTo({ url: "/pages/privacy/index" });
  },

  onShareAppMessage() {
    return {
      title: "DRPA 光学传输｜文件不上传网络",
      path: "/pages/transfer/index",
    };
  },
};

Page(page);
