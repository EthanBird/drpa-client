export const OPTICAL_PROTOCOL = "DRPA2";
export const OPTICAL_WIRE_PREFIX = "DRPA2:";
export const MAX_OPTICAL_FILE_BYTES = 64 * 1024 * 1024;
export const MIN_OPTICAL_FRAME_BYTES = 480;
export const MAX_OPTICAL_FRAME_BYTES = 2953;
export const DEFAULT_OPTICAL_FRAME_BYTES = 1465;
export const OPTICAL_FRAME_BYTE_OPTIONS = [900, DEFAULT_OPTICAL_FRAME_BYTES, 1850, 2331, MAX_OPTICAL_FRAME_BYTES] as const;

const FRAME_MAGIC = new Uint8Array([0x44, 0x52, 0x50, 0x41]); // DRPA
const FRAME_VERSION = 2;
const FRAME_HEADER_BYTES = 30;
const CONTAINER_MAGIC = new Uint8Array([0x44, 0x52, 0x46, 0x32]); // DRF2
const CONTAINER_HEADER_BYTES = 49;
const CONTAINER_FLAG_GZIP = 1;
const MAX_SOURCE_BLOCKS = 0xffff;
const BASE45_ALPHABET = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";
const BASE45_VALUES = new Int16Array(128).fill(-1);

for (let index = 0; index < BASE45_ALPHABET.length; index += 1) {
  BASE45_VALUES[BASE45_ALPHABET.charCodeAt(index)] = index;
}

export interface OpticalTransfer {
  sessionId: string;
  name: string;
  mime: string;
  size: number;
  transmittedSize: number;
  compression: "none" | "gzip";
  sha256: string;
  frameBytes: number;
  blockBytes: number;
  totalChunks: number;
  createFrame(sequence: number): Uint8Array;
  createFrameText(sequence: number): string;
}

export interface OpticalReceiveProgress {
  sessionId: string;
  name: string;
  acceptedFrames: number;
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

interface ParsedFrame {
  sessionId: number;
  sequence: number;
  sourceBlocks: number;
  blockBytes: number;
  totalBytes: number;
  containerCrc: number;
  block: Uint8Array;
}

interface PackedContainer {
  bytes: Uint8Array;
  digest: Uint8Array;
  compression: "none" | "gzip";
  transmittedSize: number;
}

const crcTable = new Uint32Array(256).map((_, index) => {
  let value = index;
  for (let bit = 0; bit < 8; bit += 1) value = (value & 1) ? 0xedb88320 ^ (value >>> 1) : value >>> 1;
  return value >>> 0;
});

function crc32(bytes: Uint8Array): number {
  let checksum = 0xffffffff;
  for (const byte of bytes) checksum = crcTable[(checksum ^ byte) & 0xff]! ^ (checksum >>> 8);
  return (checksum ^ 0xffffffff) >>> 0;
}

function safeFileName(value: string): string {
  const leaf = value.split(/[\\/]/).at(-1) ?? "";
  return leaf.replace(/[\u0000-\u001f<>:"|?*]/g, "_").trim().slice(0, 180) || "received-file.bin";
}

function digestHex(bytes: Uint8Array): string {
  return Array.from(bytes, (value) => value.toString(16).padStart(2, "0")).join("");
}

export function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let offset = 0; offset < bytes.byteLength; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, Math.min(offset + 0x8000, bytes.byteLength)));
  }
  return btoa(binary);
}

async function sha256(bytes: Uint8Array): Promise<Uint8Array> {
  const stable = new Uint8Array(bytes.byteLength);
  stable.set(bytes);
  return new Uint8Array(await crypto.subtle.digest("SHA-256", stable));
}

function isAlreadyCompressed(mime: string): boolean {
  return /^(image\/(?:avif|gif|heic|jpeg|png|webp)|video\/|audio\/(?:aac|flac|mpeg|ogg|opus)|application\/(?:gzip|pdf|vnd\.rar|x-7z-compressed|zip|zstd))/.test(mime);
}

