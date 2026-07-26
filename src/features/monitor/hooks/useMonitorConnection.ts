import { useQuery } from "@tanstack/react-query";
import { monitorDevicesQuery, monitorSettingsQuery } from "../queries/monitor";

export function useMonitorConnection() {
  const settings = useQuery(monitorSettingsQuery);
  const devices = useQuery(monitorDevicesQuery);

  const connectedDevice = devices.data?.find(
    (device) => device.id === settings.data?.deviceId,
  );
  const otherDevices = (devices.data ?? []).filter(
    (device) => device.id !== connectedDevice?.id,
  );

  return {
    settings,
    devices,
    connectedDevice,
    otherDevices,
    isPending: settings.isPending || devices.isPending,
    hasConfiguredDevice: Boolean(settings.data?.deviceId),
    hasAvailableDevice: (devices.data?.length ?? 0) > 0,
  };
}
