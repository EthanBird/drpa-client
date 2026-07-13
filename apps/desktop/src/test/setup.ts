import "@testing-library/jest-dom/vitest";
import { vi } from "vitest";

vi.mock("@monaco-editor/react", () => ({
  default: () => null,
  loader: { config: vi.fn() },
}));

vi.mock("monaco-editor/esm/vs/editor/editor.api.js", () => ({}));
vi.mock("monaco-editor/esm/vs/editor/editor.worker.js?worker", () => ({
  default: class TestEditorWorker {},
}));
vi.mock("monaco-editor/esm/vs/basic-languages/python/python.contribution.js", () => ({}));
vi.mock("monaco-editor/esm/vs/basic-languages/yaml/yaml.contribution.js", () => ({}));
