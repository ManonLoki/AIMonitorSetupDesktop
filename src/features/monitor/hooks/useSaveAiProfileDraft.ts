import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { Dispatch, SetStateAction } from "react";
import {
  saveAiProfileDraft,
  type AiProfileDraft,
  type AiTool,
  type MonitorDeviceSnapshot,
} from "../api/monitor";
import { monitorKeys } from "../queries/monitor";

type ProfileDrafts = Partial<Record<AiTool, AiProfileDraft>>;

// 保存结果只回写发起保存时的设备与草稿，避免迟到响应覆盖设备切换或后续编辑。
export function useSaveAiProfileDraft(
  setDrafts: Dispatch<SetStateAction<ProfileDrafts>>,
) {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      profile,
      expectedDeviceId,
    }: {
      profile: AiProfileDraft;
      expectedDeviceId: string;
    }) => saveAiProfileDraft(profile, expectedDeviceId),
    onSuccess: (saved, variables) => {
      const currentSnapshot =
        queryClient.getQueryData<MonitorDeviceSnapshot>(monitorKeys.devices());
      if (currentSnapshot?.selectedDeviceId === variables.expectedDeviceId) {
        setDrafts((current) =>
          current[saved.tool] === variables.profile
            ? { ...current, [saved.tool]: saved }
            : current,
        );
      }
      return queryClient.invalidateQueries({
        queryKey: monitorKeys.profiles(),
      });
    },
  });
}
