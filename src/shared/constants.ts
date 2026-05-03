export const TOOL_NAMES = {
  GET_ALL_ENTITIES: "getAllEntitiesName",
  QUERY_ENTITY: "queryEntity",
  EDIT_ENTITY: "editEntity",
  CREATE_PLOT: "createPlot",
  EDIT_PLOT: "editPlot",
  GET_PLOT: "getPlot",
  GENERATE_DIALOGUE: "generateDialogueStep",
} as const;

export type ToolName = (typeof TOOL_NAMES)[keyof typeof TOOL_NAMES];
