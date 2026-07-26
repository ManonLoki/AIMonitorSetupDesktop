import {
  ActionIcon,
  Badge,
  Button,
  Group,
  Paper,
  Progress,
  Stack,
  Text,
  ThemeIcon,
  Title,
  Tooltip,
} from "@mantine/core";
import { useNavigate } from "@tanstack/react-router";
import { useAtom, useAtomValue, useSetAtom } from "jotai";
import { useEffect, useState } from "react";
import {
  onboardingCompletedAtom,
  onboardingOpenAtom,
} from "../state/ui";
import { LineIcon } from "./LineIcon";

const steps = [
  {
    title: "设置 AI 客户端与 Hooks",
    route: "/settings" as const,
    routeLabel: "前往设置",
    items: [
      "在“AI 客户端”中勾选要使用的客户端并保存。",
      "在“Hooks 管理”中确认配置目录，然后写入 Hooks 配置。",
      "可选：在“通用设置”中修改显示用户名及其他偏好。",
    ],
  },
  {
    title: "上传展示图片",
    route: "/images" as const,
    routeLabel: "前往图片管理",
    items: [
      "上传至少一张 JPEG、PNG 或 GIF 图片。",
      "这些图片会在下一步分配给不同的 AI 行为状态。",
    ],
  },
  {
    title: "配置监控展示",
    route: "/ai-management" as const,
    routeLabel: "前往监控管理",
    items: [
      "为当前 AI 客户端选择显示位置。",
      "分别为“空闲、运行中、询问、异常”四种行为选择图片。",
      "确认四种行为均已配置后，保存展示配置。",
    ],
  },
];

export function OnboardingGuide() {
  const navigate = useNavigate();
  const completed = useAtomValue(onboardingCompletedAtom);
  const setCompleted = useSetAtom(onboardingCompletedAtom);
  const [opened, setOpened] = useAtom(onboardingOpenAtom);
  const [step, setStep] = useState(0);

  useEffect(() => {
    if (!completed) {
      setStep(0);
      setOpened(true);
    }
  }, [completed, setOpened]);

  if (!opened) return null;

  const current = steps[step];
  const finish = () => {
    setCompleted(true);
    setOpened(false);
  };

  return (
    <Paper
      withBorder
      shadow="xl"
      radius="lg"
      p="md"
      className="onboarding-guide"
      role="dialog"
      aria-label="AIMonitor 新手引导"
    >
      <Stack gap="sm">
        <Group justify="space-between" align="flex-start" wrap="nowrap">
          <Group gap="sm" wrap="nowrap">
            <ThemeIcon radius="md" variant="light" size="lg">
              <LineIcon name="guide" size={20} />
            </ThemeIcon>
            <div>
              <Group gap="xs">
                <Title order={4}>新手引导</Title>
                <Badge variant="light" color="violet">
                  {step + 1} / {steps.length}
                </Badge>
              </Group>
              <Text size="xs" c="dimmed" mt={2}>
                按顺序完成首次配置；引导关闭后不会再次自动出现。
              </Text>
            </div>
          </Group>
          <Tooltip label="跳过并不再自动显示">
            <ActionIcon
              variant="subtle"
              color="gray"
              aria-label="跳过新手引导"
              onClick={finish}
            >
              <LineIcon name="close" size={17} />
            </ActionIcon>
          </Tooltip>
        </Group>

        <Progress value={((step + 1) / steps.length) * 100} size="xs" />

        <div>
          <Text fw={700} size="sm">
            步骤 {step + 1}：{current.title}
          </Text>
          <Stack gap={5} mt="xs">
            {current.items.map((item, index) => (
              <Group key={item} gap="xs" align="flex-start" wrap="nowrap">
                <Badge
                  size="xs"
                  circle
                  variant="light"
                  color={step === 0 && index === 2 ? "gray" : "violet"}
                >
                  {index + 1}
                </Badge>
                <Text size="xs" lh={1.45}>
                  {item}
                </Text>
              </Group>
            ))}
          </Stack>
        </div>

        <Button
          size="xs"
          variant="light"
          onClick={() => navigate({ to: current.route })}
        >
          {current.routeLabel}
        </Button>

        <Group justify="space-between" gap="xs">
          <Button
            size="xs"
            variant="default"
            disabled={step === 0}
            onClick={() => setStep((value) => value - 1)}
          >
            上一步
          </Button>
          {step === steps.length - 1 ? (
            <Button size="xs" onClick={finish}>
              完成引导
            </Button>
          ) : (
            <Button size="xs" onClick={() => setStep((value) => value + 1)}>
              下一步
            </Button>
          )}
        </Group>
      </Stack>
    </Paper>
  );
}
