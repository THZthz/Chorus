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

import { checkText, wrapSafe } from "@/server/llm/tools/shared";
import { exec } from "../helpers";

describe("checkText", () => {
  it("allows ASCII text", () => {
    expect(checkText("Hello world", "test")).toBeNull();
  });

  it("allows numbers and punctuation", () => {
    expect(checkText("Test 123, with punctuation!?", "test")).toBeNull();
  });

  it("allows markdown formatting", () => {
    expect(checkText("**bold** and *italic* and `code`", "test")).toBeNull();
  });

  it("rejects emoji", () => {
    const result = checkText("Hello 😀 world", "test");
    expect(result).not.toBeNull();
    expect(result).toContain("disallowed characters");
  });

  it("rejects CJK characters", () => {
    const result = checkText("Hello 世界", "test");
    expect(result).not.toBeNull();
    expect(result).toContain("disallowed characters");
  });

  it("rejects Arabic characters", () => {
    const result = checkText("Hello مرحبا", "test");
    expect(result).not.toBeNull();
    expect(result).toContain("disallowed characters");
  });

  it("rejects Cyrillic characters", () => {
    const result = checkText("Hello привет", "test");
    expect(result).not.toBeNull();
    expect(result).toContain("disallowed characters");
  });

  it("accepts non-string values by JSON-stringifying them", () => {
    expect(checkText(12345, "test")).toBeNull();
    expect(checkText(true, "test")).toBeNull();
  });
});

describe("wrapSafe", () => {
  it("passes through successful result", async () => {
    const wrapped = wrapSafe(async (args: { x: number }) => `Result: ${args.x}`, "TestTool");
    const result = await wrapped({ x: 42 });
    expect(result).toBe("Result: 42");
  });

  it("blocks input with disallowed characters", async () => {
    const wrapped = wrapSafe(async (_args: { text: string }) => "ok", "TestTool");
    const result = await wrapped({ text: "Hello 😀" });
    expect(result).toContain("TEXT VERIFICATION FAILED");
  });

  it("catches thrown errors and formats them", async () => {
    const wrapped = wrapSafe(async (_args: {}) => {
      throw new Error("Something broke");
    }, "TestTool");
    const result = await wrapped({});
    expect(result).toContain("ERROR: Tool");
    expect(result).toContain("TestTool");
    expect(result).toContain("Something broke");
  });

  it("catches non-Error throws", async () => {
    const wrapped = wrapSafe(async (_args: {}) => {
      throw "raw string error";
    }, "TestTool");
    const result = await wrapped({});
    expect(result).toContain("ERROR: Tool");
    expect(result).toContain("raw string error");
  });
});
