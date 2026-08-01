import { useEffect, useRef, type RefObject } from "react";

const focusableSelector = [
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "a[href]",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

export function useModalFocus<T extends HTMLElement>(
  onEscape?: () => void,
  enabled = true,
): RefObject<T | null> {
  const ref = useRef<T>(null);
  const onEscapeRef = useRef(onEscape);

  useEffect(() => {
    onEscapeRef.current = onEscape;
  }, [onEscape]);

  useEffect(() => {
    if (!enabled || !ref.current) return;
    const dialog = ref.current;
    const previous = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : undefined;
    const focusable = () =>
      Array.from(dialog.querySelectorAll<HTMLElement>(focusableSelector))
        .filter((element) => !element.hidden && element.getAttribute("aria-hidden") !== "true");
    const elements = focusable();
    const preferred = dialog.querySelector<HTMLElement>("[data-modal-initial-focus]");
    (preferred ?? elements[0] ?? dialog).focus();
    const handleKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape" && onEscapeRef.current) {
        event.preventDefault();
        onEscapeRef.current();
        return;
      }
      if (event.key !== "Tab") return;
      const current = focusable();
      if (current.length === 0) {
        event.preventDefault();
        dialog.focus();
        return;
      }
      const first = current[0];
      const last = current[current.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    dialog.addEventListener("keydown", handleKeyDown);
    return () => {
      dialog.removeEventListener("keydown", handleKeyDown);
      previous?.focus();
    };
  }, [enabled]);

  return ref;
}
