import { Camera, CheckCircle2, CircleStop, FileUp, LoaderCircle, Play, QrCode, RadioTower, RefreshCw, Save, ShieldAlert } from "lucide-react";
import jsQR from "jsqr";
import QRCode from "qrcode";
import { useEffect, useMemo, useRef, useState } from "react";

import {
  createOpticalCarousel,
  DEFAULT_OPTICAL_CHUNK_BYTES,
  MAX_OPTICAL_FILE_BYTES,
  OpticalReceiver,
  prepareOpticalTransfer,
  type OpticalReceiveProgress,
  type OpticalReceivedFile,
  type OpticalTransfer,
} from "../features/optical/opticalProtocol";

export type OpticalFileSaver = (file: OpticalReceivedFile) => Promise<string | null>;

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

export function OpticalTransferPage({ saveFile }: { saveFile?: OpticalFileSaver } = {}) {
  const [mode, setMode] = useState<"send" | "receive">("send");
  const [transfer, setTransfer] = useState<OpticalTransfer | null>(null);
  const [preparing, setPreparing] = useState(false);
  const [sending, setSending] = useState(false);
  const [fps, setFps] = useState(8);
  const [chunkBytes, setChunkBytes] = useState(DEFAULT_OPTICAL_CHUNK_BYTES);
  const [currentFrame, setCurrentFrame] = useState({ cursor: 0, total: 0, dataIndex: -1 });
  const [notice, setNotice] = useState("选择文件后，DRPA 会生成一组循环播放的二维码。");
  const [scanning, setScanning] = useState(false);
  const [receiveProgress, setReceiveProgress] = useState<OpticalReceiveProgress>(emptyProgress);
  const [receivedFile, setReceivedFile] = useState<OpticalReceivedFile | null>(null);
  const [saving, setSaving] = useState(false);
  const qrCanvasRef = useRef<HTMLCanvasElement>(null);
  const videoRef = useRef<HTMLVideoElement>(null);
  const scanCanvasRef = useRef<HTMLCanvasElement>(null);
  const streamRef = useRef<MediaStream | null>(null);
  const receiverRef = useRef(new OpticalReceiver());
  const verifyingRef = useRef(false);
  const carousel = useMemo(() => transfer ? createOpticalCarousel(transfer) : [], [transfer]);

  const stopCamera = () => {
    streamRef.current?.getTracks().forEach((track) => track.stop());
    streamRef.current = null;
    if (videoRef.current) videoRef.current.srcObject = null;
    setScanning(false);
  };

  useEffect(() => () => stopCamera(), []);

  useEffect(() => {
    if (!transfer || !qrCanvasRef.current) return;
    let disposed = false;
    let timer = 0;
    let cursor = 0;
    const render = async () => {
      if (disposed || !qrCanvasRef.current) return;
      const encoded = sending ? carousel[cursor] : transfer.metadataFrame;
      await QRCode.toCanvas(qrCanvasRef.current, encoded, {
        width: 520,
        margin: 2,
        errorCorrectionLevel: "L",
        color: { dark: "#0d1b3b", light: "#ffffff" },
      });
      if (disposed) return;
      const dataIndex = encoded.includes("|D|") ? Number(encoded.split("|", 5)[3]) : -1;
      setCurrentFrame({ cursor: sending ? cursor + 1 : 0, total: carousel.length, dataIndex });
      if (sending) {
        cursor = (cursor + 1) % carousel.length;
        timer = window.setTimeout(() => { void render(); }, Math.round(1000 / fps));
      }
    };
    void render().catch((error) => setNotice(`二维码生成失败：${String(error)}`));
    return () => {
      disposed = true;
      window.clearTimeout(timer);
    };
  }, [carousel, fps, sending, transfer]);

  useEffect(() => {
    if (!scanning) return;
    let disposed = false;
    let frame = 0;
    let lastScan = 0;
    const scan = async (time: number) => {
      if (disposed) return;
      const video = videoRef.current;
      const canvas = scanCanvasRef.current;
      if (video && canvas && video.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA && time - lastScan >= 90) {
        lastScan = time;
        const scale = Math.min(1, 960 / Math.max(video.videoWidth, video.videoHeight));
        canvas.width = Math.max(1, Math.round(video.videoWidth * scale));
        canvas.height = Math.max(1, Math.round(video.videoHeight * scale));
        const context = canvas.getContext("2d", { willReadFrequently: true });
        context?.drawImage(video, 0, 0, canvas.width, canvas.height);
        const image = context?.getImageData(0, 0, canvas.width, canvas.height);
        const code = image ? jsQR(image.data, image.width, image.height, { inversionAttempts: "dontInvert" }) : null;
        if (code && receiverRef.current.accept(code.data)) {
          const progress = receiverRef.current.progress();
          setReceiveProgress(progress);
          setNotice(`已识别 ${progress.receivedChunks}/${progress.totalChunks} 个数据分片`);
          if (progress.totalChunks > 0 && progress.receivedChunks === progress.totalChunks && !verifyingRef.current) {
            verifyingRef.current = true;
            try {
              const complete = await receiverRef.current.complete();
              if (complete) {
                setReceivedFile(complete);
                setNotice(`接收完成并通过 SHA-256 校验：${complete.name}`);
                stopCamera();
                return;
              }
            } catch (error) {
              setNotice(String(error));
            } finally {
              verifyingRef.current = false;
            }
          }
        }
      }
      frame = window.requestAnimationFrame((nextTime) => { void scan(nextTime); });
    };
    frame = window.requestAnimationFrame((time) => { void scan(time); });
    return () => {
      disposed = true;
      window.cancelAnimationFrame(frame);
    };
  }, [scanning]);

  const selectFile = async (file?: File) => {
    if (!file) return;
    setPreparing(true);
    setSending(false);
    try {
      const prepared = await prepareOpticalTransfer(file, chunkBytes);
      setTransfer(prepared);
      setNotice(`已生成 ${prepared.dataFrames.length} 个数据分片；接收端可以中途加入，元数据会周期重播。`);
    } catch (error) {
      setNotice(`无法准备文件：${String(error)}`);
    } finally {
      setPreparing(false);
    }
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
      const stream = await navigator.mediaDevices.getUserMedia({ video: { facingMode: { ideal: "environment" }, width: { ideal: 1280 }, height: { ideal: 720 } }, audio: false });
      streamRef.current = stream;
      if (videoRef.current) {
        videoRef.current.srcObject = stream;
        await videoRef.current.play();
      }
      setScanning(true);
      setNotice("摄像头已启动，请让发送端二维码完整进入取景框。");
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
        if (!savedPath) {
          setNotice("已取消保存，接收到的文件仍保留在当前页面。");
          return;
        }
        setNotice(`文件已保存：${savedPath}`);
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

  return (
    <div className="page optical-page">
      <header className="page-header optical-header">
        <div><div className="eyebrow">AIR-GAPPED FILE CHANNEL</div><h1>光学文件传输</h1><p>把文件编码为动态二维码，经屏幕与摄像头跨越物理隔离边界。</p></div>
        <div className="optical-mode-switch" role="tablist" aria-label="传输方向">
          <button type="button" className={mode === "send" ? "active" : ""} onClick={() => switchMode("send")}><QrCode size={15} /> 发送</button>
          <button type="button" className={mode === "receive" ? "active" : ""} onClick={() => switchMode("receive")}><Camera size={15} /> 接收</button>
        </div>
      </header>

      <div className="optical-security-note"><ShieldAlert size={16} /><span><strong>物理通道不等于加密。</strong> 任何能看到二维码的摄像头都可能接收文件；DRPA 使用 CRC32 检测分片错误，并用 SHA-256 校验完整文件。</span></div>

      {mode === "send" ? (
        <main className="optical-layout send-layout">
          <section className="optical-control-panel">
            <div className="optical-section-title"><RadioTower size={17} /><div><strong>发送设置</strong><span>单文件最大 {formatBytes(MAX_OPTICAL_FILE_BYTES)}</span></div></div>
            <label className="optical-dropzone" onDragOver={(event) => event.preventDefault()} onDrop={(event) => { event.preventDefault(); void selectFile(event.dataTransfer.files[0]); }}>
              <input type="file" onChange={(event) => { void selectFile(event.target.files?.[0]); event.currentTarget.value = ""; }} />
              {preparing ? <LoaderCircle className="spin" size={25} /> : <FileUp size={25} />}
              <strong>{preparing ? "正在计算文件摘要…" : "选择或拖入文件"}</strong>
              <span>文件只在本机内存中分片，不上传网络</span>
            </label>
            <label className="optical-setting"><span>二维码帧率</span><select value={fps} onChange={(event) => setFps(Number(event.target.value))} disabled={sending}><option value={4}>4 FPS · 兼容</option><option value={8}>8 FPS · 推荐</option><option value={12}>12 FPS · 快速</option></select></label>
            <label className="optical-setting"><span>单帧数据量</span><select value={chunkBytes} onChange={(event) => setChunkBytes(Number(event.target.value))} disabled={sending}><option value={320}>320 B · 远距离</option><option value={480}>480 B · 推荐</option><option value={720}>720 B · 近距离</option></select></label>
            {transfer && <dl className="optical-file-meta"><div><dt>文件</dt><dd>{transfer.name}</dd></div><div><dt>大小</dt><dd>{formatBytes(transfer.size)}</dd></div><div><dt>数据帧</dt><dd>{transfer.dataFrames.length}</dd></div><div><dt>预计单轮</dt><dd>{Math.ceil(carousel.length / fps)} 秒</dd></div><div className="wide"><dt>SHA-256</dt><dd><code>{transfer.sha256}</code></dd></div></dl>}
            <button className={sending ? "button danger wide" : "button primary wide"} type="button" disabled={!transfer} onClick={() => setSending((current) => !current)}>{sending ? <><CircleStop size={16} /> 停止播放</> : <><Play size={16} fill="currentColor" /> 开始循环发送</>}</button>
          </section>
          <section className="optical-stage">
            {transfer ? <><div className="optical-qr-shell"><canvas ref={qrCanvasRef} aria-label="光学传输二维码" /></div><div className="optical-stage-status"><span className={sending ? "live" : ""} /> <strong>{sending ? "正在发送" : "已暂停"}</strong><span>{currentFrame.dataIndex >= 0 ? `数据帧 ${currentFrame.dataIndex + 1}/${transfer.dataFrames.length}` : "文件信息帧"}</span><small>轮播位置 {currentFrame.cursor}/{currentFrame.total}</small></div></> : <div className="optical-stage-empty"><QrCode size={54} /><h2>二维码将在这里显示</h2><p>建议两块屏幕保持正对，关闭反光并将接收摄像头对准完整二维码。</p></div>}
          </section>
        </main>
      ) : (
        <main className="optical-layout receive-layout">
          <section className="optical-camera-stage">
            <video ref={videoRef} muted playsInline aria-label="二维码接收摄像头" />
            <canvas ref={scanCanvasRef} hidden />
            {!scanning && !receivedFile && <div className="optical-camera-empty"><Camera size={48} /><h2>启动摄像头开始接收</h2><p>浏览器权限只用于当前取景，不录制、不上传。</p></div>}
            {scanning && <div className="optical-scan-guide"><span /><span /><span /><span /></div>}
            {receivedFile && <div className="optical-received"><CheckCircle2 size={54} /><h2>文件校验完成</h2><p>{receivedFile.name} · {formatBytes(receivedFile.bytes.byteLength)}</p><code>{receivedFile.sha256}</code></div>}
          </section>
          <aside className="optical-control-panel receive-controls">
            <div className="optical-section-title"><Camera size={17} /><div><strong>接收进度</strong><span>{receiveProgress.name}</span></div></div>
            <div className="optical-progress"><div><span style={{ width: `${receiveProgress.percent}%` }} /></div><strong>{receiveProgress.percent}%</strong></div>
            <dl className="optical-file-meta"><div><dt>已接收</dt><dd>{receiveProgress.receivedChunks} / {receiveProgress.totalChunks || "--"} 帧</dd></div><div><dt>数据量</dt><dd>{formatBytes(receiveProgress.receivedBytes)} / {receiveProgress.fileSize ? formatBytes(receiveProgress.fileSize) : "--"}</dd></div><div className="wide"><dt>会话</dt><dd><code>{receiveProgress.sessionId || "等待二维码"}</code></dd></div></dl>
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
