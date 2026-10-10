// @fframes/editor bundles its own copy of mediabunny, and we also import
// mediabunny for video probing/export. Two instances load, and the second one
// emits a benign "loaded twice" console warning. The two copies never share
// objects (fframes uses its bundle internally; we use ours for export), so the
// warning is noise. Filter exactly that message — every other console.error
// still surfaces.
const originalError = console.error.bind(console);

console.error = (...args: unknown[]): void => {
  const first = args[0];
  if (typeof first === "string" && first.includes("Mediabunny was loaded twice")) {
    return;
  }
  originalError(...args);
};
// Marker so tests can confirm this filter was installed (and ran before any
// mediabunny load) without needing to read the real console stream.
(console.error as { __rscapsQuiet?: boolean }).__rscapsQuiet = true;
