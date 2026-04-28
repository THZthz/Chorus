import { createDeepSeek } from "@ai-sdk/deepseek";
// Try to use Google Gen AI for the plot writer, fallback to deepseek
import { createGoogleGenerativeAI } from "@ai-sdk/google";
import { generateText, stepCountIs, type LanguageModel } from "ai";
import { Message } from "@/types/dialogue";
import { getAllEntities } from "@/server/models/world";
import { getAllPlots } from "@/server/models/plot";
import { addLlmLog, updateLlmLog } from "@/server/models/debug";
import { createDraftUpdateWorldStateTool } from "@/services/tools/draftUpdateWorldState";
import { createDraftAddDialogueStepTool } from "@/services/tools/draftAddDialogueStep";
import { createDraftUpdatePlotStatusTool } from "@/services/tools/draftUpdatePlotStatus";
import { createDraftAddPlotTool } from "@/services/tools/draftAddPlot";
import { createCommunicateAssistantTool } from "@/services/tools/communicateAssistant";

let googleModelInstance: LanguageModel | null = null;
let deepseekModelInstance: LanguageModel | null = null;

function getGoogleModel(): LanguageModel | null {
  if (!googleModelInstance && process.env.GEMINI_API_KEY) {
    try {
      googleModelInstance = createGoogleGenerativeAI({
        apiKey: process.env.GEMINI_API_KEY,
      })('gemini-2.0-flash-lite-preview-02-05');
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
  if (google) return { model: google, name: 'gemini-2.0-flash' };

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
  // Log the interaction start
  let logId = addLlmLog({
    model: modelName,
    system: gmSystemInstruction,
    prompt: promptText,
    userInput,
    history: history,
    tools: []
  });

  try {
    const result = await generateText({
      model,
      system: gmSystemInstruction,
      messages: gmMessages,
      stopWhen: stepCountIs(10),
      tools: {
        draftUpdateWorldState: createDraftUpdateWorldStateTool(drafts),
        draftAddDialogueStep: createDraftAddDialogueStepTool(drafts),
        draftUpdatePlotStatus: createDraftUpdatePlotStatusTool(drafts),
        draftAddPlot: createDraftAddPlotTool(drafts),
        communicateAssistant: createCommunicateAssistantTool({
          model,
          worldState,
          activePlots,
          drafts,
          onCommit: (dialogue) => {
            turnFinished = true;
            finalResponse = dialogue;
          }
        })
      }
    });

    const duration = Date.now() - startTime;
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

