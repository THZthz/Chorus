import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "@/client/App";
import "@/client/index.css";
import { CharacterProvider } from "@/context/CharacterContext";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <CharacterProvider>
      <App />
    </CharacterProvider>
  </StrictMode>,
);