async function gzip(bytes: Uint8Array): Promise<Uint8Array | null> {
  if (typeof CompressionStream === "undefined" || typeof Blob.prototype.stream !== "function") return null;
  const stream = new Blob([Uint8Array.from(bytes)]).stream().pipeThrough(new CompressionStream("gzip"));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

async function gunzip(bytes: Uint8Array, expectedBytes: number): Promise<Uint8Array> {
  if (typeof DecompressionStream === "undefined" || typeof Blob.prototype.stream !== "function") throw new Error("当前浏览器不支持 gzip 解压，请关闭发送端压缩后重试");
  const reader = new Blob([Uint8Array.from(bytes)]).stream().pipeThrough(new DecompressionStream("gzip")).getReader();
  const chunks: Uint8Array[] = [];
  let length = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    length += value.byteLength;
    if (length > expectedBytes || length > MAX_OPTICAL_FILE_BYTES) {
      await reader.cancel();
      throw new Error("压缩数据展开后超过声明大小");
    }
    chunks.push(value);
  }
  if (length !== expectedBytes) throw new Error("压缩数据长度校验失败");
  const output = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    output.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return output;
}

async function packContainer(bytes: Uint8Array, name: string, mime: string): Promise<PackedContainer> {
  const encoder = new TextEncoder();
  const nameBytes = encoder.encode(safeFileName(name));
  const mimeBytes = encoder.encode(mime.slice(0, 128) || "application/octet-stream");
  const shouldTryCompression = bytes.byteLength >= 768 && !isAlreadyCompressed(mime);
  const [digest, compressed] = await Promise.all([
    sha256(bytes),
    shouldTryCompression ? gzip(bytes) : Promise.resolve(null),
  ]);
  const useGzip = compressed !== null && compressed.byteLength + 64 < bytes.byteLength;
  const payload = useGzip ? compressed : bytes;
  const output = new Uint8Array(CONTAINER_HEADER_BYTES + nameBytes.byteLength + mimeBytes.byteLength + payload.byteLength);
  const view = new DataView(output.buffer);
  output.set(CONTAINER_MAGIC, 0);
  view.setUint8(4, useGzip ? CONTAINER_FLAG_GZIP : 0);
  view.setUint16(5, nameBytes.byteLength, true);
  view.setUint16(7, mimeBytes.byteLength, true);
  view.setUint32(9, bytes.byteLength, true);
  view.setUint32(13, payload.byteLength, true);
  output.set(digest, 17);
  output.set(nameBytes, CONTAINER_HEADER_BYTES);
  output.set(mimeBytes, CONTAINER_HEADER_BYTES + nameBytes.byteLength);
  output.set(payload, CONTAINER_HEADER_BYTES + nameBytes.byteLength + mimeBytes.byteLength);
  return { bytes: output, digest, compression: useGzip ? "gzip" : "none", transmittedSize: payload.byteLength };
}

async function unpackContainer(container: Uint8Array): Promise<OpticalReceivedFile> {
  if (container.byteLength < CONTAINER_HEADER_BYTES) throw new Error("文件容器不完整");
  if (!CONTAINER_MAGIC.every((value, index) => container[index] === value)) throw new Error("文件容器版本不受支持");
  const view = new DataView(container.buffer, container.byteOffset, container.byteLength);
  const flags = view.getUint8(4);
  if (flags & ~CONTAINER_FLAG_GZIP) throw new Error("文件容器包含未知特性");
  const nameBytes = view.getUint16(5, true);
  const mimeBytes = view.getUint16(7, true);
  const originalBytes = view.getUint32(9, true);
  const transmittedBytes = view.getUint32(13, true);
  const dataOffset = CONTAINER_HEADER_BYTES + nameBytes + mimeBytes;
  if (originalBytes > MAX_OPTICAL_FILE_BYTES || dataOffset + transmittedBytes !== container.byteLength) throw new Error("文件容器长度校验失败");
  const decoder = new TextDecoder("utf-8", { fatal: true });
  const name = safeFileName(decoder.decode(container.subarray(CONTAINER_HEADER_BYTES, CONTAINER_HEADER_BYTES + nameBytes)));
  const mime = decoder.decode(container.subarray(CONTAINER_HEADER_BYTES + nameBytes, dataOffset)).slice(0, 128) || "application/octet-stream";
  const transmitted = container.slice(dataOffset);
  const bytes = flags & CONTAINER_FLAG_GZIP ? await gunzip(transmitted, originalBytes) : transmitted;
  if (bytes.byteLength !== originalBytes) throw new Error("文件长度校验失败");
  const expectedDigest = container.subarray(17, 49);
  const actualDigest = await sha256(bytes);
  if (!expectedDigest.every((value, index) => actualDigest[index] === value)) throw new Error("文件 SHA-256 校验失败");
  return { name, mime, bytes, sha256: digestHex(actualDigest) };
}

