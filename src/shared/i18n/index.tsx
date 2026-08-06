import { useEffect } from "react";
import type { PropsWithChildren } from "react";
import { useAtom } from "jotai";
import { useTranslation } from "react-i18next";
import {
  languagePreferenceAtom,
  type LanguagePreference,
} from "../state/ui";
import type { TranslationKey } from "./en";
import i18next from "./i18next";
import { RustCommandError } from "../tauri/invoke-command";

export type ResolvedLanguage = "zh-CN" | "en";
type Variables = Record<string, string | number>;

function systemLanguage(): ResolvedLanguage {
  const languages = typeof navigator === "undefined"
    ? []
    : [navigator.language, ...(navigator.languages ?? [])];
  return languages.some((language) => language.toLowerCase().startsWith("zh"))
    ? "zh-CN"
    : "en";
}

export function resolveLanguage(
  preference: LanguagePreference,
): ResolvedLanguage {
  if (preference === "zh-CN" || preference === "en") return preference;
  return systemLanguage();
}

export function I18nProvider({ children }: PropsWithChildren) {
  const [preference] = useAtom(languagePreferenceAtom);

  useEffect(() => {
    const applyResolvedLanguage = () => {
      const resolved = resolveLanguage(preference);
      void i18next.changeLanguage(resolved);
      document.documentElement.lang = resolved;
    };
    applyResolvedLanguage();
    if (preference === "system") {
      window.addEventListener("languagechange", applyResolvedLanguage);
      return () => window.removeEventListener("languagechange", applyResolvedLanguage);
    }
  }, [preference]);

  return <>{children}</>;
}

interface I18nValue {
  language: ResolvedLanguage;
  preference: LanguagePreference;
  setPreference: (preference: LanguagePreference) => void;
  t: (key: TranslationKey, variables?: Variables) => string;
}

export function useI18n(): I18nValue {
  const [preference, setPreference] = useAtom(languagePreferenceAtom);
  const { t } = useTranslation();

  return {
    language: resolveLanguage(preference),
    preference,
    setPreference,
    t: (key, variables) => t(key, variables),
  };
}

// 统一的错误展示辅助：Rust 命令失败时抛出的 RustCommandError 按当前界面语言
// 翻译；其余错误（网络层、JS 运行时异常等）直接使用其自身 message。
export function describeError(error: unknown, t: I18nValue["t"]): string {
  if (error instanceof RustCommandError) {
    return t(error.code as TranslationKey, error.params);
  }
  if (error instanceof Error) return error.message;
  return String(error);
}
