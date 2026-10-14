import { invoke, isTauri } from "@tauri-apps/api/core";
export const native = isTauri();
let chain: Promise<unknown> = Promise.resolve();
// Native operations share one worker slot; rapid selection changes cannot
// overlap decoding with export or cause a "busy" error in an active batch.
export function command<T>(
  name: string,
  args: Record<string, unknown>,
  current = () => true,
): Promise<T> {
  const task = chain
    .catch(() => {})
    .then(() => {
      if (!current()) throw new Error("Superseded preview");
      return invoke<T>(name, args);
    });
  chain = task.catch(() => {});
  return task;
}
export function cancelTask() {
  return invoke("cancel_task");
}
export function bytes(value: number) {
  if (value < 1024) return `${value} B`;
  if (value < 1024 ** 2) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / 1024 ** 2).toFixed(2)} MB`;
}
