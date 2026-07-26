// 引入 React 核心库（用于 StrictMode 等）
import React from "react";
// 引入 React DOM 客户端渲染入口
import ReactDOM from "react-dom/client";
// 引入 Mantine 组件库的基础样式
import "@mantine/core/styles.css";
// 引入应用根组件
import { App } from "./App";
// 引入应用自定义全局样式
import "./App.css";

// 找到 index.html 中 id 为 root 的 DOM 节点，创建 React 根并渲染应用
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  // 使用严格模式包裹，帮助在开发环境中发现潜在问题
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
