import type { Message, DialogueOption } from "@/types/dialogue";

export function buildHistoryFromTree(
  stepId: string,
  treeSteps: Record<
    string,
    {
      id: string;
      parentStepId: string | null;
      parentOptionId: string | null;
      messages: Message[];
      options: DialogueOption[];
    }
  >,
): Message[] {
  const chain: (typeof treeSteps)[string][] = [];
  let cur: (typeof treeSteps)[string] | undefined = treeSteps[stepId];
  while (cur) {
    chain.unshift(cur);
    cur = cur.parentStepId ? treeSteps[cur.parentStepId] : undefined;
  }
  const result: Message[] = [];
  for (let i = 0; i < chain.length; i++) {
    const step = chain[i];
    if (i > 0) {
      const parent = chain[i - 1];
      const opt = parent.options.find((o) => o.id === step.parentOptionId);
      if (opt) {
        const youText = opt.selectionMessage ?? opt.text.replace(/^\[[^\]]*?:[^\]]*?\]\s*/, "");
        result.push({ id: `you-tree-${i}`, speaker: "YOU", type: "YOU", text: youText });
      } else if (!step.parentOptionId) {
        result.push({ id: `you-tree-${i}`, speaker: "YOU", type: "YOU", text: "[Free choice]" });
      }
    }
    result.push(...step.messages);
  }
  return result;
}
