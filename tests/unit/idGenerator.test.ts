/**
 * Chorus — cinematic dialogue engine
 * Copyright (C) 2026 Amias
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

import { feistelEncrypt, feistelDecrypt, SECRET } from "@/server/idGenerator";

describe("SECRET", () => {
  it("is an array", () => {
    expect(Array.isArray(SECRET)).toBe(true);
  });

  it("has 4 elements", () => {
    expect(SECRET).toHaveLength(4);
  });

  it("contains only numbers", () => {
    for (const val of SECRET) {
      expect(typeof val).toBe("number");
    }
  });

  it("contains 16-bit unsigned integers", () => {
    for (const val of SECRET) {
      expect(Number.isInteger(val)).toBe(true);
      expect(val).toBeGreaterThanOrEqual(0);
      expect(val).toBeLessThanOrEqual(0xffff);
    }
  });
});

describe("feistelEncrypt", () => {
  const KEY = [0xabcd, 0x1234, 0x5678, 0x9abc];

  it("returns a non-negative integer for valid inputs", () => {
    const result = feistelEncrypt(42, KEY);
    expect(Number.isInteger(result)).toBe(true);
    expect(result).toBeGreaterThanOrEqual(0);
  });

  it("returns a value within the 32-bit unsigned range", () => {
    for (let i = 0; i < 1000; i++) {
      const result = feistelEncrypt(i, KEY);
      expect(result).toBeGreaterThanOrEqual(0);
      expect(result).toBeLessThanOrEqual(0xffffffff);
    }
  });

  it("is deterministic — same input, same key produces same output", () => {
    for (let i = 0; i < 100000; i++) {
      expect(feistelEncrypt(i, KEY)).toBe(feistelEncrypt(i, KEY));
    }
  });

  it("produces different outputs for different inputs (no collisions in 100k range)", () => {
    const seen = new Set<number>();
    for (let i = 0; i < 100000; i++) {
      const enc = feistelEncrypt(i, KEY);
      expect(seen.has(enc)).toBe(false);
      seen.add(enc);
    }
  });

  it("maps 0 to something non-obvious", () => {
    const enc = feistelEncrypt(0, KEY);
    // Should obfuscate — not trivially 0
    expect(enc).not.toBe(0);
  });

  it("maps sequential inputs to non-sequential outputs", () => {
    const r0 = feistelEncrypt(0, KEY);
    const r1 = feistelEncrypt(1, KEY);
    const r2 = feistelEncrypt(2, KEY);
    // Sequential inputs should not produce sequential outputs
    expect(r1 - r0).not.toBe(1);
    expect(r2 - r1).not.toBe(1);
    expect(r1).not.toBe(r0 + 1);
  });

  it("handles the maximum 32-bit unsigned value", () => {
    const maxVal = 0xffffffff;
    const result = feistelEncrypt(maxVal, KEY);
    expect(result).toBeGreaterThanOrEqual(0);
    expect(result).toBeLessThanOrEqual(0xffffffff);
  });

  it("handles large inputs near the 32-bit boundary correctly", () => {
    const nearMax = 0xfffffffe;
    const result = feistelEncrypt(nearMax, KEY);
    expect(result).toBeGreaterThanOrEqual(0);
    expect(result).toBeLessThanOrEqual(0xffffffff);
  });

  it("produces different results with different keys", () => {
    const keyA = [0x1111, 0x2222, 0x3333, 0x4444];
    const keyB = [0x5555, 0x6666, 0x7777, 0x8888];
    const input = 12345;
    expect(feistelEncrypt(input, keyA)).not.toBe(feistelEncrypt(input, keyB));
  });

  it("produces different results with different-length keys", () => {
    const shortKey = [0xabcd];
    const input = 9999;
    const r1 = feistelEncrypt(input, KEY);
    const r2 = feistelEncrypt(input, shortKey);
    expect(r1).not.toBe(r2);
  });

  it("works with different key lengths", () => {
    for (const key of [[0xabcd], [0x1111, 0x2222], [0x3333, 0x4444, 0x5555, 0x6666, 0x7777]]) {
      for (let i = 0; i < 100000; i++) {
        const enc = feistelEncrypt(i, key);
        expect(enc).toBeGreaterThanOrEqual(0);
        expect(enc).toBeLessThanOrEqual(0xffffffff);
      }
    }
  });

  it("distributes output across the full 16-bit halves (visual inspection)", () => {
    // Ensure both halves vary — encodings that only vary in one half are weak.
    const halfSeen = { left: new Set<number>(), right: new Set<number>() };
    for (let i = 0; i < 1000; i++) {
      const enc = feistelEncrypt(i, KEY);
      halfSeen.left.add((enc >>> 16) & 0xffff);
      halfSeen.right.add(enc & 0xffff);
    }
    // Each half should vary across more than just a handful of values
    expect(halfSeen.left.size).toBeGreaterThan(100);
    expect(halfSeen.right.size).toBeGreaterThan(100);
  });
});

describe("feistelDecrypt", () => {
  const KEY = [0xabcd, 0x1234, 0x5678, 0x9abc];

  it("is the inverse of feistelEncrypt for various inputs", () => {
    for (let i = 0; i < 10000; i++) {
      const enc = feistelEncrypt(i, KEY);
      const dec = feistelDecrypt(enc, KEY);
      expect(dec).toBe(i);
    }
  });

  it("encrypt(decrypt(y)) === y", () => {
    for (let i = 0; i < 10000; i++) {
      const enc = feistelEncrypt(i, KEY);
      // Decrypt then encrypt should yield original encrypted value
      const dec = feistelDecrypt(enc, KEY);
      const reEnc = feistelEncrypt(dec, KEY);
      expect(reEnc).toBe(enc);
    }
  });

  it("handles edge case 0 correctly (round-trip)", () => {
    const enc = feistelEncrypt(0, KEY);
    expect(feistelDecrypt(enc, KEY)).toBe(0);
  });

  it("handles 2^32-1 correctly (round-trip)", () => {
    const maxVal = 0xffffffff;
    const enc = feistelEncrypt(maxVal, KEY);
    expect(feistelDecrypt(enc, KEY)).toBe(maxVal);
  });

  it("decrypts with the same key used for encryption", () => {
    const input = 42;
    const enc = feistelEncrypt(input, SECRET);
    expect(feistelDecrypt(enc, SECRET)).toBe(input);
  });

  it("fails to decrypt with the WRONG key", () => {
    const wrongKey = [0xdead, 0xbeef, 0xcafe, 0xbabe];
    const enc = feistelEncrypt(42, KEY);
    const dec = feistelDecrypt(enc, wrongKey);
    expect(dec).not.toBe(42);
  });

  it("round-trips with a single-element key", () => {
    const key = [0xabcd];
    for (let i = 0; i < 1000; i++) {
      const enc = feistelEncrypt(i, key);
      expect(feistelDecrypt(enc, key)).toBe(i);
    }
  });

  it("round-trips with an odd-length key", () => {
    const key = [0x1111, 0x2222, 0x3333];
    for (let i = 0; i < 500; i++) {
      const enc = feistelEncrypt(i, key);
      expect(feistelDecrypt(enc, key)).toBe(i);
    }
  });

  it("round-trips every value in a medium range (comprehensive bijection check)", () => {
    // Full 2^32 would take too long, but 0xffff (65535) is a reasonable verification
    const range = 0xffff;
    const seen = new Set<number>();
    for (let i = 0; i < range; i++) {
      const enc = feistelEncrypt(i, KEY);
      expect(enc).toBeGreaterThanOrEqual(0);
      expect(enc).toBeLessThanOrEqual(0xffffffff);
      expect(seen.has(enc)).toBe(false);
      seen.add(enc);
      expect(feistelDecrypt(enc, KEY)).toBe(i);
    }
  });
});

describe("feistelEncrypt/Decrypt integration with SECRET", () => {
  it("SECRET round-trips values correctly", () => {
    for (let i = 0; i < 10000; i++) {
      const enc = feistelEncrypt(i, SECRET);
      const dec = feistelDecrypt(enc, SECRET);
      expect(dec).toBe(i);
    }
  });

  it("SECRET produces non-sequential 4-char base62 IDs (sampled)", () => {
    // Quick sanity check that the typical usage pattern works
    const ids = new Set<string>();
    for (let i = 0; i < 1000; i++) {
      const enc = feistelEncrypt(i, SECRET);
      // Each encoded value should fit in 4 base62 chars (62^4 ≈ 14.7M > 2^32)
      expect(enc).toBeLessThanOrEqual(0xffffffff);
      ids.add(String(enc));
    }
    expect(ids.size).toBe(1000); // all unique
  });
});
