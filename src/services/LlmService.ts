import { WorldState } from "@/types/entities";
import { Message, DialogueStep, SpeakerType } from "@/types/dialogue";

export interface AIResponse extends Omit<DialogueStep, 'messages'> {
  messages: {
    speaker: string;
    type: SpeakerType;
    text: string;
    skillCheck?: {
      skill: string;
      difficulty: string;
      success: boolean;
    };
  }[];
}

class AIService {
  async generateResponse(
    userInput: string,
    worldState: WorldState,
    history: Message[]
  ): Promise<AIResponse> {
    const response = await fetch('/api/chat', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json'
      },
      body: JSON.stringify({
        userInput,
        history
      })
    });

    if (!response.ok) {
      const err = await response.json().catch(() => ({}));
      throw new Error(err.error || response.statusText);
    }
    return response.json();
  }
}

export const aiService = new AIService();

