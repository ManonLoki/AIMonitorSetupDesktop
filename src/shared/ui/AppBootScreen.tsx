import { Loader, Stack, Text } from "@mantine/core";
import appIconUrl from "../../../src-tauri/icons/ai-monitor/128x128.png";

export function AppBootScreen() {
  return (
    <div className="boot-screen">
      <div className="boot-screen-glow" />
      <Stack align="center" gap="lg" className="boot-screen-content">
        <div className="boot-screen-icon-wrap">
          <img className="boot-screen-icon" src={appIconUrl} alt="" />
        </div>
        <Stack align="center" gap={6}>
          <Text fw={700} size="lg">
            正在初始化设备连接
          </Text>
          <Text size="sm" c="dimmed">
            正在扫描局域网内的 AIMonitor 设备…
          </Text>
        </Stack>
        <Loader type="bars" size="sm" />
      </Stack>
    </div>
  );
}
