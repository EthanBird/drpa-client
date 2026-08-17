export const OPTICAL_PROTOCOL = "DRPAO1";
export const MAX_OPTICAL_FILE_BYTES = 64 * 1024 * 1024;
export const DEFAULT_OPTICAL_CHUNK_BYTES = 480;

export interface OpticalTransfer {
  sessionId: string;
  name: string;
  mime: string;
  size: number;
  sha256: string;
  metadataFrame: string;
  dataFrames: string[];
}

export type OpticalFrame =
  | { kind: "metadata"; sessionId: string; totalChunks: number; fileSize: number; sha256: string; name: string; mime: string }
  | { kind: "data"; sessionId: string; index: number; totalChunks: number; payload: Uint8Array };

export interface OpticalReceiveProgress {
  sessionId: string;
  name: string;
  receivedChunks: number;
  totalChunks: number;
  receivedBytes: number;
  fileSize: number;
  percent: number;
}

export interface OpticalReceivedFile {
  name: string;
  mime: string;
  bytes: Uint8Array;
  sha256: string;
}

const crcTable = new Uint32Array(256).map((_, index) => {
  let value = index;
  for (let bit = 0; bit < 8; bit += 1) value = (value & 1) ? 0xedb88320 ^ (value >>> 1) : value >>> 1;
  return value >>> 0;
});

function crc32(bytes: Uint8Array): string {
  let checksum = 0xffffffff;
  for (const byte of bytes) checksum = crcTable[(checksum ^ byte) & 0xff] ^ (checksum >>> 8);
  return ((checksum ^ 0xffffffff) >>> 0).toString(16).padStart(8, "0");
}

export function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, Math.min(offset + 0x8000, bytes.length)));
  }
  return btoa(binary);
}

export function base64ToBytes(encoded: string): Uint8Array {
  const binary = atob(encoded);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  return bytes;
}

function textToBase64(value: string): string {
  return bytesToBase64(new TextEncoder().encode(value));
}

function base64ToText(value: string): string {
  return new TextDecoder("utf-8", { fatal: true }).decode(base64ToBytes(value));
}

