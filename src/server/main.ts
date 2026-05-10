import "dotenv/config";
import express from "express";
import apiRouter from "@/server/api";

const app = express();
const PORT = 3000;

app.use(express.json());
app.use("/api", apiRouter);

app.listen(PORT, "0.0.0.0", () => {
  console.log(`Server running on http://localhost:${PORT}`);
});
