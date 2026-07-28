import { en } from "./locales/en";

type WidenMessages<T> = T extends string
  ? string
  : T extends (...arguments_: infer Arguments) => unknown
    ? (...arguments_: Arguments) => string
    : { readonly [Key in keyof T]: WidenMessages<T[Key]> };

export type Messages = WidenMessages<typeof en>;

const catalogs = { en } satisfies Record<string, Messages>;

export type SupportedLocale = keyof typeof catalogs;

export function resolveLocale(languages: readonly string[] = []): SupportedLocale {
  for (const language of languages) {
    const baseLanguage = language.toLowerCase().split("-")[0];
    if (baseLanguage in catalogs) return baseLanguage as SupportedLocale;
  }
  return "en";
}

export function messagesFor(locale: SupportedLocale): Messages {
  return catalogs[locale];
}

export function browserMessages(): Messages {
  const languages = typeof navigator === "undefined" ? [] : navigator.languages;
  return messagesFor(resolveLocale(languages));
}
