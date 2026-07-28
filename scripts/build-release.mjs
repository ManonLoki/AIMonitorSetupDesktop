#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { platform } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import process from "node:process";
import { fileURLToPath } from "node:url";

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const tauriRoot = join(projectRoot, "src-tauri");
const publishDir = join(projectRoot, "publish");
const projectCargoTargetDir = join(tauriRoot, "target");
const packageJson = JSON.parse(readFileSync(join(projectRoot, "package.json"), "utf8"));
const tauriConfig = JSON.parse(readFileSync(join(tauriRoot, "tauri.conf.json"), "utf8"));
const cargoManifest = readFileSync(join(tauriRoot, "Cargo.toml"), "utf8");
const cargoVersion = cargoManifest.match(/\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m)?.[1];
const releaseName = packageJson.releaseName;
const version = packageJson.version;
const productName = tauriConfig.productName;
const mainBinaryName = tauriConfig.mainBinaryName;
const requestedPlatform = process.argv[2] ?? "all";
const supportedPlatforms = new Set(["mac", "win", "all"]);

if (typeof releaseName !== "string" || !/^[A-Za-z0-9]+$/.test(releaseName)) {
  throw new Error("package.json releaseName 必须是非空的 ASCII 字母数字名称");
}
if (version !== tauriConfig.version || version !== cargoVersion) {
  throw new Error(
    `版本不一致：package.json=${version}, tauri.conf.json=${tauriConfig.version}, Cargo.toml=${cargoVersion}`,
  );
}
if (typeof productName !== "string" || typeof mainBinaryName !== "string") {
  throw new Error("tauri.conf.json 必须定义 productName 和 mainBinaryName");
}
if (!supportedPlatforms.has(requestedPlatform)) {
  throw new Error("用法：node scripts/build-release.mjs [mac|win|all]");
}