export function encodeBase45(bytes: Uint8Array): string {
  let output = "";
  for (let offset = 0; offset < bytes.byteLength; offset += 2) {
    if (offset + 1 < bytes.byteLength) {
      const value = bytes[offset]! * 256 + bytes[offset + 1]!;
      output += BASE45_ALPHABET[value % 45]!;
      output += BASE45_ALPHABET[Math.floor(value / 45) % 45]!;
      output += BASE45_ALPHABET[Math.floor(value / 2025)]!;
    } else {
      const value = bytes[offset]!;
      output += BASE45_ALPHABET[value % 45]!;
      output += BASE45_ALPHABET[Math.floor(value / 45)]!;
    }
  }
  return output;
}

export function decodeBase45(encoded: string): Uint8Array {
  if (encoded.length % 3 === 1) throw new Error("Base45 长度无效");
  const output = new Uint8Array(Math.floor(encoded.length / 3) * 2 + (encoded.length % 3 === 2 ? 1 : 0));
  let input = 0;
  let offset = 0;
  while (input < encoded.length) {
    const first = encoded.charCodeAt(input);
    const second = encoded.charCodeAt(input + 1);
    const a = first < BASE45_VALUES.length ? BASE45_VALUES[first]! : -1;
    const b = second < BASE45_VALUES.length ? BASE45_VALUES[second]! : -1;
    if (a < 0 || b < 0) throw new Error("Base45 字符无效");
    if (input + 2 < encoded.length) {
      const third = encoded.charCodeAt(input + 2);
      const c = third < BASE45_VALUES.length ? BASE45_VALUES[third]! : -1;
      const value = a + b * 45 + c * 2025;
      if (c < 0 || value > 0xffff) throw new Error("Base45 数值无效");
      output[offset] = value >>> 8;
      output[offset + 1] = value & 0xff;
      input += 3;
      offset += 2;
    } else {
      const value = a + b * 45;
      if (value > 0xff) throw new Error("Base45 数值无效");
      output[offset] = value;
      input += 2;
      offset += 1;
    }
  }
  return output;
}

function splitmix32(seed: number): () => number {
  let state = seed | 0;
  return () => {
    state = (state + 0x9e3779b9) | 0;
    let value = state;
    value = Math.imul(value ^ (value >>> 16), 0x21f0aaad);
    value = Math.imul(value ^ (value >>> 15), 0x735a2d97);
    return (value ^ (value >>> 15)) >>> 0;
  };
}

function repairIndices(sourceBlocks: number, sessionId: number, sequence: number): number[] {
  const random = splitmix32(Math.imul(sessionId ^ sequence, 0x9e3779b1) ^ (sequence >>> 1));
  const maximumDegree = Math.min(sourceBlocks, 24);
  const minimumDegree = Math.min(sourceBlocks, 4);
  const degree = minimumDegree + (random() % Math.max(1, maximumDegree - minimumDegree + 1));
  const indices = new Set<number>();
  while (indices.size < degree) indices.add(random() % sourceBlocks);
  return [...indices];
}

export function opticalFrameComposition(sourceBlocks: number, sessionId: number, sequence: number): number[] {
  const position = sequence % (sourceBlocks * 2);
  return position < sourceBlocks ? [position] : repairIndices(sourceBlocks, sessionId, sequence);
}

function packFrame(
  sessionId: number,
  sequence: number,
  sourceBlocks: number,
  blockBytes: number,
  totalBytes: number,
  containerCrc: number,
  block: Uint8Array,
): Uint8Array {
  const frame = new Uint8Array(FRAME_HEADER_BYTES + blockBytes);
  const view = new DataView(frame.buffer);
  frame.set(FRAME_MAGIC, 0);
  view.setUint8(4, FRAME_VERSION);
  view.setUint8(5, 0);
  view.setUint32(6, sessionId, true);
  view.setUint32(10, sequence, true);
  view.setUint16(14, sourceBlocks, true);
  view.setUint16(16, blockBytes, true);
  view.setUint32(18, totalBytes, true);
  view.setUint32(22, containerCrc, true);
  view.setUint32(26, crc32(block), true);
  frame.set(block, FRAME_HEADER_BYTES);
  return frame;
}

