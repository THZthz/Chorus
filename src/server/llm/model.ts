/**
 * Chorus — cinematic RPG-style dialogue engine
 * Copyright (C) 2026  Amias
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

import { createDeepSeek } from "@ai-sdk/deepseek";
import { createGoogleGenerativeAI } from "@ai-sdk/google";
import { createOpenAI } from "@ai-sdk/openai";
import { wrapLanguageModel, type LanguageModel } from "ai";
import { devToolsMiddleware } from "@ai-sdk/devtools";

let googleModelInstance: LanguageModel | null = null;
let openrouterModelInstance: LanguageModel | null = null;
let deepseekModelInstance: LanguageModel | null = null;

function getGoogleModel(): LanguageModel | null {
  if (!googleModelInstance && process.env.GEMINI_API_KEY) {
    try {
      const raw = createGoogleGenerativeAI({
        apiKey: process.env.GEMINI_API_KEY,
      })("gemini-2.0-flash-lite-preview-02-05");
      googleModelInstance = wrapLanguageModel({
        model: raw,
        middleware: devToolsMiddleware(),
      });
    } catch (e) {
      console.error("Failed to initialize Google model:", e);
    }
  }
  return googleModelInstance;
}

function getOpenRouterModel(): LanguageModel | null {
  if (!openrouterModelInstance && process.env.OPENROUTER_API_KEY) {
    try {
      const modelName = process.env.OPENROUTER_MODEL || "openrouter/auto";
      const raw = createOpenAI({
        apiKey: process.env.OPENROUTER_API_KEY,
        baseURL: "https://openrouter.ai/api/v1",
      })(modelName);
      openrouterModelInstance = wrapLanguageModel({
        model: raw,
        middleware: devToolsMiddleware(),
      });
    } catch (e) {
      console.error("Failed to initialize OpenRouter model:", e);
    }
  }
  return openrouterModelInstance;
}

function getDeepSeekModel(): LanguageModel | null {
  if (!deepseekModelInstance && process.env.DEEPSEEK_API_KEY) {
    try {
      const raw = createDeepSeek({
        apiKey: process.env.DEEPSEEK_API_KEY,
      })("deepseek-v4-flash");
      deepseekModelInstance = wrapLanguageModel({
        model: raw,
        middleware: devToolsMiddleware(),
      });
    } catch (e) {
      console.error("Failed to initialize DeepSeek model:", e);
    }
  }
  return deepseekModelInstance;
}

export function getModel(): { model: LanguageModel; name: string } {
  const google = getGoogleModel();
  if (google) return { model: google, name: "gemini-2.0-flash" };
  const openrouter = getOpenRouterModel();
  if (openrouter) return { model: openrouter, name: "openrouter/auto" };
  const deepseek = getDeepSeekModel();
  if (deepseek) return { model: deepseek, name: "deepseek-v4-flash" };
  throw new Error(
    "Missing API Key: Please set GEMINI_API_KEY, OPENROUTER_API_KEY, or DEEPSEEK_API_KEY in .env",
  );
}
