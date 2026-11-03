// SQL formatting that follows the engine's dialect. The formatter only rearranges whitespace and keyword case; it
// never changes what the statement does. An engine that isn't SQL has no formatter.
import { format, type SqlLanguage } from "sql-formatter";

const LANGUAGE: Record<string, SqlLanguage> = {
  postgres: "postgresql",
  mysql: "mysql",
  mssql: "transactsql",
  oracle: "plsql",
  rqlite: "sqlite",
};

export const canFormat = (engine: string) => engine in LANGUAGE;

/** The text formatted, or null (with a reason) when it can't be. */
export function formatSql(engine: string, sql: string): { text: string } | { error: string } {
  const language = LANGUAGE[engine];
  if (!language) return { error: "This database type has no SQL to format." };
  try {
    return { text: format(sql, { language, keywordCase: "upper", tabWidth: 2, linesBetweenQueries: 1 }) };
  } catch (e) {
    return { error: `Couldn't format this: ${e instanceof Error ? e.message.split("\n")[0] : String(e)}` };
  }
}