function parseFrame(frame: Uint8Array): ParsedFrame | null {
  if (frame.byteLength < FRAME_HEADER_BYTES || !FRAME_MAGIC.every((value, index) => frame[index] === value)) return null;
  const view = new DataView(frame.buffer, frame.byteOffset, frame.byteLength);
  if (view.getUint8(4) !== FRAME_VERSION || view.getUint8(5) !== 0) return null;
  const sourceBlocks = view.getUint16(14, true);
  const blockBytes = view.getUint16(16, true);
  const totalBytes = view.getUint32(18, true);
  if (!sourceBlocks || !blockBytes || blockBytes > MAX_OPTICAL_FRAME_BYTES - FRAME_HEADER_BYTES) return null;
  if (!totalBytes || totalBytes > MAX_OPTICAL_FILE_BYTES + 1024 || frame.byteLength !== FRAME_HEADER_BYTES + blockBytes) return null;
  const block = frame.slice(FRAME_HEADER_BYTES);
  if (crc32(block) !== view.getUint32(26, true)) return null;
  return {
    sessionId: view.getUint32(6, true),
    sequence: view.getUint32(10, true),
    sourceBlocks,
    blockBytes,
    totalBytes,
    containerCrc: view.getUint32(22, true),
    block,
  };
}

export function decodeOpticalFrameText(value: string): Uint8Array | null {
  if (!value.startsWith(OPTICAL_WIRE_PREFIX)) return null;
  try {
    return decodeBase45(value.slice(OPTICAL_WIRE_PREFIX.length));
  } catch {
    return null;
  }
}

export function isOpticalFrame(encoded: string | Uint8Array): boolean {
  const bytes = typeof encoded === "string" ? decodeOpticalFrameText(encoded) : encoded;
  return Boolean(bytes && parseFrame(bytes));
}

export async function createOpticalTransfer(
  bytes: Uint8Array,
  name: string,
  mime = "application/octet-stream",
  frameBytes = DEFAULT_OPTICAL_FRAME_BYTES,
): Promise<OpticalTransfer> {
  if (!bytes.byteLength) throw new Error("不能发送空文件");
  if (bytes.byteLength > MAX_OPTICAL_FILE_BYTES) throw new Error("光学传输单文件最大为 64 MB");
  if (!Number.isInteger(frameBytes) || frameBytes < MIN_OPTICAL_FRAME_BYTES || frameBytes > MAX_OPTICAL_FRAME_BYTES) {
    throw new Error(`二维码帧必须在 ${MIN_OPTICAL_FRAME_BYTES}–${MAX_OPTICAL_FRAME_BYTES} 字节之间`);
  }
  const fileName = safeFileName(name);
  const fileMime = mime.slice(0, 128) || "application/octet-stream";
  const packed = await packContainer(bytes, fileName, fileMime);
  const blockBytes = frameBytes - FRAME_HEADER_BYTES;
  const sourceBlocks = Math.ceil(packed.bytes.byteLength / blockBytes);
  if (sourceBlocks > MAX_SOURCE_BLOCKS) throw new Error("文件分块数量超限，请提高单帧容量");
  const sessionId = new DataView(crypto.getRandomValues(new Uint8Array(4)).buffer).getUint32(0, true) || 1;
  const checksum = crc32(packed.bytes);
  const source = new Uint8Array(sourceBlocks * blockBytes);
  source.set(packed.bytes);

  const createFrame = (sequence: number): Uint8Array => {
    if (!Number.isSafeInteger(sequence) || sequence < 0 || sequence > 0xffffffff) throw new Error("光学帧序号无效");
    const block = new Uint8Array(blockBytes);
    for (const index of opticalFrameComposition(sourceBlocks, sessionId, sequence)) {
      const offset = index * blockBytes;
      for (let byte = 0; byte < blockBytes; byte += 1) block[byte] ^= source[offset + byte]!;
    }
    return packFrame(sessionId, sequence, sourceBlocks, blockBytes, packed.bytes.byteLength, checksum, block);
  };

  return {
    sessionId: sessionId.toString(16).padStart(8, "0"),
    name: fileName,
    mime: fileMime,
    size: bytes.byteLength,
    transmittedSize: packed.transmittedSize,
    compression: packed.compression,
    sha256: digestHex(packed.digest),
    frameBytes,
    blockBytes,
    totalChunks: sourceBlocks,
    createFrame,
    createFrameText: (sequence) => `${OPTICAL_WIRE_PREFIX}${encodeBase45(createFrame(sequence))}`,
  };
}