function run(command, args, env = process.env) {
  console.log(`\n> ${command} ${args.join(" ")}`);
  const result = spawnSync(command, args, {
    cwd: projectRoot,
    env,
    stdio: "inherit",
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${command} 退出，状态码 ${result.status}`);
  }
}

function commandExists(command, env = process.env) {
  const probe = spawnSync(command, ["--version"], {
    cwd: projectRoot,
    env,
    stdio: "ignore",
  });
  return !probe.error;
}

function commandSucceeds(command, args, env = process.env) {
  const result = spawnSync(command, args, {
    cwd: projectRoot,
    env,
    stdio: "ignore",
  });
  return !result.error && result.status === 0;
}

function notarizeAndVerifyMacArtifact(artifact, env) {
  if (!commandSucceeds("xcrun", ["stapler", "validate", artifact], env)) {
    const profile = process.env.AIMONITOR_NOTARY_PROFILE ?? "AIMonitorNotary";
    run(
      "xcrun",
      [
        "notarytool",
        "submit",
        artifact,
        "--keychain-profile",
        profile,
        "--wait",
        "--timeout",
        "30m",
      ],
      env,
    );
    run("xcrun", ["stapler", "staple", artifact], env);
  }
  run("xcrun", ["stapler", "validate", artifact], env);
  run(
    "spctl",
    [
      "--assess",
      "--verbose=2",
      "--type",
      "open",
      "--context",
      "context:primary-signature",
      artifact,
    ],
    env,
  );
}

function filesBelow(directory, extension) {
  if (!existsSync(directory)) return [];
  const files = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const entryPath = join(directory, entry.name);
    if (entry.isDirectory()) files.push(...filesBelow(entryPath, extension));
    if (entry.isFile() && entry.name.toLowerCase().endsWith(extension)) files.push(entryPath);
  }
  return files;
}

function newestArtifact(directory, extension) {
  const candidates = filesBelow(directory, extension)
    .sort((left, right) => statSync(right).mtimeMs - statSync(left).mtimeMs);
  if (candidates.length === 0) {
    throw new Error(`没有在 ${directory} 找到 ${extension} 构建产物`);
  }
  return candidates[0];
}

function buildMac() {
  if (platform() !== "darwin") {
    throw new Error("macOS DMG 必须在 macOS 主机上构建");
  }
  const target = process.env.AIMONITOR_MAC_TARGET ?? "universal-apple-darwin";
  const supportedTargets = new Set([
    "universal-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
  ]);
  if (!supportedTargets.has(target)) {
    throw new Error(`不支持的 AIMONITOR_MAC_TARGET：${target}`);
  }
  if (target === "universal-apple-darwin") {
    run("rustup", ["target", "add", "aarch64-apple-darwin", "x86_64-apple-darwin"]);
  } else {
    run("rustup", ["target", "add", target]);
  }
  const env = {
    ...process.env,
    CARGO_TARGET_DIR: projectCargoTargetDir,
  };
  run("pnpm", ["tauri", "build", "--target", target, "--ci"], env);
  const source = newestArtifact(
    join(tauriRoot, "target", target, "release", "bundle", "dmg"),
    ".dmg",
  );
  run("codesign", ["--verify", "--strict", "--verbose=2", source], env);
  notarizeAndVerifyMacArtifact(source, env);
  const architecture = target === "universal-apple-darwin"
    ? "universal"
    : target.startsWith("aarch64")
      ? "arm64"
      : "x64";
  return {
    source,
    filename: `${releaseName}-macOS-${architecture}-v${version}.dmg`,
  };
}

function buildWindows() {
  if (platform() === "win32") {
    throw new Error("build:win 使用 xwin 交叉编译，请在 macOS 或 Linux 主机运行");
  }
  const target = "x86_64-pc-windows-msvc";
  const llvmPaths = [
    "/opt/homebrew/opt/llvm/bin",
    "/usr/local/opt/llvm/bin",
    "/usr/lib/llvm/bin",
  ].filter(existsSync);
  const env = {
    ...process.env,
    CARGO_TARGET_DIR: projectCargoTargetDir,
    PATH: [...llvmPaths, process.env.PATH].filter(Boolean).join(":"),
    XWIN_CACHE_DIR: process.env.XWIN_CACHE_DIR ?? join(projectRoot, ".xwin-cache"),
  };
  for (const command of ["cargo-xwin", "makensis", "llvm-rc"]) {
    if (!commandExists(command, env)) {
      throw new Error(`Windows 交叉构建缺少命令：${command}`);
    }
  }
  run("rustup", ["target", "add", target], env);
  run(
    "pnpm",
    ["tauri", "build", "--runner", "cargo-xwin", "--target", target, "--ci", "--no-sign"],
    env,
  );
  const source = newestArtifact(
    join(tauriRoot, "target", target, "release", "bundle", "nsis"),
    ".exe",
  );
  return {
    source,
    filename: `${releaseName}-Windows-x64-v${version}-setup.exe`,
  };
}

const artifacts = [];
if (requestedPlatform === "mac" || requestedPlatform === "all") artifacts.push(buildMac());
if (requestedPlatform === "win" || requestedPlatform === "all") artifacts.push(buildWindows());

rmSync(publishDir, { recursive: true, force: true });
mkdirSync(publishDir, { recursive: true });

const checksums = [];
for (const artifact of artifacts) {
  const destination = join(publishDir, artifact.filename);
  copyFileSync(artifact.source, destination);
  const checksum = createHash("sha256").update(readFileSync(destination)).digest("hex");
  checksums.push(`${checksum}  ${basename(destination)}`);
  console.log(`已发布：${destination}`);
}
writeFileSync(
  join(publishDir, `${releaseName}-SHA256SUMS.txt`),
  `${checksums.join("\n")}\n`,
);
console.log(`\npublish 已清理并写入 ${artifacts.length} 个 ${releaseName} 安装包。`);
