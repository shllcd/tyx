import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

const manifest = "Cargo.toml";
const changelogFile = "CHANGELOG.md";
const notesFile = process.env.NOTES_FILE ?? "notes.md";

const read = (path) => readFileSync(path, "utf8");
const write = (path, content) => writeFileSync(path, content);

const tagExists = (version) => {
  try {
    execFileSync("git", ["rev-parse", "-q", "--verify", `refs/tags/v${version}`], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
};

const currentVersion = (manifestText) => {
  const match = /^version = "([^"]+)"$/m.exec(manifestText);
  if (!match) throw new Error("no version in [workspace.package]");
  return match[1];
};

const unreleased = (changelog) => {
  const match = /^## \[Unreleased\][^\n]*\n([\s\S]*?)(?=^## \[|(?![\s\S]))/m.exec(changelog);
  if (!match) throw new Error(`${changelogFile} has no ## [Unreleased] section`);
  const notes = match[1].trim();
  if (!/^\s*[-*] /m.test(notes)) throw new Error("## [Unreleased] has no entries");
  return { match, notes };
};

const bumpLevel = (notes) => {
  const headings = [...notes.matchAll(/^### (.+)$/gm)].map(([, heading]) => heading.trim().toLowerCase());
  if (headings.some((heading) => /breaking|removed/.test(heading))) return "major";
  if (headings.some((heading) => /added|changed|deprecated|features?/.test(heading))) return "minor";
  return "patch";
};

const nextVersion = (current, level) => {
  const [major, minor, patch] = current.split(".").map(Number);
  const effective = major === 0 && level === "major" ? "minor" : level;
  if (effective === "major") return `${major + 1}.0.0`;
  if (effective === "minor") return `${major}.${minor + 1}.0`;
  return `${major}.${minor}.${patch + 1}`;
};

const changelog = read(changelogFile);
const manifestText = read(manifest);
const { match, notes } = unreleased(changelog);
const version = nextVersion(currentVersion(manifestText), bumpLevel(notes));
if (tagExists(version)) throw new Error(`tag v${version} already exists`);

const versioned = manifestText
  .replace(/^version = "[^"]+"$/m, `version = "${version}"`)
  .replace(/(path = "[^"]+")(?:, version = "[^"]+")?/g, `$1, version = "${version}"`);
const pathDependencies = manifestText.match(/path = "[^"]+"/g)?.length ?? 0;
const versionedDependencies = versioned.match(/path = "[^"]+", version = "[^"]+"/g)?.length ?? 0;
if (versionedDependencies !== pathDependencies) throw new Error(`versioned ${versionedDependencies} of ${pathDependencies} path dependencies`);
write(manifest, versioned);

const section = `## [Unreleased]\n\n## [${version}] - ${new Date().toISOString().slice(0, 10)}\n\n${notes}\n\n`;
const updated = changelog.slice(0, match.index) + section + changelog.slice(match.index + match[0].length);
write(changelogFile, `${updated.trimEnd()}\n`);

write(notesFile, `${notes}\n`);

console.log(version);
