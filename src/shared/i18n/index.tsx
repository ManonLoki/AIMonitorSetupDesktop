import { createContext, useContext, useEffect, useMemo, useState } from "react";
import type { PropsWithChildren } from "react";
import { useAtom } from "jotai";
import {
  languagePreferenceAtom,
  type LanguagePreference,
} from "../state/ui";
import { enMessages, type TranslationKey } from "./en";
import { zhCNMessages } from "./zh-CN";

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

function translate(
  language: ResolvedLanguage,
  key: TranslationKey,
  variables?: Variables,
): string {
  const messages = language === "zh-CN" ? zhCNMessages : enMessages;
  return Object.entries(variables ?? {}).reduce(
    (message, [name, value]) =>
      message.split(`{{${name}}}`).join(String(value)),
    messages[key] as string,
  );
}

interface I18nValue {
  language: ResolvedLanguage;
  preference: LanguagePreference;
  setPreference: (preference: LanguagePreference) => void;
  t: (key: TranslationKey, variables?: Variables) => string;
}

const I18nContext = createContext<I18nValue | null>(null);

export function I18nProvider({ children }: PropsWithChildren) {
  const [preference, setPreference] = useAtom(languagePreferenceAtom);
  const [language, setLanguage] = useState(() => resolveLanguage(preference));

  useEffect(() => {
    const update = () => setLanguage(resolveLanguage(preference));
    update();
    if (preference === "system") {
      window.addEventListener("languagechange", update);
      return () => window.removeEventListener("languagechange", update);
    }
  }, [preference]);

  useEffect(() => {
    document.documentElement.lang = language;
  }, [language]);

  const value = useMemo<I18nValue>(
    () => ({
      language,
      preference,
      setPreference,
      t: (key, variables) => translate(language, key, variables),
    }),
    [language, preference, setPreference],
  );

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nValue {
  const value = useContext(I18nContext);
  if (!value) throw new Error("useI18n must be used within I18nProvider");
  return value;
}
