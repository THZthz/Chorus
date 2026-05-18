/**
 * Chorus — cinematic dialogue engine
 * Copyright (C) 2026 Amias 1289941679@qq.com
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

import chalk from "chalk";

export function renderMarkdown(text: string): string {
  if (!text) return "";
  const codeSpans: string[] = [];

  // Extract inline code spans first (they take precedence over all other formatting)
  let result = text.replace(/`([^`]+)`/g, (_match, content) => {
    const idx = codeSpans.length;
    codeSpans.push(chalk.cyan(content));
    return `\x00C${idx}\x00`;
  });

  // Bold: **text** or __text__
  result = result.replace(/\*\*([^*\n]+?)\*\*|__([^_\n]+?)__/g, (_m, star, under) =>
    chalk.bold(star ?? under),
  );

  // Italic: *text* or _text_ (but not ** or __)
  result = result.replace(/(?<!\*)\*([^*\n]+?)\*(?!\*)|\b_([^_\n]+?)_\b/g, (_m, star, under) =>
    chalk.italic(star ?? under),
  );

  // Strikethrough: ~~text~~
  result = result.replace(/~~([^~\n]+?)~~/g, (_m, inner) => chalk.strikethrough(inner));

  // Restore code spans
  result = result.replace(/\x00C(\d+)\x00/g, (_m, idx) => codeSpans[Number(idx)] ?? "");

  return result;
}
