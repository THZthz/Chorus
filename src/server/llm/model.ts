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

import { createDeepSeek } from "@ai-sdk/deepseek";
import { createGoogleGenerativeAI } from "@ai-sdk/google";
import { createOpenAI } from "@ai-sdk/openai";
import { wrapLanguageModel, type LanguageModel } from "ai";
import { devToolsMiddleware } from "@ai-sdk/devtools";

let googleModelInstance: LanguageModel | null = null;
let openrouterModelInstance: LanguageModel | null = null;
let deepseekModelInstance: LanguageModel | null = null;

const googleModel = process.env.GOOGLE_MODEL || "gemini-3.1-pro-preview";
const openrouterModel = process.env.OPENROUTER_MODEL || "anthropic/claude-sonnet-4.6";
const deepseekModel = process.env.DEEPSEEK_MODEL || "deepseek-v4-flash";

function getGoogleModel(): LanguageModel | null {
  if (!googleModelInstance && process.env.GEMINI_API_KEY) {
    try {
      const raw = createGoogleGenerativeAI({
        apiKey: process.env.GEMINI_API_KEY,
      })(googleModel);
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
      const raw = createOpenAI({
        apiKey: process.env.OPENROUTER_API_KEY,
        baseURL: "https://openrouter.ai/api/v1",
      })(openrouterModel);
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
      })(deepseekModel);
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
  const deepseek = getDeepSeekModel();
  if (deepseek) return { model: deepseek, name: deepseekModel };
  const google = getGoogleModel();
  if (google) return { model: google, name: googleModel };
  const openrouter = getOpenRouterModel();
  if (openrouter) return { model: openrouter, name: openrouterModel };
  throw new Error(
    "Missing API Key: Please set GEMINI_API_KEY, OPENROUTER_API_KEY, or DEEPSEEK_API_KEY in file `.env`.",
  );
}
