import "@testing-library/jest-dom/vitest";

// jsdom does no layout, so it has no scrollIntoView; the views call it to keep keyboard focus and
// diff anchors in view. A no-op is faithful enough: nothing under test depends on scroll position.
if (!("scrollIntoView" in Element.prototype)) {
  Object.defineProperty(Element.prototype, "scrollIntoView", { value: () => undefined, writable: true });
}
