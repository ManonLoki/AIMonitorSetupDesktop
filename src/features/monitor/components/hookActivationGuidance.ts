import type { useI18n } from "../../../shared/i18n";
import type { TranslationKey } from "../../../shared/i18n/en";
import type { HookWriteOutcome } from "../api/monitor";

type Translate = ReturnType<typeof useI18n>["t"];

interface OutcomePresentationKeys {
  badge: TranslationKey;
  guidance: TranslationKey | null;
  title: TranslationKey | null;
  color: "blue" | "yellow";
}

const outcomePresentationKeys = {
  unchanged: {
    badge: "hooks.unchanged",
    guidance: "hooks.guidanceUnchanged",
    title: "hooks.unchangedTitle",
    color: "blue",
  },
  active: {
    badge: "hooks.written",
    guidance: null,
    title: null,
    color: "blue",
  },
  restartRequired: {
    badge: "hooks.written",
    guidance: "hooks.guidanceRestart",
    title: "hooks.reloadTitle",
    color: "yellow",
  },
  codexReviewRequired: {
    badge: "hooks.writtenReview",
    guidance: "hooks.guidanceCodex",
    title: "hooks.trustTitle",
    color: "yellow",
  },
  workBuddyReviewRequired: {
    badge: "hooks.writtenReview",
    guidance: "hooks.guidanceWorkBuddy",
    title: "hooks.trustTitle",
    color: "yellow",
  },
  codeBuddyReviewRequired: {
    badge: "hooks.writtenReview",
    guidance: "hooks.guidanceCodeBuddy",
    title: "hooks.trustTitle",
    color: "yellow",
  },
  hermesEnableRequired: {
    badge: "hooks.writtenReview",
    guidance: "hooks.guidanceHermes",
    title: "hooks.trustTitle",
    color: "yellow",
  },
  openClawEnableRequired: {
    badge: "hooks.writtenReview",
    guidance: "hooks.guidanceOpenClaw",
    title: "hooks.trustTitle",
    color: "yellow",
  },
} satisfies Record<HookWriteOutcome, OutcomePresentationKeys>;

// Rust 已经决定唯一 outcome；这里仅把类型化结果映射为本地化展示文案。
export function hookActivationPresentation(
  outcome: HookWriteOutcome,
  toolLabel: string,
  filename: string,
  t: Translate,
): { badge: string; guidance: string | null; title: string | null; color: "blue" | "yellow" } {
  const keys = outcomePresentationKeys[outcome];
  return {
    badge: t(keys.badge),
    guidance: keys.guidance
      ? t(keys.guidance, { filename, tool: toolLabel })
      : null,
    title: keys.title ? t(keys.title, { tool: toolLabel }) : null,
    color: keys.color,
  };
}
