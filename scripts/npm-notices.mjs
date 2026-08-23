#!/usr/bin/env node
// Attribution for the npm packages that ship inside the webview bundle.
//
// Prints markdown on stdout; `generate-notices.sh` is what assembles it into
// LICENSES/NOTICE-third-party.md. Deliberately dependency-free -- a notice
// generator that itself needs a package to be installed is one more thing that
// can be out of date when the notice is regenerated.
//
// Every *production* dependency is attributed, resolved from the tree npm
// reports rather than from `package.json` alone, so transitive packages are
// covered too. Vite tree-shakes, so some of these contribute no bytes to the
// final bundle -- `@types/react` contributes nothing but types. Attributing
// them anyway is deliberate: over-attribution costs a paragraph, and
// under-attribution is the failure that matters.

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const MODULES = path.join(ROOT, "node_modules");

/** Every production dependency in the tree, deduplicated by name@version. */
function productionDependencies() {
  const raw = execFileSync(
    "npm",
    ["ls", "--omit=dev", "--all", "--json"],
    { cwd: ROOT, encoding: "utf8", maxBuffer: 32 * 1024 * 1024 },
  );
  const seen = new Map();

  const walk = (node) => {
    for (const [name, info] of Object.entries(node.dependencies ?? {})) {
      const version = info.version;
      // An unmet optional peer: npm lists it with no version because it is not
      // installed, so no code of its ships and there is nothing to attribute.
      if (!version) {
        continue;
      }
      const key = `${name}@${version}`;
      if (!seen.has(key)) {
        seen.set(key, { name, version });
        walk(info);
      }
    }
  };

  walk(JSON.parse(raw));
  return [...seen.values()].sort((a, b) => a.name.localeCompare(b.name));
}

/**
 * Where a package actually lives.
 *
 * npm's own output carries no path, and the tree is mostly but not entirely
 * flat, so the hoisted copy is checked first and nested copies second. A
 * package that cannot be found is reported rather than skipped -- a notice
 * file that quietly omits a dependency is worse than one that fails loudly.
 */
function locate(name, version) {
  const hoisted = path.join(MODULES, name);
  if (existsSync(path.join(hoisted, "package.json"))) {
    const manifest = JSON.parse(
      readFileSync(path.join(hoisted, "package.json"), "utf8"),
    );
    if (manifest.version === version) {
      return hoisted;
    }
  }

  // Scope directories (`@tauri-apps`) hold packages rather than being one, so
  // the search has to descend through them -- the second copy of
  // `@tauri-apps/api` lives at
  // `node_modules/@tauri-apps/plugin-shell/node_modules/@tauri-apps/api`, and
  // a search that only looked one level down declared it missing.
  const owners = readdirSync(MODULES).flatMap((entry) =>
    entry.startsWith("@")
      ? readdirSync(path.join(MODULES, entry)).map((child) =>
          path.join(entry, child),
        )
      : [entry],
  );

  for (const owner of owners) {
    const nested = path.join(MODULES, owner, "node_modules", name);
    if (!existsSync(path.join(nested, "package.json"))) {
      continue;
    }
    const manifest = JSON.parse(
      readFileSync(path.join(nested, "package.json"), "utf8"),
    );
    if (manifest.version === version) {
      return nested;
    }
  }

  return null;
}

/** The licence text a package ships, if it ships one. */
function licenseText(directory) {
  const candidates = readdirSync(directory).filter((file) =>
    /^(LICENSE|LICENCE|COPYING|NOTICE)(\..*)?$/i.test(file),
  );
  if (candidates.length === 0) {
    return null;
  }
  return candidates
    .map((file) => readFileSync(path.join(directory, file), "utf8").trim())
    .join("\n\n");
}

function attribution(name, version) {
  const directory = locate(name, version);
  if (!directory) {
    throw new Error(
      `cannot find ${name}@${version} in node_modules -- run \`npm install\` before generating notices`,
    );
  }

  const manifest = JSON.parse(
    readFileSync(path.join(directory, "package.json"), "utf8"),
  );
  const declared =
    typeof manifest.license === "string"
      ? manifest.license
      : (manifest.license?.type ?? manifest.licenses?.[0]?.type ?? "see below");

  const lines = [`### ${name} ${version}`, "", `License: ${declared}`];
  if (manifest.homepage) {
    lines.push("", manifest.homepage);
  }

  const text = licenseText(directory);
  if (text) {
    lines.push("", "```text", text, "```");
  } else {
    // Said out loud rather than left blank: a package declaring MIT while
    // shipping no notice file is exactly the case a human has to look at.
    lines.push(
      "",
      `> This package declares \`${declared}\` but ships no licence file. ` +
        "Its notice is the declaration above.",
    );
  }

  return lines.join("\n");
}

const packages = productionDependencies();
const sections = packages.map(({ name, version }) => attribution(name, version));

process.stdout.write(
  [
    "## JavaScript dependencies",
    "",
    `${packages.length} production packages, resolved from the installed tree.`,
    "",
    sections.join("\n\n"),
    "",
  ].join("\n"),
);
