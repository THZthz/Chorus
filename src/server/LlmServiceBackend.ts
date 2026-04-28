import { createDeepSeek } from "@ai-sdk/deepseek";
// Try to use Google Gen AI for the plot writer, fallback to deepseek
import { createGoogleGenerativeAI } from "@ai-sdk/google";
import { generateText, type LanguageModel, tool } from "ai";
import { z } from "zod";
import { WorldState } from "@/types/entities";
import { Message } from "@/types/dialogue";
import { getAllEntities, updateEntity } from "@/server/models/world";
import { getHistory, addMessage, clearHistory } from "@/server/models/history";
import { getAllPlots, addPlot, updatePlotStatus } from "@/server/models/plot";
import { updateWorldStateTool } from "@/services/tools/updateWorldState";
import { addDialogueStepTool } from "@/services/tools/addDialogueStep";
import { addPlotTool } from "@/services/tools/addPlot";
import { updatePlotStatusTool } from "@/services/tools/updatePlotStatus";
import { addLlmLog, updateLlmLog } from "@/server/models/debug";

let googleModelInstance: LanguageModel | null = null;
let deepseekModelInstance: LanguageModel | null = null;

function getGoogleModel(): LanguageModel | null {
  if (!googleModelInstance && process.env.GEMINI_API_KEY) {
    try {
      googleModelInstance = createGoogleGenerativeAI({
        apiKey: process.env.GEMINI_API_KEY,
      })('gemini-3.1-flash-lite-preview');
    } catch (e) {
      console.error("Failed to initialize Google model:", e);
    }
  }
  return googleModelInstance;
}

function getDeepSeekModel(): LanguageModel | null {
  if (!deepseekModelInstance && process.env.DEEPSEEK_API_KEY) {
    try {
      deepseekModelInstance = createDeepSeek({
        apiKey: process.env.DEEPSEEK_API_KEY,
      })('deepseek-chat');
    } catch (e) {
      console.error("Failed to initialize DeepSeek model:", e);
    }
  }
  return deepseekModelInstance;
}

// The preferred model is Google if we are running in AI Studio with GEMINI_API_KEY, fallback to deepseek
function getModelInfo(): { model: LanguageModel; name: string } {
  const google = getGoogleModel();
  if (google) return { model: google, name: 'gemini-3.1-flash-lite-preview' };

  const deepseek = getDeepSeekModel();
  if (deepseek) return { model: deepseek, name: 'deepseek-chat' };

  throw new Error(
    "Missing API Key: Please set GEMINI_API_KEY or DEEPSEEK_API_KEY in the application settings or .env file."
  );
}

