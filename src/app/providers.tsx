// 引入 Mantine 的主题 Provider 与主题创建函数
import { MantineProvider, createTheme } from "@mantine/core";
// 引入 TanStack Query 的 Provider，用于向子树提供 QueryClient 上下文
import { QueryClientProvider } from "@tanstack/react-query";
// 引入 React 的 PropsWithChildren 类型，方便声明“接收 children”的组件 props
import type { PropsWithChildren } from "react";
// 引入 Jotai 的只读取值 hook
import { useAtomValue } from "jotai";
// 引入本应用全局唯一的 QueryClient 实例
import { queryClient } from "./query-client";
// 引入颜色主题（浅色/深色）的 Jotai atom
import { colorSchemeAtom } from "../shared/state/ui";
// 引入监控设备事件订阅 hook，用于监听 Rust 端推送的设备相关事件
import { useMonitorDeviceEvents } from "../features/monitor/hooks/useMonitorDeviceEvents";
import { I18nProvider } from "../shared/i18n";

// 定义 Mantine 主题：主色调、默认圆角、字体等全局视觉配置
const theme = createTheme({
  primaryColor: "violet",
  defaultRadius: "lg",
  fontFamily:
    "Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, sans-serif",
  headings: {
    fontFamily:
      "Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, sans-serif",
    fontWeight: "650",
  },
});

// 应用级 Provider 组合：串联 TanStack Query、Mantine 主题与设备事件监听
export function AppProviders({ children }: PropsWithChildren) {
  // 读取当前颜色主题（浅色/深色），用于强制 Mantine 使用对应配色方案
  const colorScheme = useAtomValue(colorSchemeAtom);

  return (
    // 提供 QueryClient 上下文，使子树内组件可以使用 useQuery / useMutation
    <I18nProvider>
      <QueryClientProvider client={queryClient}>
        {/* 提供 Mantine 主题上下文，并根据 Jotai 状态强制指定当前配色方案 */}
        <MantineProvider theme={theme} forceColorScheme={colorScheme}>
          {/* 挂载设备事件监听组件（本身不渲染任何内容） */}
          <MonitorDeviceEvents />
          {children}
        </MantineProvider>
      </QueryClientProvider>
    </I18nProvider>
  );
}

// 纯副作用组件：订阅监控设备事件，不渲染任何 UI（返回 null）
function MonitorDeviceEvents() {
  useMonitorDeviceEvents();
  return null;
}
