import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { enMessages } from "./en";
import { zhCNMessages } from "./zh-CN";

// i18next 的默认插值语法就是 `{{name}}`，与既有词典的占位符格式一致，
// 迁移时无需改动 en.ts / zh-CN.ts 里的任何 key 或占位符。
void i18n.use(initReactI18next).init({
  resources: {
    en: { translation: enMessages },
    "zh-CN": { translation: zhCNMessages },
  },
  lng: "en",
  fallbackLng: "en",
  interpolation: { escapeValue: false },
  returnNull: false,
});

export default i18n;
