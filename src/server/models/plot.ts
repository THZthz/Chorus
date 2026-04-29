import db from "@/server/db";

export interface Plot {
  id: string;
  title: string;
  description: string;
  triggerCondition: string;
  status: 'PENDING' | 'IN_PROGRESS' | 'RESOLVED';
}

export function getAllPlots(): Plot[] {
  return db.prepare("SELECT * FROM plots").all() as Plot[];
}

export function updatePlotStatus(id: string, status: string) {
  db.prepare("UPDATE plots SET status = ? WHERE id = ?").run(status, id);
}

export function addPlot(plot: Omit<Plot, 'id' | 'status'> & { id?: string }) {
  const id = plot.id || `plot_${Date.now()}`;
  db.prepare("INSERT INTO plots (id, title, description, triggerCondition, status) VALUES (?, ?, ?, ?, 'PENDING')")
    .run(id, plot.title, plot.description, plot.triggerCondition);
}
