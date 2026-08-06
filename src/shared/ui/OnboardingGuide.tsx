import {
  ActionIcon,
  Button,
  Group,
  Paper,
  Stack,
  Stepper,
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
// 引入路由路径常量的唯一来源
import { ROUTES } from "../routes";
import { useI18n } from "../i18n";

type StepItemTone = "default" | "muted";

type OnboardingStep = {
  title: string;
  route?: string;
  routeLabel?: string;
  items: Array<{
    text: string;
    tone?: StepItemTone;
  }>;
};

export function OnboardingGuide() {
  const { t } = useI18n();
  const steps: OnboardingStep[] = [
    {
      title: t("onboarding.step1Title"),
      items: [
        { text: t("onboarding.step1Item1") },
        { text: t("onboarding.step1Item2") },
        { text: t("onboarding.step1Item3"), tone: "muted" },
      ],
    },
    {
      title: t("onboarding.step2Title"),
      route: ROUTES.settings,
      routeLabel: t("onboarding.step2Route"),
      items: [
        { text: t("onboarding.step2Item1") },
        { text: t("onboarding.step2Item2") },
        { text: t("onboarding.step2Item3"), tone: "muted" },
      ],
    },
    {
      title: t("onboarding.step3Title"),
      route: ROUTES.aiManagement,
      routeLabel: t("onboarding.step3Route"),
      items: [
        { text: t("onboarding.step3Item1") },
        { text: t("onboarding.step3Item2") },
        { text: t("onboarding.step3Item3") },
      ],
    },
  ];
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
      aria-label={t("onboarding.dialog")}
    >
      <Stack gap="sm">
        <Group justify="space-between" align="flex-start" wrap="nowrap">
          <Group gap="sm" wrap="nowrap">
            <ThemeIcon radius="md" variant="light" size="lg">
              <LineIcon name="guide" size={20} />
            </ThemeIcon>
            <div>
              <Title order={4}>{t("onboarding.title")}</Title>
              <Text size="xs" c="dimmed" mt={2}>
                {t("onboarding.description")}
              </Text>
            </div>
          </Group>
          <Tooltip label={t("onboarding.skipTooltip")}>
            <ActionIcon
              variant="subtle"
              color="gray"
              aria-label={t("onboarding.skipAria")}
              onClick={finish}
            >
              <LineIcon name="close" size={17} />
            </ActionIcon>
          </Tooltip>
        </Group>

        <Stepper active={step} onStepClick={setStep} size="xs" iconSize={22}>
          {steps.map((item) => (
            <Stepper.Step key={item.title} label={item.title}>
              <Stack gap={5} mt="xs">
                {item.items.map((entry) => (
                  <Text
                    key={entry.text}
                    size="xs"
                    c={entry.tone === "muted" ? "dimmed" : undefined}
                    lh={1.45}
                  >
                    {entry.text}
                  </Text>
                ))}
              </Stack>
            </Stepper.Step>
          ))}
        </Stepper>

        {current.route && current.routeLabel ? (
          <Button
            size="xs"
            variant="light"
            onClick={() => navigate({ to: current.route })}
          >
            {current.routeLabel}
          </Button>
        ) : null}

        <Group justify="space-between" gap="xs">
          <Button
            size="xs"
            variant="default"
            disabled={step === 0}
            onClick={() => setStep((value) => value - 1)}
          >
            {t("onboarding.previous")}
          </Button>
          {step === steps.length - 1 ? (
            <Button size="xs" onClick={finish}>
              {t("onboarding.finish")}
            </Button>
          ) : (
            <Button size="xs" onClick={() => setStep((value) => value + 1)}>
              {t("onboarding.next")}
            </Button>
          )}
        </Group>
      </Stack>
    </Paper>
  );
}
