// Copying and saving what is on screen: the loaded rows of a result as CSV, TSV or JSON. (A whole table is exported
// by the backend in chunks; see the Export dialog.)
import { save } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import * as api from "$lib/api";
import { toCsv, toJson, toTsv } from "$lib/dbdata";
import { ui } from "$lib/stores/ui.svelte";
import { errorMessage, type DbQueryResult } from "$lib/types";

export type CopyFormat = "csv" | "tsv" | "json";

const text = (r: Pick<DbQueryResult, "columns" | "rows">, fmt: CopyFormat) => (fmt === "csv" ? toCsv(r) : fmt === "tsv" ? toTsv(r) : toJson(r));

export async function copyResult(r: DbQueryResult, fmt: CopyFormat) {
  try {
    await writeText(text(r, fmt));
    ui.notify("info", `Copied ${r.rows.length.toLocaleString()} rows as ${fmt.toUpperCase()}.`);
  } catch (e) {
    ui.notify("error", errorMessage(e));
  }
}

export async function saveResult(r: DbQueryResult, fmt: "csv" | "json", name: string) {
  const path = await save({
    title: "Export the result",
    defaultPath: `${name.replace(/[^\w.-]+/g, "_")}.${fmt}`,
    filters: [{ name: fmt.toUpperCase(), extensions: [fmt] }],
  });
  if (!path) return;
  try {
    await api.db.saveExport(path, text(r, fmt));
    ui.notify("info", `Saved ${r.rows.length.toLocaleString()} rows to ${path}.`);
  } catch (e) {
    ui.notify("error", errorMessage(e));
  }
}
