// The actions of the explorer's context menu, by what was clicked. Which ones are offered (and which are greyed out,
// with the reason) depends on the object's kind and on what the connection can do. Pure and tested.
import type { DbNodeKind } from "./types";

export type ActionId =
  | "connect"
  | "disconnect"
  | "new-query"
  | "refresh"
  | "edit"
  | "delete"
  | "copy-name"
  | "copy-qualified"
  | "open-data"
  | "open-structure"
  | "open-ddl"
  | "select-query"
  | "export"
  | "import";

export interface MenuItem {
  id: ActionId;
  label: string;
  group: number;
  enabled: boolean;
  why?: string;
  danger?: boolean;
}

export interface MenuContext {
  /** What was clicked: a connection (`connection`) or a node of this kind. */
  target: "connection" | DbNodeKind;
  connected: boolean;
  /** The engine's console takes SQL. */
  sql: boolean;
  /** A table can be paged, exported and (with `changes`) imported into: PostgreSQL and MySQL. */
  transfer: boolean;
  changes: boolean;
  readOnly: boolean;
}

export function menuFor(c: MenuContext): MenuItem[] {
  const items: MenuItem[] = [];
  const add = (id: ActionId, label: string, group: number, extra: Partial<MenuItem> = {}) => items.push({ id, label, group, enabled: true, ...extra });

  if (c.target === "connection") {
    add(c.connected ? "disconnect" : "connect", c.connected ? "Disconnect" : "Connect", 0);
    add("new-query", "New query", 0, c.connected ? {} : { enabled: false, why: "Connect first." });
    add("refresh", "Refresh", 0, c.connected ? {} : { enabled: false, why: "Connect first." });
    add("copy-name", "Copy name", 1);
    add("edit", "Edit connection…", 2);
    add("delete", "Delete connection…", 2, { danger: true });
    return items;
  }

  const table = c.target === "table" || c.target === "view";
  if (table) {
    add("open-data", "Open data", 0);
    add("open-structure", "Open structure", 0, c.sql ? {} : { enabled: false, why: "This database type has no structure view." });
    add("open-ddl", "Show definition", 0, c.sql ? {} : { enabled: false, why: "This database type has no definition to show." });
    add("select-query", "New SELECT query", 0);
    add("copy-name", "Copy name", 1);
    add("copy-qualified", "Copy qualified name", 1);
    add("refresh", "Refresh", 2);
    add("export", "Export rows…", 3, c.transfer ? {} : { enabled: false, why: "Exporting a table is available for PostgreSQL and MySQL." });
    if (c.target === "table") {
      add("import", "Import CSV…", 3, !c.transfer || !c.changes ? { enabled: false, why: "Importing is available for PostgreSQL and MySQL." } : c.readOnly ? { enabled: false, why: "This connection is read-only." } : {});
    }
    return items;
  }

  if (c.target === "database" || c.target === "schema" || c.target === "folder") {
    add("new-query", "New query", 0);
    add("refresh", "Refresh", 0);
    add("copy-name", "Copy name", 1);
    return items;
  }

  add("copy-name", "Copy name", 0);
  add("copy-qualified", "Copy qualified name", 0);
  return items;
}
