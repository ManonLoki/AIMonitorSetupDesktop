import { Stack } from "@mantine/core";
import { DeviceConnectPanel } from "../components/DeviceConnectPanel";

export function SettingsPage() {
  return (
    <Stack gap="lg" maw={860}>
      <DeviceConnectPanel />
    </Stack>
  );
}
