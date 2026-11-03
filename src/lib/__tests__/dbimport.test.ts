import { describe, expect, it } from "vitest";
import { autoMapping } from "../dbimport";

describe("autoMapping", () => {
  it("matches by name, ignoring case, spaces, dashes and underscores", () => {
    expect(autoMapping(["ID", "First Name", "e-mail", "extra"], ["id", "first_name", "email", "created"])).toEqual(["id", "first_name", "email", null]);
  });
  it("uses a table column once, and leaves blanks unmapped", () => {
    expect(autoMapping(["name", "Name", ""], ["name"])).toEqual(["name", null, null]);
  });
  it("maps nothing for a table it knows nothing about", () => {
    expect(autoMapping(["a", "b"], [])).toEqual([null, null]);
  });
});
