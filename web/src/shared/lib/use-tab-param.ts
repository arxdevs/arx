import { useSearchParams } from "react-router-dom";

export function useTabParam(
  defaultTab: string,
): [string, (tab: string) => void] {
  const [params, setParams] = useSearchParams();
  const active = params.get("tab") ?? defaultTab;
  const setActive = (tab: string) => {
    setParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        if (tab === defaultTab) next.delete("tab");
        else next.set("tab", tab);
        return next;
      },
      { replace: true },
    );
  };
  return [active, setActive];
}
