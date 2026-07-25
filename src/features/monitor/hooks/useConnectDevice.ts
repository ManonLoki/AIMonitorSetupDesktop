import { useMutation, useQueryClient } from "@tanstack/react-query";
import { saveMonitorSettings } from "../api/monitor";
import type { DiscoveredMonitorDevice } from "../api/monitor";
import { monitorKeys } from "../queries/monitor";

export function useConnectDevice() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      device,
      username,
    }: {
      device: DiscoveredMonitorDevice;
      username: string;
    }) => saveMonitorSettings(device, username),
    onSuccess: (data) => {
      queryClient.setQueryData(monitorKeys.settings(), data);
    },
  });
}
