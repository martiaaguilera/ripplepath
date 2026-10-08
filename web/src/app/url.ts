// Navigation state lives in the URL so every view, selection and file is a shareable link and the
// browser's back button works. A router library would add a dependency to manage six flat views;
// URLSearchParams and the History API already do everything needed here.

import { useCallback, useEffect, useState } from "react";

export const VIEWS = ["overview", "diff", "tests", "risk", "architecture", "owners"] as const;
export type View = (typeof VIEWS)[number];

export const VIEW_LABEL: Record<View, string> = {
  overview: "Change",
  diff: "Diff",
  tests: "Tests",
  risk: "Risk & policy",
  architecture: "Architecture",
  owners: "Owners",
};

export interface UrlState {
  base: string;
  head: string;
  view: View;
  /** Selected symbol id (overview inspector, graph highlight). */
  sel: string | null;
  /** Selected file path (diff view). */
  file: string | null;
  /** Selected test id (tests view). */
  test: string | null;
  /** Head line to scroll to and mark in the diff view. */
  line: number | null;
}

function isView(value: string | null): value is View {
  return value !== null && (VIEWS as readonly string[]).includes(value);
}

function positiveInt(value: string | null): number | null {
  if (value === null || !/^\d{1,9}$/.test(value)) return null;
  const parsed = Number(value);
  return parsed > 0 ? parsed : null;
}

export function parseUrl(search: string): UrlState {
  const params = new URLSearchParams(search);
  const view = params.get("view");
  return {
    base: params.get("base") ?? "",
    head: params.get("head") ?? "",
    view: isView(view) ? view : "overview",
    sel: params.get("sel"),
    file: params.get("file"),
    test: params.get("test"),
    line: positiveInt(params.get("line")),
  };
}

/** Fixed parameter order, defaults omitted: equal states always produce equal URLs. */
export function toSearch(state: UrlState): string {
  const params = new URLSearchParams();
  if (state.base) params.set("base", state.base);
  if (state.head) params.set("head", state.head);
  if (state.view !== "overview") params.set("view", state.view);
  if (state.sel) params.set("sel", state.sel);
  if (state.file) params.set("file", state.file);
  if (state.test) params.set("test", state.test);
  if (state.line !== null) params.set("line", String(state.line));
  const search = params.toString();
  return search ? `?${search}` : "";
}

export type Navigate = (patch: Partial<UrlState>, options?: { replace?: boolean }) => void;

export function useUrlState(): [UrlState, Navigate] {
  const [state, setState] = useState(() => parseUrl(window.location.search));

  useEffect(() => {
    const onPop = () => {
      setState(parseUrl(window.location.search));
    };
    window.addEventListener("popstate", onPop);
    return () => {
      window.removeEventListener("popstate", onPop);
    };
  }, []);

  const navigate = useCallback<Navigate>((patch, options) => {
    // The URL is the source of truth, so the next state is derived from it rather than from a
    // possibly stale closure.
    const next = { ...parseUrl(window.location.search), ...patch };
    const search = toSearch(next);
    if (search !== window.location.search) {
      const url = `${window.location.pathname}${search}${window.location.hash}`;
      // Selections replace the entry, view changes push one: "back" should return to the
      // previous view, not replay every click inside it.
      if (options?.replace) window.history.replaceState(null, "", url);
      else window.history.pushState(null, "", url);
    }
    setState(next);
  }, []);

  return [state, navigate];
}

/** Same-document link target for a state patch, for real `<a href>` elements. */
export function hrefFor(current: UrlState, patch: Partial<UrlState>): string {
  return `${window.location.pathname}${toSearch({ ...current, ...patch })}`;
}
