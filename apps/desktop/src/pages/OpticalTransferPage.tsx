import { Camera, CheckCircle2, CircleStop, FileUp, Gauge, LoaderCircle, Play, QrCode, RadioTower, RefreshCw, Save, ShieldAlert } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { OpticalDecodePool, type OpticalDecodedSymbol } from "../features/optical/opticalDecodePool";
import {
  DEFAULT_OPTICAL_FRAME_BYTES,
  MAX_OPTICAL_FILE_BYTES,
  OPTICAL_FRAME_BYTE_OPTIONS,
  OpticalReceiver,
  prepareOpticalTransfer,
  type OpticalReceiveProgress,
  type OpticalReceivedFile,
  type OpticalTransfer,
} from "../features/optical/opticalProtocol";
import { renderOpticalQrGrid } from "../features/optical/opticalQr";

export type OpticalFileSaver = (file: OpticalReceivedFile) => Promise<string | null>;

interface NativeBarcodeDetector {
  detect(source: CanvasImageSource): Promise<Array<{ rawValue: string; boundingBox?: DOMRectReadOnly }>>;
}

interface NativeBarcodeDetectorConstructor {
  new(options: { formats: string[] }): NativeBarcodeDetector;
  getSupportedFormats(): Promise<string[]>;
}

const emptyProgress: OpticalReceiveProgress = {
  sessionId: "",
  name: "等待扫描…",
  receivedChunks: 0,
  totalChunks: 0,
  receivedBytes: 0,
  fileSize: 0,
  percent: 0,
};

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(2)} MB`;
}

function formatRate(bytesPerSecond: number): string {
  return `${(bytesPerSecond / 1024).toFixed(bytesPerSecond >= 1024 * 100 ? 0 : 1)} KB/s`;
}

export function OpticalTransferPage({ saveFile }: { saveFile?: OpticalFileSaver } = {}) {
  const [mode, setMode] = useState<"send" | "receive">("send");
  const [transfer, setTransfer] = useState<OpticalTransfer | null>(null);
  const [preparing, setPreparing] = useState(false);
  const [sending, setSending] = useState(false);
  const [fps, setFps] = useState(30);
  const [frameBytes, setFrameBytes] = useState(DEFAULT_OPTICAL_FRAME_BYTES);
  const [codeCount, setCodeCount] = useState(2);
  const [currentFrame, setCurrentFrame] = useState({ sequence: 0, cycle: 0 });
  const [notice, setNotice] = useState("选择文件后，DRPA 会生成可丢帧恢复的高速二维码流。");
  const [scanning, setScanning] = useState(false);
  const [receiveProgress, setReceiveProgress] = useState<OpticalReceiveProgress>(emptyProgress);
  const [receivedFile, setReceivedFile] = useState<OpticalReceivedFile | null>(null);
  const [saving, setSaving] = useState(false);
  const qrCanvasRef = useRef<HTMLCanvasElement>(null);
  const videoRef = useRef<HTMLVideoElement>(null);
  const scanCanvasRef = useRef<HTMLCanvasElement>(null);
  const streamRef = useRef<MediaStream | null>(null);
  const decodePoolRef = useRef<OpticalDecodePool | null>(null);
  const nativeDetectorRef = useRef<NativeBarcodeDetector | null>(null);
  const nativeDetectorBusyRef = useRef(false);
  const scanFrameRef = useRef(0);
  const receiverRef = useRef(new OpticalReceiver());
  const verifyingRef = useRef(false);
  const selectedFileRef = useRef<File | null>(null);
  const lastProgressUpdateRef = useRef(0);

  const stopCamera = () => {
    window.cancelAnimationFrame(scanFrameRef.current);
    scanFrameRef.current = 0;
    decodePoolRef.current?.terminate();
    decodePoolRef.current = null;
    nativeDetectorRef.current = null;
    nativeDetectorBusyRef.current = false;
    streamRef.current?.getTracks().forEach((track) => track.stop());
    streamRef.current = null;
    if (videoRef.current) videoRef.current.srcObject = null;
    setScanning(false);
  };

  useEffect(() => () => stopCamera(), []);

  useEffect(() => {
    if (!transfer || !qrCanvasRef.current) return;
    let disposed = false;
    let animationFrame = 0;
    let sequence = 0;
    let nextFrameAt = 0;
    let lastUiUpdate = 0;
    const cycleLength = transfer.totalChunks * 2;

    const draw = (time: number) => {
      if (disposed || !qrCanvasRef.current) return;
      if (!sending || time >= nextFrameAt) {
        const spacing = Math.max(1, Math.floor(cycleLength / codeCount));
        const sequences = Array.from({ length: sending ? codeCount : 1 }, (_, index) => (sequence + index * spacing) >>> 0);
        try {
          renderOpticalQrGrid(qrCanvasRef.current, sequences.map((value) => transfer.createFrameText(value)));
        } catch (error) {
          setNotice(`二维码生成失败：${String(error)}`);
          return;
        }
        if (time - lastUiUpdate >= 200 || !sending) {
          setCurrentFrame({ sequence, cycle: sequence % cycleLength });
          lastUiUpdate = time;
        }
        if (!sending) return;
        sequence = (sequence + 1) >>> 0;
        nextFrameAt = time + 1000 / fps;
      }
      animationFrame = window.requestAnimationFrame(draw);
    };

    animationFrame = window.requestAnimationFrame(draw);
    return () => {
      disposed = true;
      window.cancelAnimationFrame(animationFrame);
    };
  }, [codeCount, fps, sending, transfer]);

  const finishIfComplete = async () => {
    if (!receiverRef.current.isComplete() || verifyingRef.current) return;
    verifyingRef.current = true;
    try {
      const complete = await receiverRef.current.complete();
      if (complete) {
        setReceivedFile(complete);
        setReceiveProgress((progress) => ({ ...progress, name: complete.name, receivedBytes: complete.bytes.byteLength, fileSize: complete.bytes.byteLength, percent: 100 }));
        setNotice(`接收完成并通过 SHA-256 校验：${complete.name}`);
        stopCamera();
      }
    } catch (error) {
      setNotice(String(error));
    } finally {
      verifyingRef.current = false;
    }
  };

  const handleDecoded = (symbols: OpticalDecodedSymbol[]) => {
    let accepted = 0;
    for (const symbol of symbols) {
      if (receiverRef.current.accept(symbol.text)) accepted += 1;
    }
    if (!accepted) return;
    const progress = receiverRef.current.progress();
    const now = performance.now();
    if (now - lastProgressUpdateRef.current >= 100 || receiverRef.current.isComplete()) {
      setReceiveProgress(progress);
      setNotice(`已接收 ${progress.receivedChunks}/${progress.totalChunks} 个有效帧；修复帧会自动补齐丢失数据。`);
      lastProgressUpdateRef.current = now;
    }
    void finishIfComplete();
  };

  const selectFile = async (file?: File, selectedFrameBytes = frameBytes) => {
    if (!file) return;
    selectedFileRef.current = file;
    setPreparing(true);
    setSending(false);
    try {
      const prepared = await prepareOpticalTransfer(file, selectedFrameBytes);
      setTransfer(prepared);
      const saving = prepared.size - prepared.transmittedSize;
      const compression = prepared.compression === "gzip" ? `gzip 节省 ${formatBytes(Math.max(0, saving))}` : "无需压缩";
      setNotice(`已准备 ${prepared.totalChunks} 个源块（${compression}）；系统帧后会持续发送修复帧，无需等待整轮重播。`);
    } catch (error) {
      setTransfer(null);
      setNotice(`无法准备文件：${String(error)}`);
    } finally {
      setPreparing(false);
    }
  };

  const changeFrameBytes = (value: number) => {
    setFrameBytes(value);
    if (selectedFileRef.current) void selectFile(selectedFileRef.current, value);
  };

  const startCamera = async () => {
    stopCamera();
    setReceivedFile(null);
    receiverRef.current.reset();
    setReceiveProgress(emptyProgress);
    try {
      if (!window.isSecureContext || !navigator.mediaDevices?.getUserMedia) {
        throw new Error("摄像头只允许在 HTTPS 或 localhost 安全页面中使用");
      }
      const stream = await navigator.mediaDevices.getUserMedia({
        video: {
          facingMode: { ideal: "environment" },
          width: { ideal: 1280 },
          height: { ideal: 720 },
          frameRate: { ideal: 30, max: 60 },
        },
        audio: false,
      });
      streamRef.current = stream;
      if (videoRef.current) {
        videoRef.current.srcObject = stream;
        await videoRef.current.play();
      }
      const workerCount = Math.max(1, Math.min(4, Math.floor((navigator.hardwareConcurrency || 4) / 2)));
      decodePoolRef.current = new OpticalDecodePool(workerCount, handleDecoded);
      const detectorConstructor = (window as Window & { BarcodeDetector?: NativeBarcodeDetectorConstructor }).BarcodeDetector;
      if (detectorConstructor) {
        try {
          const formats = await detectorConstructor.getSupportedFormats();
          if (formats.includes("qr_code")) nativeDetectorRef.current = new detectorConstructor({ formats: ["qr_code"] });
        } catch {
          nativeDetectorRef.current = null;
        }
      }
      setScanning(true);
      setNotice(nativeDetectorRef.current
        ? `摄像头已启动，正在使用浏览器原生多码解码器（${workerCount} 个 Worker 待命）。`
        : `摄像头已启动，${workerCount} 个解码 Worker 正在并行工作。`);

      let lastCapture = 0;
      const capture = (time: number) => {
        const video = videoRef.current;
        const canvas = scanCanvasRef.current;
        const pool = decodePoolRef.current;
        if (!pool || !video || !canvas) return;
        if (video.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA && time - lastCapture >= 25 && pool.busyCount < workerCount) {
          lastCapture = time;
          const scale = Math.min(1, 960 / Math.max(video.videoWidth, video.videoHeight));
          const width = Math.max(1, Math.round(video.videoWidth * scale));
          const height = Math.max(1, Math.round(video.videoHeight * scale));
          if (canvas.width !== width || canvas.height !== height) {
            canvas.width = width;
            canvas.height = height;
          }
          const context = canvas.getContext("2d", { willReadFrequently: true });
          context?.drawImage(video, 0, 0, width, height);
          const detector = nativeDetectorRef.current;
          if (detector && !nativeDetectorBusyRef.current) {
            nativeDetectorBusyRef.current = true;
            void detector.detect(canvas).then((barcodes) => {
              handleDecoded(barcodes.map((barcode) => ({
                text: barcode.rawValue,
                box: barcode.boundingBox
                  ? { x: barcode.boundingBox.x, y: barcode.boundingBox.y, width: barcode.boundingBox.width, height: barcode.boundingBox.height }
                  : { x: 0, y: 0, width, height },
              })));
            }).catch(() => {
              nativeDetectorRef.current = null;
            }).finally(() => {
              nativeDetectorBusyRef.current = false;
            });
          } else if (!detector) {
            const image = context?.getImageData(0, 0, width, height);
            if (image) pool.submit(image, 4);
          }
        }
        scanFrameRef.current = window.requestAnimationFrame(capture);
      };
      scanFrameRef.current = window.requestAnimationFrame(capture);
    } catch (error) {
      setNotice(`无法启动摄像头：${String(error)}`);
    }
  };

  const resetReceiver = () => {
    stopCamera();
    receiverRef.current.reset();
    setReceiveProgress(emptyProgress);
    setReceivedFile(null);
    setNotice("接收状态已清空，可以开始扫描新的文件。");
  };

  const saveReceivedFile = async () => {
    if (!receivedFile || saving) return;
    setSaving(true);
    try {
      if (saveFile) {
        const savedPath = await saveFile(receivedFile);
        setNotice(savedPath ? `文件已保存：${savedPath}` : "已取消保存，文件仍保留在当前页面。");
      } else {
        const blob = new Blob([Uint8Array.from(receivedFile.bytes)], { type: receivedFile.mime });
        const url = URL.createObjectURL(blob);
        const anchor = document.createElement("a");
        anchor.href = url;
        anchor.download = receivedFile.name;
        anchor.click();
        URL.revokeObjectURL(url);
        setNotice(`已下载：${receivedFile.name}`);
      }
    } catch (error) {
      setNotice(`保存失败：${String(error)}`);
    } finally {
      setSaving(false);
    }
  };

  const switchMode = (next: "send" | "receive") => {
    if (next === mode) return;
    setSending(false);
    stopCamera();
    setMode(next);
  };

  const theoreticalRate = transfer ? transfer.blockBytes * fps * codeCount : (frameBytes - 30) * fps * codeCount;

  return (
    <div className="page optical-page">
      <header className="page-header optical-header">
        <div><div className="eyebrow">AIR-GAPPED FILE CHANNEL</div><h1>光学文件传输</h1><p>高速二维码流经屏幕与摄像头跨越物理隔离边界，丢帧可自动恢复。</p></div>
        <div className="optical-mode-switch" role="tablist" aria-label="传输方向">
          <button type="button" className={mode === "send" ? "active" : ""} onClick={() => switchMode("send")}><QrCode size={15} /> 发送</button>
          <button type="button" className={mode === "receive" ? "active" : ""} onClick={() => switchMode("receive")}><Camera size={15} /> 接收</button>
        </div>
      </header>

      <div className="optical-security-note"><ShieldAlert size={16} /><span><strong>物理通道不等于加密。</strong> 任何能看到二维码的摄像头都可能接收文件；DRPA 使用单帧 CRC32、容器 CRC32 与文件 SHA-256 三级校验。</span></div>

      {mode === "send" ? (
        <main className="optical-layout send-layout">
          <section className="optical-control-panel">
            <div className="optical-section-title"><RadioTower size={17} /><div><strong>高速发送设置</strong><span>单文件最大 {formatBytes(MAX_OPTICAL_FILE_BYTES)}</span></div></div>
            <label className="optical-dropzone" onDragOver={(event) => event.preventDefault()} onDrop={(event) => { event.preventDefault(); void selectFile(event.dataTransfer.files[0]); }}>
              <input type="file" onChange={(event) => { void selectFile(event.target.files?.[0]); event.currentTarget.value = ""; }} />
              {preparing ? <LoaderCircle className="spin" size={25} /> : <FileUp size={25} />}
              <strong>{preparing ? "正在压缩并计算摘要…" : "选择或拖入文件"}</strong>
              <span>文件只在本机内存中处理，不上传网络</span>
            </label>
            <label className="optical-setting"><span>二维码帧率</span><select value={fps} onChange={(event) => setFps(Number(event.target.value))} disabled={sending}><option value={12}>12 FPS · 远距离</option><option value={24}>24 FPS · 兼容</option><option value={30}>30 FPS · 推荐</option><option value={45}>45 FPS · 高刷屏</option><option value={60}>60 FPS · 实验</option></select></label>
            <label className="optical-setting"><span>单帧容量</span><select value={frameBytes} onChange={(event) => changeFrameBytes(Number(event.target.value))} disabled={sending || preparing}>{OPTICAL_FRAME_BYTE_OPTIONS.map((value) => <option key={value} value={value}>{value} B{value === DEFAULT_OPTICAL_FRAME_BYTES ? " · 推荐" : ""}</option>)}</select></label>
            <label className="optical-setting"><span>同屏二维码</span><select value={codeCount} onChange={(event) => setCodeCount(Number(event.target.value))} disabled={sending}><option value={1}>1 个 · 远距离</option><option value={2}>2 个 · 推荐</option><option value={4}>4 个 · 大屏高速</option></select></label>
            <div className="optical-throughput"><Gauge size={16} /><div><span>理论净载荷</span><strong>{formatRate(theoreticalRate)}</strong></div><small>实际速度取决于屏幕刷新率、距离和摄像头解码率</small></div>
            {transfer && <dl className="optical-file-meta"><div><dt>文件</dt><dd>{transfer.name}</dd></div><div><dt>原始大小</dt><dd>{formatBytes(transfer.size)}</dd></div><div><dt>源块</dt><dd>{transfer.totalChunks} × {formatBytes(transfer.blockBytes)}</dd></div><div><dt>压缩</dt><dd>{transfer.compression === "gzip" ? `${formatBytes(transfer.transmittedSize)} gzip` : "未压缩"}</dd></div><div className="wide"><dt>SHA-256</dt><dd><code>{transfer.sha256}</code></dd></div></dl>}
            <button className={`button ${sending ? "danger" : "primary"} wide`} type="button" disabled={!transfer || preparing} onClick={() => setSending((value) => !value)}>{sending ? <CircleStop size={16} /> : <Play size={16} />}{sending ? "停止发送" : "开始高速发送"}</button>
          </section>
          <section className="optical-stage">
            {transfer ? <><div className={`optical-qr-shell optical-grid-${sending ? codeCount : 1}`}><canvas ref={qrCanvasRef} aria-label="动态光学传输二维码" /></div><div className="optical-stage-status"><span className={sending ? "live" : ""} /><strong>{sending ? "高速发送中" : "已暂停"}</strong><span>序号 {currentFrame.sequence}</span><small>周期位置 {currentFrame.cycle + 1}/{transfer.totalChunks * 2}</small></div></> : <div className="optical-stage-empty"><QrCode size={54} /><h2>二维码将在这里显示</h2><p>普通显示器建议使用 30 FPS、2200 B 和双码；距离较远时降低帧大小或切换单码。</p></div>}
          </section>
        </main>
      ) : (
        <main className="optical-layout receive-layout">
          <section className="optical-camera-stage">
            <video ref={videoRef} muted playsInline aria-label="二维码接收摄像头" />
            <canvas ref={scanCanvasRef} hidden />
            {!scanning && !receivedFile && <div className="optical-camera-empty"><Camera size={48} /><h2>启动摄像头开始接收</h2><p>解码在本机 Worker 中并行运行，不录制、不上传。</p></div>}
            {scanning && <div className="optical-scan-guide"><span /><span /><span /><span /></div>}
            {receivedFile && <div className="optical-received"><CheckCircle2 size={54} /><h2>文件校验完成</h2><p>{receivedFile.name} · {formatBytes(receivedFile.bytes.byteLength)}</p><code>{receivedFile.sha256}</code></div>}
          </section>
          <aside className="optical-control-panel receive-controls">
            <div className="optical-section-title"><Camera size={17} /><div><strong>Fountain 接收进度</strong><span>{receiveProgress.name}</span></div></div>
            <div className="optical-progress"><div><span style={{ width: `${receiveProgress.percent}%` }} /></div><strong>{receiveProgress.percent}%</strong></div>
            <dl className="optical-file-meta"><div><dt>有效帧</dt><dd>{receiveProgress.receivedChunks} / {receiveProgress.totalChunks || "--"}</dd></div><div><dt>净载荷</dt><dd>{formatBytes(receiveProgress.receivedBytes)} / {receiveProgress.fileSize ? formatBytes(receiveProgress.fileSize) : "--"}</dd></div><div className="wide"><dt>会话</dt><dd><code>{receiveProgress.sessionId || "等待二维码"}</code></dd></div></dl>
            {!scanning ? <button className="button primary wide" type="button" onClick={() => void startCamera()} disabled={Boolean(receivedFile)}><Camera size={16} /> 启动摄像头</button> : <button className="button danger wide" type="button" onClick={stopCamera}><CircleStop size={16} /> 停止扫描</button>}
            {receivedFile && <button className="button primary wide" type="button" onClick={() => void saveReceivedFile()} disabled={saving}>{saving ? <LoaderCircle className="spin" size={16} /> : <Save size={16} />}{saving ? "正在保存…" : "保存接收文件"}</button>}
            <button className="button secondary wide" type="button" onClick={resetReceiver}><RefreshCw size={15} /> 清空并重新接收</button>
          </aside>
        </main>
      )}
      <footer className="optical-notice" aria-live="polite">{notice}</footer>
    </div>
  );
}
