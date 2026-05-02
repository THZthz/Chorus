export interface PlotOption {
  plotId: string | null;
  triggerCondition: string;
}

export interface Plot {
  id: string;
  title: string;
  description: string;
  status: "PENDING" | "IN_PROGRESS" | "RESOLVED";
  involvedLocations: string[];
  involvedCharacters: string[];
  parentPlotId: string | null;
  parentOptionId: number | null;
  childPlots: PlotOption[];
}
