import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { Dispatch, SetStateAction } from "react";
import { useState } from "react";
import {
  uploadRemoteImages,
  type AiProfileDraft,
  type AiTool,
  type HookBehavior,
  type MonitorDeviceSnapshot,
} from "../api/monitor";
import { monitorKeys } from "../queries/monitor";

type ProfileDrafts = Partial<Record<AiTool, AiProfileDraft>>;

interface DirectImageUpload {
  file: File;
  tool: AiTool;
  behavior: HookBehavior;
  deviceId: string;
}

interface UseDirectImageUploadOptions {
  setDrafts: Dispatch<SetStateAction<ProfileDrafts>>;
  onDraftChange: () => void;
}

// 管理监控页图片选择器的单图上传、设备切换竞态保护与 Profile 草稿回填。
export function useDirectImageUpload({
  setDrafts,
  onDraftChange,
}: UseDirectImageUploadOptions) {
  const queryClient = useQueryClient();
  const [uploadedSelection, setUploadedSelection] = useState<{
    tool: AiTool;
    filename: string;
    deviceId: string;
  } | null>(null);

  const upload = useMutation({
    // 复用批量上传 API，但图片选择器始终只传一个文件。
    mutationFn: ({ file, deviceId }: DirectImageUpload) =>
      uploadRemoteImages([file], deviceId),
    onMutate: () => setUploadedSelection(null),
    onSuccess: async (filenames, variables) => {
      await queryClient.invalidateQueries({ queryKey: monitorKeys.images() });
      const [filename] = filenames;
      if (!filename) return;

      // 上传期间若切换了设备，不把旧设备图片名写进新设备的草稿。
      const currentSnapshot = queryClient.getQueryData<MonitorDeviceSnapshot>(
        monitorKeys.devices(),
      );
      if (currentSnapshot?.selectedDeviceId !== variables.deviceId) return;

      // 必须使用设备返回的最终文件名；BMP/WebP 可能已由 Rust 转为 PNG/GIF。
      onDraftChange();
      setDrafts((current) => {
        const toolDraft = current[variables.tool];
        if (!toolDraft) return current;
        return {
          ...current,
          [variables.tool]: {
            ...toolDraft,
            hooks: toolDraft.hooks.map((item) =>
              item.behavior === variables.behavior
                ? { ...item, image: filename }
                : item,
            ),
          },
        };
      });
      setUploadedSelection({
        tool: variables.tool,
        filename,
        deviceId: variables.deviceId,
      });
    },
  });

  return {
    upload,
    uploadedSelection,
    clearUploadedSelection: () => setUploadedSelection(null),
  };
}
