import chalk from "chalk";

export function renderMarkdown(text: string): string {
  const codeSpans: string[] = [];

  // Extract inline code spans first (they take precedence over all other formatting)
  let result = text.replace(/`([^`]+)`/g, (_match, content) => {
    const idx = codeSpans.length;
    codeSpans.push(chalk.cyan(content));
    return `\x00C${idx}\x00`;
  });

  // Bold: **text** or __text__
  result = result.replace(
    /\*\*([^*\n]+?)\*\*|__([^_\n]+?)__/g,
    (_m, star, under) => chalk.bold(star ?? under),
  );

  // Italic: *text* or _text_ (but not ** or __)
  result = result.replace(
    /(?<!\*)\*([^*\n]+?)\*(?!\*)|\b_([^_\n]+?)_\b/g,
    (_m, star, under) => chalk.italic(star ?? under),
  );

  // Strikethrough: ~~text~~
  result = result.replace(
    /~~([^~\n]+?)~~/g,
    (_m, inner) => chalk.strikethrough(inner),
  );

  // Restore code spans
  result = result.replace(/\x00C(\d+)\x00/g, (_m, idx) => codeSpans[Number(idx)] ?? "");

  return result;
}
