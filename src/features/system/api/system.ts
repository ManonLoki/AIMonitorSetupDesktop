import { invokeCommand } from "../../../shared/tauri/invoke-command";

export interface SystemOverview {
  appName: string;
  version: string;
  backend: string;
  transport: string;
}

export function getSystemOverview(): Promise<SystemOverview> {
  return invokeCommand<SystemOverview>("get_system_overview");
}