export async function prepareOpticalTransfer(file: File, frameBytes = DEFAULT_OPTICAL_FRAME_BYTES): Promise<OpticalTransfer> {
  return createOpticalTransfer(new Uint8Array(await file.arrayBuffer()), file.name, file.type, frameBytes);
}

interface PendingEquation {
  indices: Set<number>;
  bytes: Uint8Array;
}

class FountainDecoder {
  private readonly solved: (Uint8Array | null)[];
  private readonly pendingByBlock = new Map<number, Set<PendingEquation>>();
  private readonly pendingEquations = new Set<PendingEquation>();
  private readonly seen = new Set<number>();
  private lastGaussianFrameCount = 0;
  readonly sourceBlocks: number;
  readonly blockBytes: number;
  readonly sessionId: number;
  readonly totalBytes: number;
  solvedCount = 0;

  constructor(
    sourceBlocks: number,
    blockBytes: number,
    sessionId: number,
    totalBytes: number,
  ) {
    this.sourceBlocks = sourceBlocks;
    this.blockBytes = blockBytes;
    this.sessionId = sessionId;
    this.totalBytes = totalBytes;
    this.solved = new Array<Uint8Array | null>(sourceBlocks).fill(null);
  }

  get uniqueFrames(): number {
    return this.seen.size;
  }

  get complete(): boolean {
    return this.solvedCount === this.sourceBlocks;
  }

  add(sequence: number, block: Uint8Array): boolean {
    if (this.seen.has(sequence)) return false;
    this.seen.add(sequence);
    if (this.complete) return true;
    const indices = new Set(opticalFrameComposition(this.sourceBlocks, this.sessionId, sequence));
    const reduced = block.slice();
    for (const index of [...indices]) {
      const known = this.solved[index];
      if (!known) continue;
      for (let byte = 0; byte < reduced.byteLength; byte += 1) reduced[byte] ^= known[byte]!;
      indices.delete(index);
    }
    if (!indices.size) return true;
    if (indices.size === 1) {
      this.resolve(indices.values().next().value!, reduced);
      this.maybeResolveDenseTail();
      return true;
    }
    const equation: PendingEquation = { indices, bytes: reduced };
    this.pendingEquations.add(equation);
    for (const index of indices) {
      const equations = this.pendingByBlock.get(index) ?? new Set<PendingEquation>();
      equations.add(equation);
      this.pendingByBlock.set(index, equations);
    }
    this.maybeResolveDenseTail();
    return true;
  }

  assemble(): Uint8Array | null {
    if (!this.complete) return null;
    const output = new Uint8Array(this.totalBytes);
    for (let index = 0; index < this.sourceBlocks; index += 1) {
      const offset = index * this.blockBytes;
      output.set(this.solved[index]!.subarray(0, Math.min(this.blockBytes, this.totalBytes - offset)), offset);
    }
    return output;
  }

  private resolve(index: number, bytes: Uint8Array): void {
    const queue: Array<[number, Uint8Array]> = [[index, bytes]];
    while (queue.length) {
      const [resolvedIndex, resolvedBytes] = queue.pop()!;
      if (this.solved[resolvedIndex]) continue;
      this.solved[resolvedIndex] = resolvedBytes;
      this.solvedCount += 1;
      const waiting = this.pendingByBlock.get(resolvedIndex);
      this.pendingByBlock.delete(resolvedIndex);
      if (!waiting) continue;
      for (const equation of waiting) {
        for (let byte = 0; byte < equation.bytes.byteLength; byte += 1) equation.bytes[byte] ^= resolvedBytes[byte]!;
        equation.indices.delete(resolvedIndex);
        if (!equation.indices.size) {
          this.pendingEquations.delete(equation);
          continue;
        }
        if (equation.indices.size === 1) {
          const next = equation.indices.values().next().value!;
          this.pendingByBlock.get(next)?.delete(equation);
          this.pendingEquations.delete(equation);
          if (!this.solved[next]) queue.push([next, equation.bytes]);
        }
      }
    }
  }

