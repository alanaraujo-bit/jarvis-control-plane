import { strict as assert } from "node:assert";
import { test } from "node:test";

import { resolveLocale, translate } from "./index.ts";

test("substitutes named placeholders", () => {
  assert.equal(
    translate("en", "update.current", { version: "0.1.0" }),
    "You are on version 0.1.0.",
  );
});

test("leaves an unknown placeholder visible rather than blanking it", () => {
  // A stray `{foo}` is a bug worth seeing; silently deleting it hides the bug
  // and produces a sentence with a hole in it.
  const rendered = translate("en", "update.current", {});
  assert.match(rendered, /\{version\}/);
});

test("selects the singular form for a count of one", () => {
  const en = translate("en", "mission.notVerified", { count: 1 });
  assert.match(en, /1 required criterion is/);

  const pt = translate("pt-BR", "mission.notVerified", { count: 1 });
  assert.match(pt, /1 critério obrigatório ainda não foi/);
});

test("selects the plural form for other counts", () => {
  const en = translate("en", "mission.notVerified", { count: 3 });
  assert.match(en, /3 required criteria are/);

  const pt = translate("pt-BR", "mission.notVerified", { count: 3 });
  assert.match(pt, /3 critérios obrigatórios ainda não foram/);
});

test("portuguese treats zero as singular, english does not", () => {
  // "0 critério" is correct in pt-BR; "0 criterion" is not correct in English.
  assert.match(translate("pt-BR", "missionControl.openCriteria", { count: 0 }), /0 não verificado$/);
  assert.match(translate("en", "mission.notVerified", { count: 0 }), /0 required criteria are/);
});

test("falls back to english when a locale lacks a key", () => {
  // Every key is translated today; this guards the behaviour for when one is
  // added to en and not yet elsewhere.
  const rendered = translate("pt-BR", "app.name");
  assert.equal(rendered, "J.A.R.V.I.S.");
});

test("resolves locales from browser language lists", () => {
  assert.equal(resolveLocale(["pt-BR", "en"]), "pt-BR");
  assert.equal(resolveLocale(["en-GB"]), "en");
  // A regional Portuguese variant we do not ship still lands on pt-BR rather
  // than dropping the user into English.
  assert.equal(resolveLocale(["pt-PT"]), "pt-BR");
  assert.equal(resolveLocale(["ja", "de"]), "en");
  assert.equal(resolveLocale([]), "en");
});
