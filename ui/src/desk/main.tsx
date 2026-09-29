import React from "react";
import ReactDOM from "react-dom/client";
import { DeskApp } from "./DeskApp";
import "./desk.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <DeskApp />
  </React.StrictMode>
);