export async function generateAIResponse(
  userInput: string,
  history: Message[]
): Promise<any> {
  const worldState = getAllEntities();
  const plots = getAllPlots();

  const activePlots = plots.filter(p => p.status === 'PENDING' || p.status === 'IN_PROGRESS');

  const gmSystemInstruction = `
You are the Game Master for a narrative-driven RPG. 
SETTING: A dark, gritty medieval world. High-contrast noir aesthetic.
TONE: Philosophical, cynical, and surreal. Mimic the writing style of Disco Elysium.

## INTERNAL VOICES
Use internal voices to represent the player's fractured psyche. 
- LOGIC: Cold, deductive.
- RHETORIC: Political, manipulative.
- VOLITION: Willpower and sanity.
- INLAND EMPIRE: Imagination, supra-natural hunches.
- HALF LIGHT: Pure lizard-brain fear.
- ELECTROCHEMISTRY: Hedonism, desire.

## CONTEXT (World State)
${JSON.stringify(worldState, null, 2)}

## PLOTS (Your master plan)
${JSON.stringify(activePlots, null, 2)}

You MUST progress these plots. If an IN_PROGRESS plot's trigger condition is met, execute it narratively and change it to RESOLVED or progress it.
If no plot is active or they are concluded, you can draft a new concrete scene-based plot.

## Dialogue History
${history.slice(-10).map(m => `${m.speaker} (${m.type}): ${m.text}`).join('\n')}

## MISSION
The user (YOU) has just said/done: "${userInput}"

Your task is to draft the game's response.
1. Draft updates to the world (items, locations) using 'draftUpdateWorldState'.
2. Draft advances to plots using 'draftUpdatePlotStatus' and 'draftAddPlot'.
3. Draft the final narrative response and options using 'draftAddDialogueStep'. Note that 'isAiTrigger' MUST be set to true for options to let the AI gain control again if continuing dialogue.
4. CRITICALLY, you CANNOT commit these changes. You must submit your drafts to the Assistant by calling 'communicateAssistant'.

If the Assistant tells you to revise, your previous drafts are cleared. You must adapt your drafts by calling the drafting tools again, and then call 'communicateAssistant' again.

Good tool usage example:
[Calls draftUpdateWorldState]
[Calls draftAddDialogueStep]
[Calls communicateAssistant with message: "I drafted the consequences. Please review."]

Bad tool usage example:
- Generating text instead of using tools.
- Not calling communicateAssistant.
- Forgetting to include 'isAiTrigger: true' in dialogue options when the player should reply.
  `;

  let finalResponse: any = null;

  const promptText = `The player says: "${userInput}". Process the turn by drafting updates and asking the Assistant.`;
  
  const { model, name: modelName } = getModelInfo();

  let gmMessages: any[] = [
    { role: 'user', content: promptText }
  ];

  let turnFinished = false;

  let drafts: any = {
    worldUpdates: [],
    newPlots: [],
    plotStatusUpdates: [],
    dialogue: null
  };

  const startTime = Date.now();
  let logId = -1; // To be set

  try {
    const result = await generateText({
      model,
      system: gmSystemInstruction,
      messages: gmMessages,
      maxSteps: 10,
      tools: {
        draftUpdateWorldState: tool({
          description: "Propose updates to world state entities. " + updateWorldStateTool.description,
          parameters: updateWorldStateTool.parameters,
          execute: async (args) => {
            if (args && args.updates) drafts.worldUpdates.push(...args.updates);
            return "Draft recorded.";
          }
        }),
        draftAddDialogueStep: tool({
          description: "Propose a narrative dialogue step. " + addDialogueStepTool.description,
          parameters: addDialogueStepTool.parameters,
          execute: async (args) => {
            drafts.dialogue = args;
            return "Draft recorded.";
          }
        }),
        draftUpdatePlotStatus: tool({
          description: "Propose an advance to a plot status. " + updatePlotStatusTool.description,
          parameters: updatePlotStatusTool.parameters,
          execute: async (args) => {
            drafts.plotStatusUpdates.push({ id: args.id, status: args.status });
            return "Draft recorded.";
          }
        }),
        draftAddPlot: tool({
          description: "Propose a new concrete plot. " + addPlotTool.description,
          parameters: addPlotTool.parameters,
          execute: async (args) => {
            drafts.newPlots.push({
              title: args.title,
              description: args.description,
              triggerCondition: args.triggerCondition
            });
            return "Draft recorded.";
          }
        }),
        communicateAssistant: tool({
          description: "Submit all drafted changes to the Assistant for review. You must provide a message. The Assistant will approve or request changes.",
          parameters: z.object({ message: z.string() }),
          execute: async ({ message }) => {
            const assistantSystemInstruction = `
You are the vigilant Assistant to the Game Master.
Your job is to thoroughly review the drafts proposed by the Game Master.

Good tool usage:
- If everything is perfect, call 'commitDrafts'.
- If the GM made a mistake (like forgetting 'isAiTrigger: true', missing a plot trigger, mutating a non-existent item), use 'replyToGM' to tell them to fix it.

Bad tool usage:
- Responding with text only instead of calling tools.
            `;

            let assistantMessages: any[] = [
              { 
                role: 'user', 
                content: `GM's Message: ${message}\n\nCurrent World State:\n${JSON.stringify(worldState, null, 2)}\n\nActive Plots:\n${JSON.stringify(activePlots, null, 2)}\n\nProposed Drafts:\n${JSON.stringify(drafts, null, 2)}\n\nEvaluate the drafts.`
              }
            ];
            
            let assistantFeedback = "";

            await generateText({
              model,
              system: assistantSystemInstruction,
              messages: assistantMessages,
              maxSteps: 3,
              tools: {
                replyToGM: tool({
                  description: "Use this to reject the drafts and give feedback to the GM.",
                  parameters: z.object({ feedback: z.string() }),
                  execute: async ({ feedback }) => {
                    assistantFeedback = feedback;
                    return "Feedback sent to GM.";
                  }
                }),
                commitDrafts: tool({
                  description: "Use this if the drafts are perfect and you want to commit them.",
                  parameters: z.object({}),
                  execute: async () => {
                    turnFinished = true;
                    for (const u of drafts.worldUpdates) updateEntity(u);
                    for (const p of drafts.newPlots) addPlot(p);
                    for (const ps of drafts.plotStatusUpdates) updatePlotStatus(ps.id, ps.status);
                    finalResponse = drafts.dialogue;
                    return "Committed.";
                  }
                })
              }
            });
            
            if (turnFinished) {
              return "SUCCESS: Assistant committed the drafts. Turn is over.";
            } else {
              drafts = { worldUpdates: [], newPlots: [], plotStatusUpdates: [], dialogue: null };
              return `ASSISTANT FEEDBACK: ${assistantFeedback || "No feedback provided by assistant."}. Your drafts have been CLEARED. Please propose new ones and call communicateAssistant again.`;
            }
          }
        })
      }
    });

    const duration = Date.now() - startTime;
    // Log the interaction
    logId = addLlmLog({
      model: modelName,
      system: gmSystemInstruction,
      prompt: promptText,
      userInput,
      history: history,
      tools: [] // omitted for brevity
    });
    updateLlmLog(logId, result, duration, 'SUCCESS');

    if (!finalResponse) {
      if (result.text) {
        finalResponse = {
          messages: [{
            speaker: 'SYSTEM',
            type: 'SYSTEM',
            text: result.text
          }],
          options: [{ id: 'opt_1', text: 'Continue', isAiTrigger: true }]
        };
      } else {
        throw new Error("AI failed to return dialogue step.");
      }
    }

    return finalResponse;
  } catch (error: any) {
    const duration = Date.now() - startTime;
    updateLlmLog(logId, { error: error.message, stack: error.stack }, duration, 'ERROR');
    throw error;
  }
}