function safeFileName(value: string): string {
  const leaf = value.split(/[\\/]/).at(-1) ?? "";
  return leaf.replace(/[\u0000-\u001f<>:"|?*]/g, "_").trim().slice(0, 180) || "received-file.bin";
}

async function sha256(bytes: Uint8Array): Promise<string> {
  const input = new Uint8Array(bytes.byteLength);
  input.set(bytes);
  const digest = await crypto.subtle.digest("SHA-256", input);
  return Array.from(new Uint8Array(digest), (value) => value.toString(16).padStart(2, "0")).join("");
}

export async function createOpticalTransfer(
  bytes: Uint8Array,
  name: string,
  mime = "application/octet-stream",
  chunkBytes = DEFAULT_OPTICAL_CHUNK_BYTES,
): Promise<OpticalTransfer> {
  if (bytes.byteLength > MAX_OPTICAL_FILE_BYTES) throw new Error("光学传输单文件最大为 64 MB");
  if (!Number.isInteger(chunkBytes) || chunkBytes < 160 || chunkBytes > 960) throw new Error("二维码分片必须在 160–960 字节之间");
  const sessionBytes = crypto.getRandomValues(new Uint8Array(8));
  const sessionId = Array.from(sessionBytes, (value) => value.toString(16).padStart(2, "0")).join("");
  const fileName = safeFileName(name);
  const fileMime = mime.slice(0, 128) || "application/octet-stream";
  const digest = await sha256(bytes);
  const totalChunks = Math.max(1, Math.ceil(bytes.byteLength / chunkBytes));
  const metadataFrame = [OPTICAL_PROTOCOL, "M", sessionId, totalChunks, bytes.byteLength, digest, textToBase64(fileName), textToBase64(fileMime)].join("|");
  const dataFrames = Array.from({ length: totalChunks }, (_, index) => {
    const payload = bytes.slice(index * chunkBytes, Math.min((index + 1) * chunkBytes, bytes.byteLength));
    return [OPTICAL_PROTOCOL, "D", sessionId, index, totalChunks, crc32(payload), bytesToBase64(payload)].join("|");
  });
  return { sessionId, name: fileName, mime: fileMime, size: bytes.byteLength, sha256: digest, metadataFrame, dataFrames };
}

export async function prepareOpticalTransfer(file: File, chunkBytes = DEFAULT_OPTICAL_CHUNK_BYTES): Promise<OpticalTransfer> {
  return createOpticalTransfer(new Uint8Array(await file.arrayBuffer()), file.name, file.type, chunkBytes);
}

export function createOpticalCarousel(transfer: OpticalTransfer, metadataInterval = 12): string[] {
  const frames: string[] = [];
  transfer.dataFrames.forEach((frame, index) => {
    if (index % metadataInterval === 0) frames.push(transfer.metadataFrame);
    frames.push(frame);
  });
  return frames;
}

export function decodeOpticalFrame(value: string): OpticalFrame | null {
  if (!value.startsWith(`${OPTICAL_PROTOCOL}|`)) return null;
  const parts = value.split("|");
  try {
    if (parts[1] === "M" && parts.length === 8) {
      const totalChunks = Number(parts[3]);
      const fileSize = Number(parts[4]);
      if (!/^[a-f0-9]{16}$/.test(parts[2]) || !Number.isInteger(totalChunks) || totalChunks < 1 || totalChunks > 300_000) return null;
      if (!Number.isInteger(fileSize) || fileSize < 0 || fileSize > MAX_OPTICAL_FILE_BYTES || !/^[a-f0-9]{64}$/.test(parts[5])) return null;
      return {
        kind: "metadata",
        sessionId: parts[2],
        totalChunks,
        fileSize,
        sha256: parts[5],
        name: safeFileName(base64ToText(parts[6])),
        mime: base64ToText(parts[7]).slice(0, 128) || "application/octet-stream",
      };
    }
    if (parts[1] === "D" && parts.length === 7) {
      const index = Number(parts[3]);
      const totalChunks = Number(parts[4]);
      const payload = base64ToBytes(parts[6]);
      if (!/^[a-f0-9]{16}$/.test(parts[2]) || !Number.isInteger(totalChunks) || totalChunks < 1 || totalChunks > 300_000) return null;
      if (!Number.isInteger(index) || index < 0 || index >= totalChunks || payload.byteLength > 960 || crc32(payload) !== parts[5]) return null;
      return { kind: "data", sessionId: parts[2], index, totalChunks, payload };
    }
  } catch {
    return null;
  }
  return null;
}

export class OpticalReceiver {
  private sessionId = "";
  private totalChunks = 0;
  private metadata: Extract<OpticalFrame, { kind: "metadata" }> | null = null;
  private readonly chunks = new Map<number, Uint8Array>();

  accept(encoded: string): boolean {
    const frame = decodeOpticalFrame(encoded);
    if (!frame) return false;
    if (this.sessionId && this.sessionId !== frame.sessionId) {
      if (frame.kind !== "metadata") return false;
      this.reset();
    }
    if (!this.sessionId) {
      this.sessionId = frame.sessionId;
      this.totalChunks = frame.totalChunks;
    }
    if (frame.totalChunks !== this.totalChunks) return false;
    if (frame.kind === "metadata") {
      this.metadata = frame;
      return true;
    }
    if (this.chunks.has(frame.index)) return false;
    this.chunks.set(frame.index, frame.payload);
    return true;
  }

  progress(): OpticalReceiveProgress {
    const receivedBytes = Array.from(this.chunks.values()).reduce((sum, chunk) => sum + chunk.byteLength, 0);
    return {
      sessionId: this.sessionId,
      name: this.metadata?.name ?? "正在等待文件信息…",
      receivedChunks: this.chunks.size,
      totalChunks: this.totalChunks,
      receivedBytes,
      fileSize: this.metadata?.fileSize ?? 0,
      percent: this.totalChunks ? Math.min(100, Math.round(this.chunks.size / this.totalChunks * 100)) : 0,
    };
  }

  async complete(): Promise<OpticalReceivedFile | null> {
    if (!this.metadata || this.chunks.size !== this.totalChunks) return null;
    const bytes = new Uint8Array(this.metadata.fileSize);
    let offset = 0;
    for (let index = 0; index < this.totalChunks; index += 1) {
      const chunk = this.chunks.get(index);
      if (!chunk || offset + chunk.byteLength > bytes.byteLength) return null;
      bytes.set(chunk, offset);
      offset += chunk.byteLength;
    }
    if (offset !== bytes.byteLength || await sha256(bytes) !== this.metadata.sha256) throw new Error("文件校验失败，请继续扫描或重新发送");
    return { name: this.metadata.name, mime: this.metadata.mime, bytes, sha256: this.metadata.sha256 };
  }

  reset(): void {
    this.sessionId = "";
    this.totalChunks = 0;
    this.metadata = null;
    this.chunks.clear();
  }
}
