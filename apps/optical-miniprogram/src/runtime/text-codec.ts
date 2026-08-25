function encodeUtf8(input: string): Uint8Array {
  const output: number[] = [];
  for (const character of input) {
    const codePoint = character.codePointAt(0)!;
    if (codePoint <= 0x7f) output.push(codePoint);
    else if (codePoint <= 0x7ff) output.push(0xc0 | (codePoint >>> 6), 0x80 | (codePoint & 0x3f));
    else if (codePoint <= 0xffff) output.push(0xe0 | (codePoint >>> 12), 0x80 | ((codePoint >>> 6) & 0x3f), 0x80 | (codePoint & 0x3f));
    else output.push(0xf0 | (codePoint >>> 18), 0x80 | ((codePoint >>> 12) & 0x3f), 0x80 | ((codePoint >>> 6) & 0x3f), 0x80 | (codePoint & 0x3f));
  }
  return Uint8Array.from(output);
}

function decodeUtf8(input: ArrayBufferView | ArrayBuffer, fatal: boolean): string {
  const bytes = input instanceof ArrayBuffer
    ? new Uint8Array(input)
    : new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
  let output = "";
  let offset = 0;

  const invalid = () => {
    if (fatal) throw new TypeError("无效的 UTF-8 数据");
    output += "\ufffd";
  };

  while (offset < bytes.byteLength) {
    const first = bytes[offset]!;
    if (first <= 0x7f) {
      output += String.fromCharCode(first);
      offset += 1;
      continue;
    }
    const continuationCount = first >= 0xc2 && first <= 0xdf ? 1 : first >= 0xe0 && first <= 0xef ? 2 : first >= 0xf0 && first <= 0xf4 ? 3 : -1;
    if (continuationCount < 0 || offset + continuationCount >= bytes.byteLength) {
      invalid();
      offset += 1;
      continue;
    }
    let codePoint = first & (0x7f >>> continuationCount);
    let valid = true;
    for (let index = 1; index <= continuationCount; index += 1) {
      const next = bytes[offset + index]!;
      if ((next & 0xc0) !== 0x80) {
        valid = false;
        break;
      }
      codePoint = (codePoint << 6) | (next & 0x3f);
    }
    const minimum = continuationCount === 1 ? 0x80 : continuationCount === 2 ? 0x800 : 0x10000;
    if (!valid || codePoint < minimum || codePoint > 0x10ffff || (codePoint >= 0xd800 && codePoint <= 0xdfff)) {
      invalid();
      offset += 1;
      continue;
    }
    if (codePoint <= 0xffff) output += String.fromCharCode(codePoint);
    else {
      const adjusted = codePoint - 0x10000;
      output += String.fromCharCode(0xd800 | (adjusted >>> 10), 0xdc00 | (adjusted & 0x3ff));
    }
    offset += continuationCount + 1;
  }
  return output;
}

export class MiniTextEncoder {
  readonly encoding = "utf-8";
  encode(input = ""): Uint8Array {
    return encodeUtf8(String(input));
  }
}

export class MiniTextDecoder {
  readonly encoding = "utf-8";
  readonly fatal: boolean;
  readonly ignoreBOM = false;

  constructor(_label = "utf-8", options: TextDecoderOptions = {}) {
    this.fatal = Boolean(options.fatal);
  }

  decode(input: AllowSharedBufferSource = new Uint8Array()): string {
    return decodeUtf8(input as ArrayBufferView | ArrayBuffer, this.fatal);
  }
}

export function installTextCodecPolyfill(): void {
  const target = globalThis as typeof globalThis & { TextEncoder?: typeof TextEncoder; TextDecoder?: typeof TextDecoder };
  if (typeof target.TextEncoder === "undefined") target.TextEncoder = MiniTextEncoder as unknown as typeof TextEncoder;
  if (typeof target.TextDecoder === "undefined") target.TextDecoder = MiniTextDecoder as unknown as typeof TextDecoder;
}
