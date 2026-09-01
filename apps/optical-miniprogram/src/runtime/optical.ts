import { sha256 } from "@noble/hashes/sha2.js";
import { gzip as pakoGzip, Inflate } from "pako";

import {
  configureOpticalRuntime,
  createOpticalTransfer,
  DEFAULT_OPTICAL_FRAME_BYTES,
  isOpticalFrame,
  MAX_OPTICAL_FILE_BYTES,
  OpticalReceiver,
  type OpticalReceivedFile,
  type OpticalReceiveProgress,
  type OpticalTransfer,
} from "../../../desktop/src/features/optical/opticalProtocol";
import { installTextCodecPolyfill } from "./text-codec";

installTextCodecPolyfill();

function boundedGunzip(bytes: Uint8Array, expectedBytes: number): Uint8Array {
  const chunks: Uint8Array[] = [];
  let length = 0;
  const inflater = new Inflate();
  inflater.onData = (chunk: Uint8Array) => {
    length += chunk.byteLength;
    if (length > expectedBytes || length > MAX_OPTICAL_FILE_BYTES) throw new Error("压缩数据展开后超过声明大小");
    chunks.push(Uint8Array.from(chunk));
  };
  inflater.push(bytes, true);
  if (inflater.err) throw new Error(inflater.msg || "gzip 解压失败");
  if (length !== expectedBytes) throw new Error("压缩数据长度校验失败");
  const output = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    output.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return output;
}

function randomBytes(length: number): Uint8Array {
  const output = new Uint8Array(length);
  let state = (Date.now() ^ Math.floor(Math.random() * 0xffffffff)) >>> 0;
  for (let index = 0; index < length; index += 1) {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    output[index] = state & 0xff;
  }
  return output;
}

configureOpticalRuntime({
  sha256: (bytes) => Uint8Array.from(sha256(bytes)),
  gzip: (bytes) => Uint8Array.from(pakoGzip(bytes)),
  gunzip: boundedGunzip,
  randomBytes,
});

export {
  createOpticalTransfer,
  DEFAULT_OPTICAL_FRAME_BYTES,
  isOpticalFrame,
  MAX_OPTICAL_FILE_BYTES,
  OpticalReceiver,
};
export type { OpticalReceivedFile, OpticalReceiveProgress, OpticalTransfer };
