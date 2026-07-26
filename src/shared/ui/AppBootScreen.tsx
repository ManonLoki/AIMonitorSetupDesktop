// 引入 Mantine 的加载动画、纵向排列容器、文本组件
import { Loader, Stack, Text } from "@mantine/core";
// 引入应用图标资源（打包后的 URL）
import appIconUrl from "../../../src-tauri/icons/ai-monitor/128x128.png";

// 应用启动/路由加载中展示的占位屏幕，用于路由 beforeLoad 预取数据期间的过渡态
export function AppBootScreen() {
  return (
    // 整体容器，承载背景光晕与内容
    <div className="boot-screen">
      {/* 背景光晕装饰元素 */}
      <div className="boot-screen-glow" />
      <Stack align="center" gap="lg" className="boot-screen-content">
        {/* 应用图标外层容器 */}
        <div className="boot-screen-icon-wrap">
          <img className="boot-screen-icon" src={appIconUrl} alt="" />
        </div>
        <Stack align="center" gap={6}>
          {/* 主提示文案 */}
          <Text fw={700} size="lg">
            正在初始化设备连接
          </Text>
          {/* 副提示文案，说明具体在做什么 */}
          <Text size="sm" c="dimmed">
            正在扫描局域网内的 AIMonitor 设备…
          </Text>
        </Stack>
        {/* 加载动画指示器 */}
        <Loader type="bars" size="sm" />
      </Stack>
    </div>
  );
}
