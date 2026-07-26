// 引入 TanStack Query 的客户端类，用于统一管理异步请求的缓存与状态
import { QueryClient } from "@tanstack/react-query";

// 创建全局唯一的 QueryClient 实例，供 QueryClientProvider 和路由预加载共用
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // 请求失败后只自动重试 1 次，避免设备离线时无限重试
      retry: 1,
      // 窗口重新获得焦点时不自动重新请求，避免频繁刷新
      refetchOnWindowFocus: false,
    },
  },
});
