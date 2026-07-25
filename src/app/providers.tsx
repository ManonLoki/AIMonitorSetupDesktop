import { MantineProvider, createTheme } from "@mantine/core";
import { QueryClientProvider } from "@tanstack/react-query";
import type { PropsWithChildren } from "react";
import { useAtomValue } from "jotai";
import { queryClient } from "./query-client";
import { colorSchemeAtom } from "../shared/state/ui";
import { useMonitorDeviceEvents } from "../features/monitor/hooks/useMonitorDeviceEvents";

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

export function AppProviders({ children }: PropsWithChildren) {
  const colorScheme = useAtomValue(colorSchemeAtom);

  return (
    <QueryClientProvider client={queryClient}>
      <MantineProvider theme={theme} forceColorScheme={colorScheme}>
        <MonitorDeviceEvents />
        {children}
      </MantineProvider>
    </QueryClientProvider>
  );
}

function MonitorDeviceEvents() {
  useMonitorDeviceEvents();
  return null;
}
