// 引入 TanStack Router 的路由提供者组件，用于渲染当前匹配到的路由
import { RouterProvider } from "@tanstack/react-router";
// 引入全局 Provider 集合（Mantine 主题、TanStack Query、Jotai 状态等）
import { AppProviders } from "./app/providers";
// 引入应用的路由实例（路由树 + 配置）
import { router } from "./app/router";

// 应用根组件：负责套上全局 Provider，再交给路由系统渲染页面
export function App() {
  return (
    // 用全局 Provider 包裹整个应用，提供主题、数据请求缓存等上下文
    <AppProviders>
      {/* 将路由实例交给 RouterProvider，由它根据当前地址渲染对应页面 */}
      <RouterProvider router={router} />
    </AppProviders>
  );
}