  private maybeResolveDenseTail(): void {
    const unresolved = this.sourceBlocks - this.solvedCount;
    if (!unresolved || unresolved > 512 || !this.pendingEquations.size) return;
    const cadence = unresolved <= 8 ? 2 : unresolved <= 64 ? 8 : 32;
    if (this.uniqueFrames !== this.sourceBlocks && this.uniqueFrames - this.lastGaussianFrameCount < cadence) return;
    this.lastGaussianFrameCount = this.uniqueFrames;

    const maximumRows = Math.min(4096, Math.max(64, unresolved * 6));
    const equations = [...this.pendingEquations]
      .sort((left, right) => left.indices.size - right.indices.size)
      .slice(0, maximumRows);
    const basis = new Map<number, PendingEquation>();

    for (const equation of equations) {
      const row: PendingEquation = { indices: new Set(equation.indices), bytes: equation.bytes.slice() };
      while (row.indices.size) {
        const pivot = Math.min(...row.indices);
        const existing = basis.get(pivot);
        if (!existing) {
          basis.set(pivot, row);
          break;
        }
        for (const index of existing.indices) {
          if (row.indices.has(index)) row.indices.delete(index);
          else row.indices.add(index);
        }
        for (let byte = 0; byte < row.bytes.byteLength; byte += 1) row.bytes[byte] ^= existing.bytes[byte]!;
      }
    }

    const recovered = new Map<number, Uint8Array>();
    const pivots = [...basis.keys()].sort((left, right) => right - left);
    for (const pivot of pivots) {
      const row = basis.get(pivot)!;
      const value = row.bytes.slice();
      let ready = true;
      for (const index of row.indices) {
        if (index === pivot) continue;
        const known = this.solved[index] ?? recovered.get(index);
        if (!known) {
          ready = false;
          break;
        }
        for (let byte = 0; byte < value.byteLength; byte += 1) value[byte] ^= known[byte]!;
      }
      if (ready) recovered.set(pivot, value);
    }
    for (const [index, bytes] of recovered) this.resolve(index, bytes);
  }
}

export class OpticalReceiver {
  private streamKey = "";
  private sessionHex = "";
  private containerCrc = 0;
  private decoder: FountainDecoder | null = null;

  accept(encoded: string | Uint8Array): boolean {
    const bytes = typeof encoded === "string" ? decodeOpticalFrameText(encoded) : encoded;
    const frame = bytes ? parseFrame(bytes) : null;
    if (!frame) return false;
    const key = [frame.sessionId, frame.sourceBlocks, frame.blockBytes, frame.totalBytes, frame.containerCrc].join(":");
    if (this.streamKey && this.streamKey !== key) this.reset();
    if (!this.decoder) {
      this.streamKey = key;
      this.sessionHex = frame.sessionId.toString(16).padStart(8, "0");
      this.containerCrc = frame.containerCrc;
      this.decoder = new FountainDecoder(frame.sourceBlocks, frame.blockBytes, frame.sessionId, frame.totalBytes);
    }
    return this.decoder.add(frame.sequence, frame.block);
  }

  progress(): OpticalReceiveProgress {
    const decoder = this.decoder;
    const received = decoder?.solvedCount ?? 0;
    const total = decoder?.sourceBlocks ?? 0;
    return {
      sessionId: this.sessionHex,
      name: decoder ? "正在接收文件…" : "等待扫描…",
      acceptedFrames: decoder?.uniqueFrames ?? 0,
      receivedChunks: received,
      totalChunks: total,
      receivedBytes: decoder ? Math.min(decoder.totalBytes, received * decoder.blockBytes) : 0,
      fileSize: decoder?.totalBytes ?? 0,
      percent: total ? Math.min(99, Math.round(received / total * 100)) : 0,
    };
  }

  async complete(): Promise<OpticalReceivedFile | null> {
    const container = this.decoder?.assemble();
    if (!container) return null;
    if (crc32(container) !== this.containerCrc) throw new Error("光学帧重组校验失败，请继续扫描修复帧或重新发送");
    return unpackContainer(container);
  }

  isComplete(): boolean {
    return this.decoder?.complete ?? false;
  }

  reset(): void {
    this.streamKey = "";
    this.sessionHex = "";
    this.containerCrc = 0;
    this.decoder = null;
  }
}
