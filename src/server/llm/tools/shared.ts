const disallowedRe =
  /[\p{Emoji}\p{Script=Han}\p{Script=Arabic}\p{Script=Cyrillic}\p{Script=Hiragana}\p{Script=Katakana}]/u;

function isAllowedText(str: string): boolean {
  if (!disallowedRe.test(str)) return true;
  return /^[0-9#*]$/.test(str);
}

export function checkText(value: unknown, context: string): string | null {
  const str = typeof value === "string" ? value : JSON.stringify(value);
  const disallowed = [...str].filter((c) => !isAllowedText(c));
  if (disallowed.length !== 0) {
    const unique = [...new Set(disallowed)].slice(0, 10);
    return `TEXT VERIFICATION FAILED in ${context}: disallowed characters detected [${unique.join(",")}].`;
  }
  return null;
}

export function wrapSafe<T>(
  fn: (args: T) => Promise<string>,
  toolName: string,
): (args: T) => Promise<string> {
  return async (args: T) => {
    const inputError = checkText(args, `${toolName} input`);
    if (inputError) return inputError;
    try {
      const result = await fn(args);
      const outputError = checkText(result, `${toolName} output`);
      if (outputError) return outputError;
      return result;
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      return `ERROR: Tool "${toolName}" failed: ${msg}. Retry or use a different approach.`;
    }
  };
}
