import { useMutation, useQueryClient } from "@tanstack/react-query";
import { selectMonitorDevice } from "../api/monitor";
import type { DiscoveredMonitorDevice } from "../api/monitor";
import { monitorKeys } from "../queries/monitor";

export function useConnectDevice() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (device: DiscoveredMonitorDevice) =>
      selectMonitorDevice(device),
    onMutate: async () => {
      await Promise.all([
        queryClient.cancelQueries({ queryKey: monitorKeys.profiles() }),
        queryClient.cancelQueries({ queryKey: monitorKeys.images() }),
      ]);
    },
    onSuccess: (data) => {
      queryClient.setQueryData(monitorKeys.profiles(), []);
      queryClient.setQueryData(monitorKeys.images(), []);
      queryClient.setQueryData(monitorKeys.settings(), data);
      void queryClient.invalidateQueries({
        queryKey: monitorKeys.profiles(),
      });
      void queryClient.invalidateQueries({
        queryKey: monitorKeys.images(),
      });
    },
  });
}
